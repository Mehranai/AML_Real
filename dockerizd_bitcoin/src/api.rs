use crate::{
    db::Database,
    model,
    warehouse::{self, INPUTS, OUTPUTS},
};
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};

#[derive(Clone)]
pub struct Api {
    pub db: Database,
    key: Arc<String>,
    slots: Arc<tokio::sync::Semaphore>,
}
impl Api {
    pub fn new(db: Database, key: String) -> Result<Self> {
        ensure!(
            key.len() >= 32,
            "AML_SERVICE_KEY must have at least 32 characters"
        );
        Ok(Self {
            db,
            key: Arc::new(key),
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
        })
    }
}
pub fn router(api: Api) -> Router {
    let protected = Router::new()
        .route("/status", get(status))
        .route("/api/bitcoin/ingestion/failures", get(ingestion_failures))
        .route(
            "/api/bitcoin/wallet/{address}/investigation",
            get(investigate),
        )
        .route("/api/bitcoin/wallet/{address}/holdings", get(holdings))
        .route(
            "/api/bitcoin/wallet/{address}/intelligence",
            get(intelligence),
        )
        .route(
            "/api/bitcoin/wallet/{address}/intelligence/snapshot",
            get(intelligence_snapshot),
        )
        .route("/api/bitcoin/wallet/{address}/paths/{target}", get(paths))
        .route_layer(middleware::from_fn_with_state(api.clone(), authorize));
    Router::new().merge(protected)
        .route("/", get(|| async { Html(include_str!("../web/index.html")) }))
        .route("/health", get(|| async { Json(json!({"status":"alive","network_id":crate::NETWORK_ID,"service":"bitcoin-api"})) }))
        .route("/assets/bitcoin.js", get(|| async { ([("Content-Type", "text/javascript")], include_str!("../web/bitcoin.js")) }))
        .route("/ready", get(ready)).with_state(api)
}
async fn authorize(
    State(api): State<Api>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let incoming = headers
        .get("X-AML-Service-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let a = Sha256::digest(incoming.as_bytes());
    let b = Sha256::digest(api.key.as_bytes());
    if a.iter().zip(b).fold(0u8, |v, (a, b)| v | (a ^ b)) != 0 {
        return failure(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Ok(_permit) = api.slots.clone().try_acquire_owned() else {
        return failure(
            StatusCode::TOO_MANY_REQUESTS,
            "Bitcoin query capacity reached",
        );
    };
    let mut response = match tokio::time::timeout(Duration::from_secs(120), next.run(request)).await
    {
        Ok(value) => value,
        Err(_) => failure(
            StatusCode::GATEWAY_TIMEOUT,
            "Bitcoin query exceeded time limit",
        ),
    };
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}
fn failure(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        failure(self.0, &self.1)
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        eprintln!("Bitcoin query failed: {error:#}");
        Self(
            StatusCode::SERVICE_UNAVAILABLE,
            "Bitcoin evidence unavailable; inspect service logs".into(),
        )
    }
}
fn address(raw: &str) -> std::result::Result<String, ApiError> {
    model::address(raw).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "invalid Bitcoin mainnet address or checksum".into(),
        )
    })
}
async fn ready(State(api): State<Api>) -> Response {
    match async { api.db.ready().await?; warehouse::migration_ready(&api.db).await }.await {
        Ok(()) => Json(json!({"status":"ready","network_id":crate::NETWORK_ID,"dependencies":{"clickhouse":"ready"},"graph_storage":"main_vm"})).into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status":"not_ready","dependencies":{"clickhouse":"unavailable"}}))).into_response()
    }
}
async fn status(State(api): State<Api>) -> std::result::Result<Json<Value>, ApiError> {
    Ok(Json(api.db.status().await?))
}
async fn ingestion_failures(
    State(api): State<Api>,
    Query(query): Query<crate::failures::Query>,
) -> std::result::Result<Json<Value>, ApiError> {
    query
        .validate()
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(crate::failures::list(&api.db, &query).await?))
}
#[derive(Deserialize)]
pub struct Options {
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    40
}

