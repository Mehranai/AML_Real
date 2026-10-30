mod bitcoin_clusters;
#[cfg(test)]
mod bitcoin_exposure_tests;
mod risk;
mod store;

use anyhow::{Context, ensure};
use axum::{
    Json, Router,
    extract::{OriginalUri, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use nanoid::nanoid;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use store::Store;

#[derive(Clone)]
struct App {
    store: Store,
    http: reqwest::Client,
    upstreams: HashMap<String, String>,
    key: String,
    ttl_ms: i64,
    requests: Arc<tokio::sync::Semaphore>,
}

#[derive(Debug)]
struct Failure(StatusCode, &'static str);
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
fn internal(error: impl std::fmt::Display) -> Failure {
    eprintln!("Analytical Node: {error}");
    Failure(
        StatusCode::SERVICE_UNAVAILABLE,
        "Central investigation service is unavailable. Retry without changing the current graph.",
    )
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as i64
}
fn required(name: &str) -> anyhow::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .with_context(|| format!("{name} is required"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--healthcheck") {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .build()?
            .get("http://127.0.0.1:7001/ready")
            .send()
            .await?
            .error_for_status()?;
        return Ok(());
    }
    let key = required("AML_SERVICE_KEY")?;
    ensure!(
        key.len() >= 32,
        "AML_SERVICE_KEY must contain at least 32 characters"
    );
    let ttl: i64 = std::env::var("AML_GRAPH_TTL_HOURS")
        .unwrap_or("24".into())
        .parse()?;
    ensure!(
        (1..=168).contains(&ttl),
        "AML_GRAPH_TTL_HOURS must be 1..168"
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let db_http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let store = Store::new(
        db_http,
        &required("AML_NEO4J_HTTP_URL")?,
        required("NEO4J_PASSWORD")?,
    );
    store.initialize().await?;
    let mut upstreams = HashMap::new();
    for (chain, env) in [
        ("tron", "AML_TRON_UPSTREAM"),
        ("ethereum", "AML_ETHEREUM_UPSTREAM"),
        ("bsc", "AML_BSC_UPSTREAM"),
        ("bitcoin", "AML_BITCOIN_UPSTREAM"),
    ] {
        let url = required(env)?;
        let parsed = reqwest::Url::parse(&url)?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https")
                && parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.path() == "/",
            "invalid {env}"
        );
        upstreams.insert(chain.to_string(), url.trim_end_matches('/').to_string());
    }
    let app = App {
        store: store.clone(),
        http,
        upstreams,
        key,
        ttl_ms: ttl * 3_600_000,
        requests: Arc::new(tokio::sync::Semaphore::new(8)),
    };
    let janitor = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(error) = store.cleanup(now()).await {
                eprintln!("Snapshot cleanup failed: {error:#}");
            }
        }
    });
    let router = Router::new()
        .route("/health", get(|| async { Json(json!({"status":"alive"})) }))
        .route("/ready", get(ready))
        .route(
            "/api/{network}/wallet/{address}/investigation",
            get(investigate),
        )
        .route("/api/{network}/wallet/{address}/paths/{target}", get(paths))
        .route("/api/investigations", get(saved))
        .route("/api/investigations/{id}", get(reopen))
        .route("/api/investigations/{id}/export", post(export))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(
        std::env::var("AML_ANALYTICAL_ADDR").unwrap_or("0.0.0.0:7001".into()),
    )
    .await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    janitor.abort();
    Ok(())
}

async fn ready(State(app): State<App>) -> Result<Json<Value>, Failure> {
    app.store
        .query("RETURN 1", json!({}))
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({"status":"ready","dependencies":{"neo4j":"ready"}}),
    ))
}

struct Owner {
    hash: String,
    cookie: Option<String>,
}
fn owner(app: &App, headers: &HeaderMap) -> Result<Owner, Failure> {
    let supplied = headers
        .get("x-aml-service-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let left = Sha256::digest(supplied.as_bytes());
    let right = Sha256::digest(app.key.as_bytes());
    let difference = left
        .iter()
        .zip(right.iter())
        .fold(0_u8, |v, (a, b)| v | (a ^ b));
    if supplied.is_empty() || difference != 0 {
        return Err(Failure(
            StatusCode::UNAUTHORIZED,
            "Service authentication required",
        ));
    }
    let user = headers
        .get("x-aml-user")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty());
    if let Some(user) = user {
        return Ok(Owner {
            hash: format!("{:x}", Sha256::digest(format!("user:{user}"))),
            cookie: None,
        });
    }
    let existing = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .find_map(|part| part.trim().strip_prefix("aml_session="))
        })
        .filter(|id| {
            id.len() == 64
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        });
    let id = existing.map(str::to_string).unwrap_or_else(|| nanoid!(64));
    let cookie = existing.is_none().then(|| {
        format!(
            "aml_session={id}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000{}",
            if headers
                .get("x-forwarded-proto")
                .is_some_and(|v| v == "https")
            {
                "; Secure"
            } else {
                ""
            }
        )
    });
    Ok(Owner {
        hash: format!("{:x}", Sha256::digest(format!("session:{id}"))),
        cookie,
    })
}
fn response(value: Value, owner: Owner) -> Response {
    let mut result = Json(value).into_response();
    result.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    if let Some(cookie) = owner.cookie {
        result
            .headers_mut()
            .insert(header::SET_COOKIE, cookie.parse().expect("valid cookie"));
    }
    result
}

