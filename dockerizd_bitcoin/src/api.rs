use crate::{
    db::Database,
    model::{self, Input, Output},
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
    match tokio::time::timeout(Duration::from_secs(120), next.run(request)).await {
        Ok(value) => value,
        Err(_) => failure(
            StatusCode::GATEWAY_TIMEOUT,
            "Bitcoin query exceeded time limit",
        ),
    }
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

#[derive(Default)]
pub struct Graph {
    nodes: BTreeMap<String, Value>,
    edges: BTreeMap<String, Value>,
    pub truncated: bool,
}
impl Graph {
    fn node(&mut self, id: &str, kind: &str) {
        self.nodes
            .entry(id.into())
            .or_insert_with(|| json!({"id":id,"address":id,"label":id,"node_type":kind}));
    }
    pub fn value(&self) -> Value {
        json!({"nodes":self.nodes.values().collect::<Vec<_>>(),"edges":self.edges.values().collect::<Vec<_>>(),
            "truncated":self.truncated,"model":"address_transaction_utxo","ownership_claimed":false,
            "amount_allocation_claimed":false})
    }
    fn merge(&mut self, other: Graph, max_nodes: usize) {
        self.truncated |= other.truncated;
        for (id, node) in other.nodes {
            if self.nodes.len() < max_nodes || self.nodes.contains_key(&id) {
                self.nodes.insert(id, node);
            } else {
                self.truncated = true;
            }
        }
        for (id, edge) in other.edges {
            if self
                .nodes
                .contains_key(edge["from_address"].as_str().unwrap_or(""))
                && self
                    .nodes
                    .contains_key(edge["to_address"].as_str().unwrap_or(""))
                && self.edges.len() < 2000
            {
                self.edges.insert(id, edge);
            } else {
                self.truncated = true;
            }
        }
    }
}
pub fn graph_from_rows(root: &str, inputs: &[Input], outputs: &[Output], truncated: bool) -> Graph {
    let mut graph = Graph {
        truncated,
        ..Graph::default()
    };
    graph.node(root, "address");
    for input in inputs {
        let subject = input.output.subject();
        let tx = format!("tx:{}:{}", input.block_hash, input.txid);
        graph.node(
            &subject,
            if input.output.address.is_empty() {
                "script"
            } else {
                "address"
            },
        );
        graph.node(&tx, "transaction");
        let id = format!(
            "{}:{}:in:{}",
            input.block_hash, input.txid, input.input_index
        );
        graph.edges.insert(id.clone(), json!({"id":id,"from_address":subject,"to_address":tx,
            "tx_hash":input.txid,"block_number":input.block_height,"block_timestamp_unix_ms":input.block_time * 1000,
            "amount":input.output.value_sats.to_string(),"asset_id":"native:BTC","decimals":8,
            "transfer_type":"utxo_input","prev_txid":input.prev_txid,"prev_vout":input.prev_vout,"input_index":input.input_index}));
    }
    for output in outputs {
        let subject = output.output.subject();
        let tx = format!("tx:{}:{}", output.block_hash, output.txid);
        graph.node(
            &subject,
            if output.output.address.is_empty() {
                "script"
            } else {
                "address"
            },
        );
        graph.node(&tx, "transaction");
        let id = format!(
            "{}:{}:out:{}",
            output.block_hash, output.txid, output.output_index
        );
        graph.edges.insert(id.clone(), json!({"id":id,"from_address":tx,"to_address":subject,
            "tx_hash":output.txid,"block_number":output.block_height,"block_timestamp_unix_ms":output.block_time * 1000,
            "amount":output.output.value_sats.to_string(),"asset_id":"native:BTC","decimals":8,
            "transfer_type":"utxo_output","output_index":output.output_index,"coinbase":output.coinbase == 1}));
    }
    graph
}
pub async fn wallet_graph(db: &Database, address: &str, limit: usize) -> Result<Graph> {
    let rows: Vec<Value> = db.rows(&format!("SELECT txid,max(block_height) AS height FROM (SELECT txid,block_height FROM ({OUTPUTS}) WHERE address={{address:String}} UNION ALL SELECT txid,block_height FROM ({INPUTS}) WHERE address={{address:String}}) GROUP BY txid ORDER BY height DESC,txid LIMIT {{limit:UInt32}}"), &[("address",address.into()),("limit",(limit + 1).to_string())]).await?;
    let mut ids = rows
        .iter()
        .take(limit)
        .filter_map(|v| v["txid"].as_str())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(graph_from_rows(address, &[], &[], false));
    }
    ids.sort_unstable();
    // Hashes come from stored validated evidence, not URL text; parameterize the array.
    let params = [("ids", format!("['{}']", ids.join("','")))];
    let inputs: Vec<Input> = db.rows(&format!("SELECT * FROM ({INPUTS}) WHERE txid IN {{ids:Array(String)}} ORDER BY block_height DESC,tx_index,input_index LIMIT 9001"), &params).await?;
    let outputs: Vec<Output> = db.rows(&format!("SELECT * FROM ({OUTPUTS}) WHERE txid IN {{ids:Array(String)}} ORDER BY block_height DESC,tx_index,output_index LIMIT 9001"), &params).await?;
    Ok(graph_from_rows(
        address,
        &inputs[..inputs.len().min(9000)],
        &outputs[..outputs.len().min(9000)],
        rows.len() > limit || inputs.len() > 9000 || outputs.len() > 9000,
    ))
}
async fn unchanged(db: &Database, revision: u64) -> Result<()> {
    ensure!(
        db.state().await?.revision == revision,
        "Bitcoin checkpoint changed during query; retry for a coherent snapshot"
    );
    Ok(())
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
        "as_of_block":if state.initialized==1 {Some(state.tip_height)} else {None},
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
    let coverage = db.status().await?;
    let graph = wallet_graph(db, address, limit).await?;
    let stats: Vec<Value> = db.rows(&format!("SELECT count() AS transfer_count,uniqExact(tuple(txid,block_hash)) AS transaction_count FROM (SELECT txid,block_hash FROM ({OUTPUTS}) WHERE address={{address:String}} UNION ALL SELECT txid,block_hash FROM ({INPUTS}) WHERE address={{address:String}})"), &[("address",address.into())]).await?;
    let holdings = wallet_holdings(db, address).await?;
    unchanged(db, state.revision).await?;
    Ok(
        json!({"api_version":"1.0","network_id":crate::NETWORK_ID,"address":address,
        "graph":graph.value(),"fingerprint":stats[0],"holdings":holdings,"data_coverage":coverage,
        "entities":[],"semantic_events":[],"exposure_paths":[],
        "data_quality":{"unresolved_inputs":0,"mempool_included":false,"ownership_claimed":false,
            "limitations":["Transaction connectivity does not allocate an input to a particular output.",
            "Common input and change heuristics are not ownership evidence; CoinJoin is not labeled as laundering.",
            "Holdings cover the ingested confirmed block window; check history_complete and as_of_block."]}}),
    )
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
    let mut graph = Graph::default();
    graph.node(source, "address");
    graph.node(target, "address");
    let mut queue = VecDeque::from([(source.to_owned(), 0)]);
    let mut seen = HashSet::from([source.to_owned()]);
    let mut parent = HashMap::<String, (String, Vec<String>, String)>::new();
    'search: while let Some((current, depth)) = queue.pop_front() {
        if depth >= hops {
            continue;
        }
        let local = wallet_graph(db, &current, 20).await?;
        let ins: BTreeMap<_, _> = local
            .edges
            .values()
            .filter(|e| e["from_address"] == current && e["transfer_type"] == "utxo_input")
            .map(|e| (e["to_address"].as_str().unwrap().to_owned(), e.clone()))
            .collect();
        for input in ins.values() {
            let tx = input["to_address"].as_str().unwrap();
            for output in local
                .edges
                .values()
                .filter(|e| e["from_address"] == tx && e["transfer_type"] == "utxo_output")
            {
                let next = output["to_address"].as_str().unwrap();
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
                queue.push_back((next.into(), depth + 1));
            }
        }
        graph.merge(local, 2000);
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
        "graph":graph.value(),"paths":paths,"max_hops":hops,"searched_addresses":seen.len(),
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
    use crate::{config::DatabaseConfig, model::ScriptOutput};
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
    fn multi_input_graph_does_not_invent_pairwise_transfers() {
        let output = ScriptOutput {
            address: "source".into(),
            script_hex: "00".into(),
            script_type: "x".into(),
            value_sats: 123,
        };
        let input = Input {
            commit_id: "c".into(),
            block_height: 1,
            block_hash: "b".into(),
            block_time: 1,
            txid: "t".into(),
            tx_index: 1,
            input_index: 0,
            prev_txid: "p".into(),
            prev_vout: 0,
            output: output.clone(),
        };
        let target = Output {
            commit_id: "c".into(),
            block_height: 1,
            block_hash: "b".into(),
            block_time: 1,
            txid: "t".into(),
            tx_index: 1,
            output_index: 0,
            coinbase: 0,
            output: ScriptOutput {
                address: "target".into(),
                ..output
            },
        };
        let graph = graph_from_rows("source", &[input], &[target], false).value();
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(graph["edges"][0]["to_address"], "tx:b:t");
        assert_eq!(graph["amount_allocation_claimed"], false);
    }
}
