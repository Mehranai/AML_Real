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
        data["risk_engine"] = risk::assess(&network, &data);
    }
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
    if matches!(network, "ethereum" | "bsc") {
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
        let address = if network == "tron" {
            risk::text(&node["id"])
        } else {
            risk::text(&node["address"])
        };
        ensure!(!address.is_empty(), "empty graph node");
        nodes.insert(address.to_string(),json!({"key":key(address),"address":address,
            "subject_key":format!("{network_id}:{address}"),"label":node.get("label").unwrap_or(&json!(address)),
            "evidence_json":node.to_string()}));
    }
    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for edge in raw_edges {
        let from = risk::text(
            &edge[if network == "tron" {
                "from"
            } else {
                "from_address"
            }],
        );
        let to = risk::text(
            &edge[if network == "tron" {
                "to"
            } else {
                "to_address"
            }],
        );
        ensure!(
            nodes.contains_key(from) && nodes.contains_key(to),
            "edge endpoint missing"
        );
        let edge_id = risk::text(&edge["id"]);
        ensure!(
            !edge_id.is_empty() && seen.insert(edge_id),
            "missing or duplicate edge ID"
        );
        // Preserve raw amounts as strings; never coerce token quantities into floats.
        edges.push(json!({"edge_id":edge_id,"from_key":key(from),"to_key":key(to),
            "from_address":from,"to_address":to,"tx_hash":edge["tx_hash"],"amount":edge["amount"],
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
    let data = app
        .store
        .read(&id, &owner.hash, now())
        .await
        .map_err(internal)?
        .ok_or(Failure(
            StatusCode::NOT_FOUND,
            "Investigation not found or expired",
        ))?;
    Ok(response(data, owner))
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
    Ok(response(json!({"investigation":data}), owner))
}

#[cfg(test)]
mod tests {
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
}