async fn investigate(
    State(app): State<App>,
    headers: HeaderMap,
    Path((network, address)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
) -> Result<Response, Failure> {
    create(app, headers, network, address, None, uri).await
}
async fn paths(
    State(app): State<App>,
    headers: HeaderMap,
    Path((network, address, target)): Path<(String, String, String)>,
    OriginalUri(uri): OriginalUri,
) -> Result<Response, Failure> {
    create(app, headers, network, address, Some(target), uri).await
}
async fn create(
    app: App,
    headers: HeaderMap,
    network: String,
    address: String,
    target: Option<String>,
    uri: axum::http::Uri,
) -> Result<Response, Failure> {
    let owner = owner(&app, &headers)?;
    let _permit = app.requests.try_acquire().map_err(|_| {
        Failure(
            StatusCode::TOO_MANY_REQUESTS,
            "Investigation capacity reached; retry shortly",
        )
    })?;
    let upstream = app
        .upstreams
        .get(&network)
        .ok_or(Failure(StatusCode::NOT_FOUND, "Unsupported network"))?;
    if !valid_address(&network, &address)
        || target.as_ref().is_some_and(|a| !valid_address(&network, a))
    {
        return Err(Failure(StatusCode::BAD_REQUEST, "Invalid wallet address"));
    }
    let mut remote = app
        .http
        .get(format!("{upstream}{uri}"))
        .header("x-aml-service-key", &app.key)
        .send()
        .await
        .map_err(internal)?;
    if !remote.status().is_success() {
        if network == "bitcoin" && remote.status() == StatusCode::UNPROCESSABLE_ENTITY {
            return Err(Failure(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Bitcoin activity scope is too large; narrow the block or UTC time filters",
            ));
        }
        let code = if remote.status().is_client_error() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::BAD_GATEWAY
        };
        return Err(Failure(
            code,
            "Network investigation failed; check the address, parameters and chain readiness",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = remote.chunk().await.map_err(internal)? {
        if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
            return Err(Failure(
                StatusCode::BAD_GATEWAY,
                "Investigation exceeds snapshot size limit",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut data: Value = serde_json::from_slice(&bytes).map_err(internal)?;
    let network_id = if network == "tron" {
        "tron:mainnet"
    } else if network == "bsc" {
        "eip155:56"
    } else if network == "bitcoin" {
        "bip122:000000000019d6689c085ae165831e93"
    } else {
        "eip155:1"
    };
    if data.get("network_id").is_some_and(|v| v != network_id) {
        return Err(Failure(
            StatusCode::BAD_GATEWAY,
            "Upstream network identity mismatch",
        ));
    }
    data["network_id"] = json!(network_id);
    if target.is_none() {
        // The chain supplies evidence. A single centrally versioned policy scores both networks.
        let assessed = risk::assess(&network, &data);
        if network == "bitcoin"
            && data["risk_snapshot"].is_object()
            && (!aml_risk_policy::bitcoin_binding_matches(&data["risk_snapshot"], &data)
                || data["risk_engine"] != assessed)
        {
            return Err(Failure(
                StatusCode::BAD_GATEWAY,
                "Bitcoin persisted risk differs from the central policy or evidence",
            ));
        }
        data["risk_engine"] = assessed;
        let immutable_holdings =
            network == "bitcoin" && data["holdings"]["holdings_snapshot"].is_object();
        if !immutable_holdings
            && let Some(holdings) = data.get_mut("holdings").and_then(Value::as_object_mut)
        {
            // Returned only after Store::create commits the complete payload.
            holdings.insert("persisted".into(), json!(true));
            holdings.insert("storage".into(), json!("central_investigation_snapshot"));
        }
    }
    refresh_source_validity(&app, &mut data, &network).await;
    let id = nanoid!(32);
    let (nodes, edges) = normalize_graph(&id, network_id, &network, &data).map_err(internal)?;
    let timestamp = now();
    let payload = serde_json::to_string(&data).map_err(internal)?;
    let metadata = json!({"id":id,"network_id":network_id,"network":network,
        "mode":if target.is_some(){"paths"}else{"wallet"},"address":address,"target":target,
        "state":"temporary","created_at_unix_ms":timestamp,"expires_at_unix_ms":timestamp+app.ttl_ms,
        "snapshot_hash":format!("{:x}",Sha256::digest(payload.as_bytes()))});
    app.store
        .create(
            json!({"metadata":metadata,"owner":owner.hash,"payload":payload,
        "nodes":nodes,"edges":edges}),
        )
        .await
        .map_err(internal)?;
    data["investigation"] = metadata;
    Ok(response(data, owner))
}

fn valid_address(network: &str, address: &str) -> bool {
    if network == "bitcoin" {
        // Full checksum and network validation is performed by the Bitcoin adapter.
        (26..=90).contains(&address.len())
            && (address.starts_with('1')
                || address.starts_with('3')
                || address.to_ascii_lowercase().starts_with("bc1"))
            && address.bytes().all(|b| b.is_ascii_alphanumeric())
    } else if matches!(network, "ethereum" | "bsc") {
        address.len() == 42
            && address.starts_with("0x")
            && address[2..].bytes().all(|b| b.is_ascii_hexdigit())
    } else {
        address.len() == 34
            && address.starts_with('T')
            && address.bytes().all(|b| b.is_ascii_alphanumeric())
    }
}

fn normalize_graph(
    id: &str,
    network_id: &str,
    network: &str,
    data: &Value,
) -> anyhow::Result<(Vec<Value>, Vec<Value>)> {
    let graph = data.get("graph").unwrap_or(data);
    let raw_nodes = graph["nodes"].as_array().context("missing graph nodes")?;
    let raw_edges = graph["edges"].as_array().context("missing graph edges")?;
    ensure!(
        raw_nodes.len() <= 20_000 && raw_edges.len() <= 20_000,
        "graph safety cap exceeded"
    );
    let mut nodes = BTreeMap::new();
    let key = |address: &str| format!("{id}:{network_id}:{address}");
    for node in raw_nodes {
        let typed = network == "bitcoin" && node["subject_id"].is_string();
        let address = if typed {
            risk::text(&node["subject_id"])
        } else if network == "tron" {
            risk::text(&node["id"])
        } else {
            risk::text(&node["address"])
        };
        ensure!(!address.is_empty(), "empty graph node");
        let kind = risk::text(&node["node_type"]);
        if typed {
            ensure!(
                ["address", "transaction", "script"].contains(&kind),
                "unknown Bitcoin graph subject type"
            );
            ensure!(
                risk::text(&node["subject_type"]) == kind,
                "inconsistent subject type"
            );
            ensure!(
                if kind == "address" {
                    risk::text(&node["address"]) == address
                } else {
                    node["address"].is_null()
                },
                "non-address subject has an address value"
            );
        }
        let address_value = if network == "bitcoin" && ["transaction", "script"].contains(&kind) {
            Value::Null
        } else {
            json!(address)
        };
        nodes.insert(address.to_string(),json!({"key":key(address),"address":address_value.clone(),
            "address_value":address_value,"subject_id":address,"subject_type":if kind.is_empty(){"address"}else{kind},
            "subject_key":if typed{format!("{network_id}:{kind}:{address}")}else{format!("{network_id}:{address}")},"label":node.get("label").unwrap_or(&json!(address)),
            "node_type":node.get("node_type").unwrap_or(&json!("address")),
            "evidence_json":node.to_string()}));
    }
    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for edge in raw_edges {
        let typed = network == "bitcoin"
            && edge["from_subject_id"].is_string()
            && edge["to_subject_id"].is_string();
        let from = if typed {
            risk::text(&edge["from_subject_id"])
        } else {
            risk::text(
                &edge[if network == "tron" {
                    "from"
                } else {
                    "from_address"
                }],
            )
        };
        let to = if typed {
            risk::text(&edge["to_subject_id"])
        } else {
            risk::text(
                &edge[if network == "tron" {
                    "to"
                } else {
                    "to_address"
                }],
            )
        };
        ensure!(
            nodes.contains_key(from) && nodes.contains_key(to),
            "edge endpoint missing"
        );
        if network == "bitcoin" {
            let from_kind = risk::text(&nodes[from]["subject_type"]);
            let to_kind = risk::text(&nodes[to]["subject_type"]);
            if typed {
                ensure!(
                    risk::text(&edge["from_subject_type"]) == from_kind
                        && risk::text(&edge["to_subject_type"]) == to_kind,
                    "Bitcoin edge/subject types disagree"
                );
            }
            let valid = match risk::text(&edge["transfer_type"]) {
                "utxo_input" => {
                    ["address", "script"].contains(&from_kind) && to_kind == "transaction"
                }
                "utxo_output" => {
                    from_kind == "transaction" && ["address", "script"].contains(&to_kind)
                }
                _ => false,
            };
            ensure!(
                valid,
                "Bitcoin graph cannot assert a pairwise address transfer"
            );
            for claim in [
                "fund_continuity_claimed",
                "amount_allocation_claimed",
                "ownership_claimed",
            ] {
                ensure!(
                    if typed {
                        edge[claim] == false
                    } else {
                        edge[claim] != true
                    },
                    "unsupported Bitcoin graph claim"
                );
            }
        }
        let edge_id = risk::text(&edge["id"]);
        ensure!(
            !edge_id.is_empty() && seen.insert(edge_id),
            "missing or duplicate edge ID"
        );
        // Preserve raw amounts as strings; never coerce token quantities into floats.
        edges.push(json!({"edge_id":edge_id,"from_key":key(from),"to_key":key(to),
            "from_address":if typed{edge["from_address"].clone()}else{json!(from)},"to_address":if typed{edge["to_address"].clone()}else{json!(to)},
            "from_subject_id":from,"to_subject_id":to,"from_subject_type":nodes[from]["subject_type"],"to_subject_type":nodes[to]["subject_type"],
            "fund_continuity_claimed":if typed{json!(false)}else{Value::Null},"amount_allocation_claimed":if typed{json!(false)}else{Value::Null},
            "tx_hash":edge["tx_hash"],"amount":edge["amount"],
            "asset_id":edge.get("asset_id").or_else(||edge.get("token_address")).unwrap_or(&Value::Null),
            "block_number":edge["block_number"],"transfer_type":edge["transfer_type"],"evidence_json":edge.to_string()}));
    }
    Ok((nodes.into_values().collect(), edges))
}

async fn saved(
    State(app): State<App>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, Failure> {
    let owner = owner(&app, &headers)?;
    let before = query
        .get("before")
        .and_then(|v| v.parse().ok())
        .unwrap_or(i64::MAX);
    let rows = app
        .store
        .list(&owner.hash, before)
        .await
        .map_err(internal)?;
    Ok(response(
        json!({"investigations":rows,"next_before":rows.last().map(|r|r["created_at_unix_ms"].clone())}),
        owner,
    ))
}
async fn reopen(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let owner = owner(&app, &headers)?;
    let mut data = app
        .store
        .read(&id, &owner.hash, now())
        .await
        .map_err(internal)?
        .ok_or(Failure(
            StatusCode::NOT_FOUND,
            "Investigation not found or expired",
        ))?;
    let network = data["investigation"]["network"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    refresh_source_validity(&app, &mut data, &network).await;
    Ok(response(data, owner))
}

fn annotate_source_validity(data: &mut Value, current: Option<&Value>) {
    let source = &data["source_snapshot"];
    let expected = source["snapshot_id"]
        .as_str()
        .filter(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut status = if let Some(current) = current {
        let actual = &current["source_snapshot"];
        if expected.is_none()
            || source["contract"] != "canonical_v1"
            || actual["contract"] != "canonical_v1"
            || source["network_id"] != data["network_id"]
            || current["network_id"] != data["network_id"]
            || actual["network_id"] != data["network_id"]
        {
            "unverified"
        } else if actual["snapshot_id"] == source["snapshot_id"]
            && actual["source_revision"] == source["source_revision"]
            && actual["block_hash"] == source["block_hash"]
            && actual["as_of_block"] == source["as_of_block"]
        {
            "current"
        } else {
            "stale"
        }
    } else {
        "unverified"
    };
    if status == "current"
        && (data.get("intelligence_snapshot").is_some()
            || data["entities"].as_array().is_some_and(|v| !v.is_empty()))
    {
        let expected = &data["intelligence_snapshot"];
        let actual = &current.unwrap()["intelligence_snapshot"];
        let valid = |value: &Value| {
            value["contract"] == "bitcoin_intelligence_v1"
                && value["network_id"] == data["network_id"]
                && value["address"] == data["address"]
                && value["snapshot_id"]
                    .as_str()
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        status = if !valid(expected) || !valid(actual) {
            "unverified"
        } else if actual["snapshot_id"] != expected["snapshot_id"] {
            "stale"
        } else {
            "current"
        };
    }
    if status == "current" && data["exposure_snapshot"].is_object() {
        let expected = &data["exposure_snapshot"];
        let actual = &current.unwrap()["exposure_snapshot"];
        let scoped = |v: &Value| {
            v["contract"] == "bitcoin_exposure_v1"
                && v["network_id"] == data["network_id"]
                && v["address"] == data["address"]
                && v["run_id"] == expected["run_id"]
                && v["snapshot_id"]
                    .as_str()
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        status = if !scoped(expected) || !scoped(actual) {
            "unverified"
        } else if actual["current"] != true || actual["snapshot_id"] != expected["snapshot_id"] {
            "stale"
        } else {
            "current"
        };
    }
    if status == "current" && data["semantic_snapshot"].is_object() {
        let expected = &data["semantic_snapshot"];
        let actual = &current.unwrap()["semantic_config"];
        let mut unsigned = expected.clone();
        unsigned.as_object_mut().unwrap().remove("snapshot_id");
        let bound = &data["fingerprint"]["semantic_detection"];
        let valid_hash = |v: &Value| {
            v.as_str()
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        status = if expected["contract"] != "bitcoin_semantics_v1"
            || actual["contract"] != "bitcoin_semantics_config_v1"
            || expected["network_id"] != data["network_id"]
            || actual["network_id"] != data["network_id"]
            || expected["source_snapshot_id"] != source["snapshot_id"]
            || expected["snapshot_id"] != aml_risk_policy::digest(&unsigned)
            || expected["events_hash"] != aml_risk_policy::digest(&data["semantic_events"])
            || expected["coverage_hash"] != aml_risk_policy::digest(&data["semantic_coverage"])
            || expected["snapshot_id"] != bound["snapshot_id"]
            || expected["definition_hash"] != bound["definition_hash"]
            || expected["detector_version"] != bound["detector_version"]
            || bound["coverage"] != data["semantic_coverage"]
            || !valid_hash(&expected["definition_hash"])
            || !valid_hash(&actual["definition_hash"])
        {
            "unverified"
        } else if expected["detector_version"] != actual["detector_version"]
            || expected["definition_hash"] != actual["definition_hash"]
        {
            "stale"
        } else {
            "current"
        };
    }
    if status == "current" && data["cluster_snapshot"].is_object() {
        status = bitcoin_clusters::validity(data, current.unwrap());
    }
    if status == "current" && data["holdings"]["holdings_snapshot"].is_object() {
        let holdings = &data["holdings"];
        let frame = &holdings["holdings_snapshot"];
        let mut body = holdings.clone();
        body.as_object_mut().unwrap().remove("holdings_snapshot");
        let mut unsigned = frame.clone();
        unsigned.as_object_mut().unwrap().remove("snapshot_id");
        let metadata = &holdings["native_asset_snapshot"];
        let actual = &current.unwrap()["native_asset_snapshot"];
        let valid = |v: &Value| {
            v["contract"] == "bitcoin_native_asset_v1"
                && v["network_id"] == data["network_id"]
                && v["asset_id"] == "native:BTC"
                && v["snapshot_id"]
                    .as_str()
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        status = if frame["contract"] != "bitcoin_holdings_v1"
            || holdings["network_id"] != data["network_id"]
            || holdings["address"] != data["address"]
            || holdings["source_snapshot"] != *source
            || frame["network_id"] != data["network_id"]
            || frame["address"] != data["address"]
            || frame["source_snapshot_id"] != source["snapshot_id"]
            || frame["evidence_hash"] != aml_risk_policy::digest(&body)
            || frame["snapshot_id"] != aml_risk_policy::digest(&unsigned)
            || frame["metadata_snapshot_id"] != metadata["snapshot_id"]
            || !valid(metadata)
            || !valid(actual)
        {
            "unverified"
        } else if actual["snapshot_id"] != metadata["snapshot_id"] {
            "stale"
        } else {
            "current"
        };
    }
    if status == "current" && data["risk_snapshot"].is_object() {
        let expected = &data["risk_snapshot"];
        let actual = &current.unwrap()["risk_snapshot"];
        let scoped = |v: &Value| {
            v["contract"] == "bitcoin_risk_v1"
                && v["network_id"] == data["network_id"]
                && v["address"] == data["address"]
                && v["assessment_id"] == expected["assessment_id"]
                && v["snapshot_id"]
                    .as_str()
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        status = if !scoped(expected)
            || !scoped(actual)
            || !aml_risk_policy::bitcoin_binding_matches(expected, data)
        {
            "unverified"
        } else if actual["current"] != true || actual["snapshot_id"] != expected["snapshot_id"] {
            "stale"
        } else {
            "current"
        };
    }
    data["source_validity"] = json!({"status":status,"checked_at_unix_ms":now(),"current_risk_usable":status=="current","historical_evidence_preserved":true});
    if status != "current" && data.get("risk_engine").is_some() {
        if data.get("historical_risk_engine").is_none() {
            data["historical_risk_engine"] = data["risk_engine"].clone();
        }
        data["risk_engine"] = json!({"risk_score":null,"risk_level":"UNKNOWN","signals":[],"policy_version":"source_snapshot_invalid",
            "limitations":["Historical snapshot: its source is changed or could not be verified. Run a new investigation before using current risk."]});
    }
}
async fn refresh_source_validity(app: &App, data: &mut Value, network: &str) {
    if network != "bitcoin" {
        return;
    }
    let current = async {
        let upstream = app.upstreams.get(network).context("missing upstream")?;
        let mut response = app
            .http
            .get(format!("{upstream}/status"))
            .header("x-aml-service-key", &app.key)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        ensure!(response.status().is_success(), "source status unavailable");
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 65536,
                "source status exceeds bound"
            );
            bytes.extend_from_slice(&chunk);
        }
        let mut current: Value = serde_json::from_slice(&bytes)?;
        if data.get("intelligence_snapshot").is_some()
            || data["entities"].as_array().is_some_and(|v| !v.is_empty())
        {
            let address = data["address"]
                .as_str()
                .filter(|a| valid_address("bitcoin", a))
                .context("invalid intelligence address")?;
            let mut response = app
                .http
                .get(format!(
                    "{upstream}/api/bitcoin/wallet/{address}/intelligence/snapshot"
                ))
                .header("x-aml-service-key", &app.key)
                .timeout(Duration::from_secs(5))
                .send()
                .await?;
            ensure!(
                response.status().is_success(),
                "intelligence snapshot unavailable"
            );
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    bytes.len() + chunk.len() <= 65536,
                    "intelligence snapshot exceeds bound"
                );
                bytes.extend_from_slice(&chunk);
            }
            let attribution: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                attribution["network_id"] == data["network_id"],
                "intelligence network mismatch"
            );
            current["intelligence_snapshot"] = attribution["intelligence_snapshot"].clone();
        }
        if data["exposure_snapshot"].is_object() {
            let address = data["address"]
                .as_str()
                .filter(|a| valid_address("bitcoin", a))
                .context("invalid exposure address")?;
            let run = data["exposure_snapshot"]["run_id"]
                .as_str()
                .context("missing exposure run")?;
            ensure!(
                run.strip_prefix("btc-exp-")
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
                "invalid exposure run"
            );
            let mut response = app
                .http
                .get(format!(
                    "{upstream}/api/bitcoin/wallet/{address}/exposure/{run}/snapshot"
                ))
                .header("x-aml-service-key", &app.key)
                .timeout(Duration::from_secs(5))
                .send()
                .await?;
            ensure!(
                response.status().is_success(),
                "exposure freshness unavailable"
            );
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    bytes.len() + chunk.len() <= 65536,
                    "exposure snapshot exceeds bound"
                );
                bytes.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                value["network_id"] == data["network_id"],
                "exposure network mismatch"
            );
            current["exposure_snapshot"] = value["exposure_snapshot"].clone();
        }
        if data["semantic_snapshot"].is_object() {
            let mut response = app
                .http
                .get(format!("{upstream}/api/bitcoin/semantics/config"))
                .header("x-aml-service-key", &app.key)
                .timeout(Duration::from_secs(5))
                .send()
                .await?;
            ensure!(
                response.status().is_success(),
                "semantic detector config unavailable"
            );
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    bytes.len() + chunk.len() <= 65536,
                    "semantic config exceeds bound"
                );
                bytes.extend_from_slice(&chunk);
            }
            current["semantic_config"] = serde_json::from_slice(&bytes)?;
        }
        if data["cluster_snapshot"].is_object() {
            // Bind saved leads and reviews without changing historical data.
            let clusters = bitcoin_clusters::fetch(app, &upstream, data).await?;
            current["cluster_snapshot"] = clusters["cluster_snapshot"].clone();
            current["cluster_config"] = clusters["cluster_config"].clone();
        }
        if data["risk_snapshot"].is_object() {
            let address = data["address"]
                .as_str()
                .filter(|a| valid_address("bitcoin", a))
                .context("invalid risk address")?;
            let id = data["risk_snapshot"]["assessment_id"]
                .as_str()
                .context("missing assessment identity")?;
            ensure!(
                id.strip_prefix("btc-risk-")
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
                "invalid assessment identity"
            );
            let mut response = app
                .http
                .get(format!(
                    "{upstream}/api/bitcoin/wallet/{address}/risk/{id}/snapshot"
                ))
                .header("x-aml-service-key", &app.key)
                .timeout(Duration::from_secs(5))
                .send()
                .await?;
            ensure!(response.status().is_success(), "risk freshness unavailable");
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    bytes.len() + chunk.len() <= 65536,
                    "risk snapshot exceeds bound"
                );
                bytes.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                value["network_id"] == data["network_id"],
                "risk network mismatch"
            );
            current["risk_snapshot"] = value["risk_snapshot"].clone();
        }
        Ok::<Value, anyhow::Error>(current)
    }
    .await
    .ok();
    annotate_source_validity(data, current.as_ref());
}
async fn export(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let owner = owner(&app, &headers)?;
    if headers
        .get(header::CONTENT_TYPE)
        .is_none_or(|v| v != "application/json")
    {
        return Err(Failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Export requires application/json",
        ));
    }
    let data = app
        .store
        .export(&id, &owner.hash, now())
        .await
        .map_err(internal)?
        .ok_or(Failure(
            StatusCode::NOT_FOUND,
            "Investigation not found or expired",
        ))?;
    let mut result = json!({"investigation":data});
    if result["investigation"]["network"] == "bitcoin" {
        let mut snapshot = app
            .store
            .read(&id, &owner.hash, now())
            .await
            .map_err(internal)?
            .ok_or(Failure(StatusCode::NOT_FOUND, "Investigation not found"))?;
        refresh_source_validity(&app, &mut snapshot, "bitcoin").await;
        result["source_validity"] = snapshot["source_validity"].clone();
        result["risk_engine"] = snapshot["risk_engine"].clone();
    }
    Ok(response(result, owner))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn reopen_and_export_keep_stale_evidence_but_suppress_current_risk() {
        let network = "bip122:000000000019d6689c085ae165831e93";
        let payload = serde_json::json!({"network_id":network,"source_snapshot":{"contract":"canonical_v1","snapshot_id":"a".repeat(64),"network_id":network,"source_revision":"10","block_hash":"b".repeat(64),"as_of_block":7},"risk_engine":{"risk_score":90,"risk_level":"HIGH"}});
        let metadata = serde_json::json!({"id":"fixture","network":"bitcoin","network_id":network,"state":"saved"});
        let query = axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let payload = payload.clone();
            let metadata = metadata.clone();
            async move {
                let rows = if body["statement"]
                    .as_str()
                    .unwrap()
                    .contains("RETURN i.payload_json")
                {
                    serde_json::json!([[payload.to_string(), metadata]])
                } else {
                    serde_json::json!([[metadata]])
                };
                axum::Json(serde_json::json!({"data":{"values":rows},"errors":[]}))
            }
        });
        let current = serde_json::json!({"network_id":network,"source_snapshot":{"contract":"canonical_v1","snapshot_id":"c".repeat(64),"network_id":network,"source_revision":"11","block_hash":"d".repeat(64),"as_of_block":7}});
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .route("/db/neo4j/query/v2", query)
            .route(
                "/status",
                axum::routing::get(move || async move { axum::Json(current) }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let http = reqwest::Client::new();
        let app = super::App {
            store: super::Store::new(http.clone(), &url, String::new()),
            http,
            upstreams: std::collections::HashMap::from([("bitcoin".into(), url)]),
            key: "fixture".into(),
            ttl_ms: 1000,
            requests: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-aml-service-key", "fixture".parse().unwrap());
        headers.insert("x-aml-user", "fixture-user".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        let response = super::reopen(
            axum::extract::State(app.clone()),
            headers.clone(),
            axum::extract::Path("fixture".into()),
        )
        .await
        .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["source_validity"]["status"], "stale");
        assert!(data["risk_engine"]["risk_score"].is_null());
        assert_eq!(data["historical_risk_engine"]["risk_score"], 90);
        let response = super::export(
            axum::extract::State(app),
            headers,
            axum::extract::Path("fixture".into()),
        )
        .await
        .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["source_validity"]["status"], "stale");
        assert!(data["risk_engine"]["risk_score"].is_null());
        server.abort();
    }
    #[test]
    fn changed_or_unverifiable_sources_cannot_supply_current_risk() {
        let network = "bip122:000000000019d6689c085ae165831e93";
        let mut data = serde_json::json!({"network_id":network,"source_snapshot":{"contract":"canonical_v1","snapshot_id":"a".repeat(64),"network_id":network,"source_revision":"10","block_hash":"b".repeat(64),"as_of_block":7},"risk_engine":{"risk_score":90,"risk_level":"HIGH","signals":["original"]}});
        let original = data.clone();
        let status =
            serde_json::json!({"network_id":network,"source_snapshot":data["source_snapshot"]});
        super::annotate_source_validity(&mut data, Some(&status));
        assert_eq!(data["source_validity"]["status"], "current");
        assert_eq!(data["risk_engine"]["risk_score"], 90);
        let mut changed = status.clone();
        changed["source_snapshot"]["source_revision"] = serde_json::json!("11");
        super::annotate_source_validity(&mut data, Some(&changed));
        assert_eq!(data["source_validity"]["status"], "stale");
        assert!(data["risk_engine"]["risk_score"].is_null());
        assert_eq!(data["historical_risk_engine"], original["risk_engine"]);
        super::annotate_source_validity(&mut data, None);
        assert_eq!(data["source_validity"]["status"], "unverified");
        assert_eq!(data["historical_risk_engine"], original["risk_engine"]);
        let mut wrong = original;
        let mut foreign = status;
        foreign["network_id"] = serde_json::json!("eip155:1");
        super::annotate_source_validity(&mut wrong, Some(&foreign));
        assert_eq!(wrong["source_validity"]["status"], "unverified");
    }
    #[tokio::test]
    async fn source_status_is_checked_over_http_and_failure_is_explicit() {
        let network = "bip122:000000000019d6689c085ae165831e93";
        let snapshot = serde_json::json!({"contract":"canonical_v1","snapshot_id":"a".repeat(64),"network_id":network,"source_revision":"10","block_hash":"b".repeat(64),"as_of_block":7});
        let result = serde_json::json!({"network_id":network,"source_snapshot":snapshot});
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new().route(
            "/status",
            axum::routing::get(move || async move { axum::Json(result) }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let http = reqwest::Client::new();
        let app = super::App {
            store: super::Store::new(http.clone(), &url, String::new()),
            http,
            upstreams: std::collections::HashMap::from([("bitcoin".into(), url)]),
            key: "fixture".into(),
            ttl_ms: 1000,
            requests: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let mut data = serde_json::json!({"network_id":network,"source_snapshot":snapshot});
        super::refresh_source_validity(&app, &mut data, "bitcoin").await;
        assert_eq!(data["source_validity"]["status"], "current");
        server.abort();
        let _ = server.await;
        super::refresh_source_validity(&app, &mut data, "bitcoin").await;
        assert_eq!(data["source_validity"]["status"], "unverified");
    }
    #[test]
    fn bitcoin_preserves_transaction_nodes_and_satoshi_evidence() {
        let data = serde_json::json!({"graph":{"nodes":[
            {"address":"1BoatSLRHtKNngkdXEeobR76b53LETtpyT","node_type":"address"},
            {"address":"tx:abc","node_type":"transaction"}],
            "edges":[{"id":"abc:in:0","from_address":"1BoatSLRHtKNngkdXEeobR76b53LETtpyT",
                "to_address":"tx:abc","amount":"2100000000000000","asset_id":"native:BTC","tx_hash":"abc","transfer_type":"utxo_input"}]}});
        let (nodes, edges) = super::normalize_graph(
            "snapshot",
            "bip122:000000000019d6689c085ae165831e93",
            "bitcoin",
            &data,
        )
        .unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges[0]["amount"], "2100000000000000");
        assert_eq!(edges[0]["to_address"], "tx:abc");
        assert!(super::valid_address(
            "bitcoin",
            "1BoatSLRHtKNngkdXEeobR76b53LETtpyT"
        ));
        assert!(!super::valid_address("bitcoin", "tb1qtest"));
    }

    #[tokio::test]
    #[ignore = "requires Bitcoin intelligence ClickHouse acceptance JSON"]
    async fn reviewed_bitcoin_clickhouse_evidence_reaches_policy_and_stale_snapshots_are_disabled()
    {
        let proof: serde_json::Value = serde_json::from_slice(
            &std::fs::read(std::env::var("BITCOIN_INTELLIGENCE_ACCEPTANCE").unwrap()).unwrap(),
        )
        .unwrap();
        for state in ["pending", "source_inactive", "revoked"] {
            let result = super::risk::assess("bitcoin", &proof[state]);
            assert_eq!(result["risk_level"], "UNKNOWN", "{state}");
            assert!(result["signals"].as_array().unwrap().is_empty());
        }
        let approved = &proof["approved"];
        let risk = super::risk::assess("bitcoin", approved);
        assert_eq!(risk["risk_score"], 70.0);
        assert_eq!(risk["signals"][0]["signal_type"], "APPROVED_RISK_LABEL");
        assert_eq!(
            risk["signals"][0]["evidence"]["source_id"],
            "fixture-source"
        );
        let mut invalid = approved.clone();
        invalid["intelligence_snapshot"] = serde_json::Value::Null;
        assert!(super::risk::assess("bitcoin", &invalid)["risk_score"].is_null());
        let live = std::sync::Arc::new(std::sync::RwLock::new(
            approved["intelligence_snapshot"].clone(),
        ));
        let read = live.clone();
        let chain = serde_json::json!({"network_id":approved["network_id"],"source_snapshot":approved["source_snapshot"],"native_asset_snapshot":approved["holdings"]["native_asset_snapshot"]});
        let network = approved["network_id"].clone();
        let semantic_config = json!({"contract":"bitcoin_semantics_config_v1","network_id":network,"detector_version":approved["semantic_snapshot"]["detector_version"],"definition_hash":approved["semantic_snapshot"]["definition_hash"]});
        let cluster_current = json!({"network_id":network,"cluster_snapshot":approved["cluster_snapshot"],"cluster_config":{"contract":"bitcoin_clustering_config_v1","network_id":network,"detector_version":approved["cluster_snapshot"]["detector_version"],"definition_hash":approved["cluster_snapshot"]["definition_hash"]}});
        let path = format!(
            "/api/bitcoin/wallet/{}/intelligence/snapshot",
            approved["address"].as_str().unwrap()
        );
        let router = axum::Router::new().route("/status", axum::routing::get(move || async move { axum::Json(chain) }))
            .route("/api/bitcoin/wallet/{address}/clusters/{run}/snapshot",axum::routing::get(move || {let value=cluster_current.clone();async move {axum::Json(value)}}))
            .route("/api/bitcoin/semantics/config",axum::routing::get(move || {let config=semantic_config.clone();async move {axum::Json(config)}}))
            .route(&path, axum::routing::get(move || { let snapshot = read.read().unwrap().clone(); let network=network.clone(); async move { axum::Json(serde_json::json!({"network_id":network,"intelligence_snapshot":snapshot})) } }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let http = reqwest::Client::new();
        let app = super::App {
            store: super::Store::new(http.clone(), &url, String::new()),
            http,
            upstreams: std::collections::HashMap::from([("bitcoin".into(), url)]),
            key: "fixture".into(),
            ttl_ms: 1000,
            requests: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let mut saved = approved.clone();
        saved["risk_engine"] = risk.clone();
        super::refresh_source_validity(&app, &mut saved, "bitcoin").await;
        assert_eq!(saved["source_validity"]["status"], "current");
        for state in ["source_inactive", "revoked"] {
            *live.write().unwrap() = proof[state]["intelligence_snapshot"].clone();
            let mut saved = approved.clone();
            saved["risk_engine"] = risk.clone();
            super::refresh_source_validity(&app, &mut saved, "bitcoin").await;
            assert_eq!(saved["source_validity"]["status"], "stale");
            assert!(saved["risk_engine"]["risk_score"].is_null());
            assert_eq!(saved["historical_risk_engine"]["risk_score"], 70.0);
        }
        *live.write().unwrap() = serde_json::Value::Null;
        super::refresh_source_validity(&app, &mut saved, "bitcoin").await;
        assert_eq!(saved["source_validity"]["status"], "unverified");
        if let Ok(path) = std::env::var("BITCOIN_INTELLIGENCE_CENTRAL_ACCEPTANCE") {
            let mut report = proof.clone();
            for state in ["pending", "approved", "source_inactive", "revoked"] {
                report[state]["risk_engine"] = super::risk::assess("bitcoin", &proof[state]);
            }
            std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        server.abort();
    }
    use super::*;
    #[test]
    fn network_and_investigation_scoping() {
        let d = json!({"graph":{"nodes":[{"id":"wallet"}],"edges":[]}});
        let (a, _) = normalize_graph("one", "tron:mainnet", "tron", &d).unwrap();
        let (b, _) = normalize_graph("two", "tron:mainnet", "tron", &d).unwrap();
        assert_ne!(a[0]["key"], b[0]["key"]);
    }
    #[test]
    fn invalid_edges_rejected() {
        assert!(
            normalize_graph(
                "a",
                "tron:mainnet",
                "tron",
                &json!({"nodes":[],"edges":[{"id":"1","from":"a","to":"b"}]})
            )
            .is_err()
        );
    }
    #[test]
    fn typed_bitcoin_subjects_remain_typed_in_central_storage() {
        let wallet = "1BoatSLRHtKNngkdXEeobR76b53LETtpyT";
        let tx = format!("tx:{}:{}", "a".repeat(64), "b".repeat(64));
        let script = format!("script:{}", "c".repeat(64));
        let node = |id: &str, kind: &str, address: Value| json!({"id":id,"subject_id":id,"subject_type":kind,"node_type":kind,"address":address});
        let edge = |id: &str, from: &str, to: &str, from_type: &str, to_type: &str, kind: &str| {
            json!({"id":id,"from_subject_id":from,"to_subject_id":to,"from_subject_type":from_type,"to_subject_type":to_type,
            "from_address":if from_type=="address"{Some(from)}else{None},"to_address":if to_type=="address"{Some(to)}else{None},"transfer_type":kind,"amount":"1","asset_id":"native:BTC","tx_hash":"b".repeat(64),"fund_continuity_claimed":false,"ownership_claimed":false,"amount_allocation_claimed":false})
        };
        let mut data = json!({"graph":{"nodes":[node(wallet,"address",json!(wallet)),node(&tx,"transaction",Value::Null),node(&script,"script",Value::Null)],
            "edges":[edge("in",wallet,&tx,"address","transaction","utxo_input"),edge("out",&tx,&script,"transaction","script","utxo_output")]}});
        let (nodes, edges) = normalize_graph(
            "snapshot",
            "bip122:000000000019d6689c085ae165831e93",
            "bitcoin",
            &data,
        )
        .unwrap();
        assert_eq!(nodes.iter().filter(|n| n["address"].is_null()).count(), 2);
        assert_eq!(edges[0]["amount"], "1");
        assert!(edges[0]["to_address"].is_null());
        assert_eq!(edges[1]["to_subject_id"], script);
        assert_eq!(edges[1]["to_subject_type"], "script");
        data["graph"]["edges"][0]["fund_continuity_claimed"] = json!(true);
        assert!(
            normalize_graph(
                "snapshot",
                "bip122:000000000019d6689c085ae165831e93",
                "bitcoin",
                &data
            )
            .is_err()
        );
        data["graph"]["edges"][0]["fund_continuity_claimed"] = json!(false);
        data["graph"]["edges"][0]["to_subject_id"] = json!(wallet);
        data["graph"]["edges"][0]["to_subject_type"] = json!("address");
        assert!(
            normalize_graph(
                "snapshot",
                "bip122:000000000019d6689c085ae165831e93",
                "bitcoin",
                &data
            )
            .is_err()
        );
    }
}