pub type Graph = aml_warehouse::relationships::Graph;
pub async fn wallet_graph(db: &Database, address: &str, limit: usize) -> Result<Graph> {
    use aml_warehouse::{
        Network,
        relationships::{self, Query, Subject},
    };
    let subject = Subject::address(Network::Bitcoin, address)?;
    Ok(relationships::read_neighborhood(
        &db.warehouse_connection(),
        db.database(),
        Network::Bitcoin,
        &subject,
        Query {
            max_transactions: limit,
            max_edges: 2000,
        },
    )
    .await?
    .graph)
}
async fn unchanged(db: &Database, revision: u64) -> Result<()> {
    ensure!(
        db.state().await?.revision == revision,
        "Bitcoin checkpoint changed during query; retry for a coherent snapshot"
    );
    Ok(())
}
fn source_snapshot(state: &crate::db::SyncState) -> Result<Value> {
    Ok(aml_warehouse::visibility::Snapshot::from_state(crate::warehouse::to_state(state))?.value())
}
pub async fn wallet_holdings(db: &Database, address: &str) -> Result<Value> {
    let state = db.state().await?;
    let rows: Vec<Value> = db.rows(&format!("SELECT toString(sum(value_sats)) AS balance_sats,count() AS utxo_count,toString(sumIf(value_sats,coinbase=1 AND block_height+100>{{height:UInt64}})) AS immature_sats FROM ({}) WHERE address={{address:String}}",warehouse::utxos()), &[("address",address.into()),("height",state.tip_height.to_string())]).await?;
    unchanged(db, state.revision).await?;
    let sats: u64 = rows[0]["balance_sats"].as_str().unwrap_or("0").parse()?;
    let immature: u64 = rows[0]["immature_sats"].as_str().unwrap_or("0").parse()?;
    Ok(
        json!({"network_id":crate::NETWORK_ID,"address":address,"asset_id":"native:BTC","symbol":"BTC","decimals":8,
        "balance_sats":sats.to_string(),"balance_decimal":model::decimal(sats),"immature_sats":immature.to_string(),
        "mature_sats":sats.saturating_sub(immature).to_string(),"utxo_count":rows[0]["utxo_count"],
        "source_snapshot":source_snapshot(&state)?,"as_of_block":if state.initialized==1 {Some(state.tip_height)} else {None},
        "history_complete":state.initialized==1 && state.start_height==0,"scope":"ingested_confirmed_utxos",
        "mempool_included":false,"persisted":true,"revision":state.revision.to_string()}),
    )
}
async fn holdings(
    State(api): State<Api>,
    Path(raw): Path<String>,
) -> std::result::Result<Json<Value>, ApiError> {
    Ok(Json(wallet_holdings(&api.db, &address(&raw)?).await?))
}
pub async fn investigation(db: &Database, address: &str, limit: usize) -> Result<Value> {
    let state = db.state().await?;
    let attribution = crate::intelligence::read(db, address).await?;
    let coverage = db.status().await?;
    let graph = wallet_graph(db, address, limit).await?;
    let stats: Vec<Value> = db.rows(&format!("SELECT count() AS transfer_count,uniqExact(tuple(txid,block_hash)) AS transaction_count FROM (SELECT txid,block_hash FROM ({OUTPUTS}) WHERE address={{address:String}} UNION ALL SELECT txid,block_hash FROM ({INPUTS}) WHERE address={{address:String}})"), &[("address",address.into())]).await?;
    let holdings = wallet_holdings(db, address).await?;
    unchanged(db, state.revision).await?;
    ensure!(
        crate::intelligence::read(db, address).await?.snapshot == attribution.snapshot,
        "Bitcoin intelligence changed during investigation; retry"
    );
    Ok(
        json!({"api_version":"1.0","network_id":crate::NETWORK_ID,"address":address,
        "source_snapshot":source_snapshot(&state)?,"graph":graph.value(),"fingerprint":stats[0],"holdings":holdings,"data_coverage":coverage,
        "entities":attribution.entities,"intelligence_snapshot":attribution.snapshot,
        "intelligence":{"claims":attribution.claims,"sources":attribution.sources,"automatic_approval":false},
        "semantic_events":[],"exposure_paths":[],
        "data_quality":{"unresolved_inputs":0,"mempool_included":false,"ownership_claimed":false,
            "limitations":["Transaction connectivity does not allocate an input to a particular output.",
            "Common input and change heuristics are not ownership evidence; CoinJoin is not labeled as laundering.",
            "Holdings cover the ingested confirmed block window; check history_complete and as_of_block."]}}),
    )
}
async fn intelligence(
    State(api): State<Api>,
    Path(raw): Path<String>,
) -> std::result::Result<Json<Value>, ApiError> {
    let address = address(&raw)?;
    let value = crate::intelligence::read(&api.db, &address).await?;
    Ok(Json(
        json!({"network_id":crate::NETWORK_ID,"address":address,"intelligence_snapshot":value.snapshot,"entities":value.entities,"claims":value.claims,"sources":value.sources}),
    ))
}
async fn intelligence_snapshot(
    State(api): State<Api>,
    Path(raw): Path<String>,
) -> std::result::Result<Json<Value>, ApiError> {
    let value = crate::intelligence::read(&api.db, &address(&raw)?).await?;
    Ok(Json(
        json!({"network_id":crate::NETWORK_ID,"intelligence_snapshot":value.snapshot}),
    ))
}
async fn investigate(
    State(api): State<Api>,
    Path(raw): Path<String>,
    Query(options): Query<Options>,
) -> std::result::Result<Json<Value>, ApiError> {
    if !(1..=100).contains(&options.limit) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "limit must be 1..100".into(),
        ));
    }
    Ok(Json(
        investigation(&api.db, &address(&raw)?, options.limit).await?,
    ))
}
#[derive(Deserialize)]
pub struct PathOptions {
    #[serde(default = "default_hops")]
    max_hops: usize,
    #[serde(default = "default_nodes")]
    max_nodes: usize,
}
fn default_hops() -> usize {
    3
}
fn default_nodes() -> usize {
    200
}

pub async fn find_paths(
    db: &Database,
    source: &str,
    target: &str,
    hops: usize,
    cap: usize,
) -> Result<Value> {
    let state = db.state().await?;
    let mut graph = Graph::new(aml_warehouse::Network::Bitcoin);
    graph.node(aml_warehouse::relationships::Subject::address(
        aml_warehouse::Network::Bitcoin,
        source,
    )?)?;
    graph.node(aml_warehouse::relationships::Subject::address(
        aml_warehouse::Network::Bitcoin,
        target,
    )?)?;
    let mut queue = VecDeque::from([(source.to_owned(), 0, None::<(u64, u64)>)]);
    let mut seen = HashSet::from([source.to_owned()]);
    let mut parent = HashMap::<String, (String, Vec<String>, String)>::new();
    'search: while let Some((current, depth, after)) = queue.pop_front() {
        if depth >= hops {
            continue;
        }
        let local = wallet_graph(db, &current, 20).await?;
        let ins: BTreeMap<_, _> = local
            .edges
            .values()
            .filter(|e| e["from_subject_id"] == current && e["transfer_type"] == "utxo_input")
            .filter(|e| {
                after.is_none_or(|p| {
                    (
                        e["block_number"].as_u64().unwrap_or(0),
                        e["transaction_index"].as_u64().unwrap_or(0),
                    ) > p
                })
            })
            .map(|e| (e["to_subject_id"].as_str().unwrap().to_owned(), e.clone()))
            .collect();
        for input in ins.values() {
            let tx = input["to_subject_id"].as_str().unwrap();
            for output in local
                .edges
                .values()
                .filter(|e| e["from_subject_id"] == tx && e["transfer_type"] == "utxo_output")
            {
                let next = output["to_subject_id"].as_str().unwrap();
                if next.starts_with("script:") || seen.contains(next) {
                    continue;
                }
                if seen.len() >= cap {
                    graph.truncated = true;
                    break;
                }
                seen.insert(next.into());
                parent.insert(
                    next.into(),
                    (
                        current.clone(),
                        vec![
                            input["id"].as_str().unwrap().into(),
                            output["id"].as_str().unwrap().into(),
                        ],
                        tx.into(),
                    ),
                );
                queue.push_back((
                    next.into(),
                    depth + 1,
                    Some((
                        input["block_number"].as_u64().unwrap(),
                        input["transaction_index"].as_u64().unwrap(),
                    )),
                ));
            }
        }
        graph.merge(local, 2000)?;
        if seen.contains(target) {
            break 'search;
        }
    }
    let mut paths = Vec::new();
    if parent.contains_key(target) {
        let mut cursor = target.to_owned();
        let mut nodes = vec![cursor.clone()];
        let mut edges = Vec::new();
        let mut addresses = vec![cursor.clone()];
        while cursor != source {
            let (previous, ids, tx) = &parent[&cursor];
            nodes.push(tx.clone());
            nodes.push(previous.clone());
            edges.extend(ids.iter().rev().cloned());
            cursor = previous.clone();
            addresses.push(cursor.clone());
        }
        nodes.reverse();
        edges.reverse();
        addresses.reverse();
        // Merge caps can drop context; a path is returned only with all its evidence.
        if edges.iter().all(|id| graph.edges.contains_key(id)) {
            paths.push(json!({"addresses":addresses,"node_ids":nodes,"edge_ids":edges,"hop_count":addresses.len()-1,
                "meaning":"possible_transaction_connectivity","fund_continuity_claimed":false}));
        } else {
            graph.truncated = true;
        }
    }
    unchanged(db, state.revision).await?;
    Ok(
        json!({"api_version":"1.0","network_id":crate::NETWORK_ID,"source":source,"target":target,
        "source_snapshot":source_snapshot(&state)?,"graph":graph.value(),"paths":paths,"max_hops":hops,"searched_addresses":seen.len(),
        "search_complete":false,"path_limit":1,"revision":state.revision.to_string(),
        "limitations":["Bounded transaction connectivity search; absence of a result does not prove no path.",
        "Address reuse and multiple inputs prevent attribution of particular funds along this path."]}),
    )
}
async fn paths(
    State(api): State<Api>,
    Path((source, target)): Path<(String, String)>,
    Query(options): Query<PathOptions>,
) -> std::result::Result<Json<Value>, ApiError> {
    let source = address(&source)?;
    let target = address(&target)?;
    if source == target
        || !(1..=10).contains(&options.max_hops)
        || !(2..=200).contains(&options.max_nodes)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "distinct addresses, max_hops 1..10, max_nodes 2..200 required".into(),
        ));
    }
    Ok(Json(
        find_paths(
            &api.db,
            &source,
            &target,
            options.max_hops,
            options.max_nodes,
        )
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DatabaseConfig;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    #[tokio::test]
    async fn service_auth_address_and_limits_are_checked_before_storage() {
        let config = DatabaseConfig {
            url: "http://127.0.0.1:1".parse().unwrap(),
            user: "x".into(),
            password: "x".into(),
            database: "bitcoin_aml".into(),
        };
        let router = router(Api::new(Database::new(config).unwrap(), "k".repeat(32)).unwrap());
        for (path, key, status) in [
            ("/status", false, 401),
            ("/health", false, 200),
            ("/api/bitcoin/ingestion/failures", false, 401),
            ("/api/bitcoin/ingestion/failures?limit=201", true, 400),
            ("/api/bitcoin/ingestion/failures?status=unknown", true, 400),
            ("/api/bitcoin/wallet/invalid/investigation", true, 400),
            (
                "/api/bitcoin/wallet/1BoatSLRHtKNngkdXEeobR76b53LETtpyT/investigation?limit=1000",
                true,
                400,
            ),
        ] {
            let mut req = Request::builder().uri(path);
            if key {
                req = req.header("X-AML-Service-Key", "k".repeat(32));
            }
            let response = router
                .clone()
                .oneshot(req.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
        }
    }
    #[test]
    fn typed_transaction_nodes_are_not_addresses() {
        use aml_warehouse::{
            Network,
            relationships::{Graph, Kind, Subject},
        };
        let mut graph = Graph::new(Network::Bitcoin);
        let tx = format!("tx:{}:{}", "a".repeat(64), "b".repeat(64));
        graph
            .node(Subject::address(Network::Bitcoin, "1BoatSLRHtKNngkdXEeobR76b53LETtpyT").unwrap())
            .unwrap();
        graph
            .node(Subject::new(Network::Bitcoin, Kind::Transaction, &tx).unwrap())
            .unwrap();
        let value = graph.value();
        assert_eq!(value["nodes"].as_array().unwrap().len(), 2);
        assert!(value["edges"].as_array().unwrap().is_empty());
        assert!(
            value["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["node_type"] == "transaction")
                .unwrap()["address"]
                .is_null()
        );
        assert_eq!(value["amount_allocation_claimed"], false);
    }
}
