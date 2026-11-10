use super::{PathOptions, SearchOptions, investigate, paths, status};
use crate::{
    config::{AppConfig, ClickHouseConfig, setting},
    db::{validate_bsc_schema, warehouse::Warehouse},
    domain::normalize_evm_address,
    intelligence, metadata,
};
use aml_chain_api::{ApiPath as Path, ApiQuery as Query, EmptyQuery, bridge, error_response};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct ApiState {
    pub db: Warehouse,
    rpc: metadata::TokenRpc,
    key: Arc<String>,
    slots: Arc<Semaphore>,
}
impl ApiState {
    pub fn new(db: Warehouse, key: String, rpc: metadata::TokenRpc) -> Result<Self> {
        ensure!(
            key.len() >= 32,
            "AML_SERVICE_KEY must have at least 32 characters"
        );
        Ok(Self {
            rpc,
            db,
            key: Arc::new(key),
            slots: Arc::new(Semaphore::new(4)),
        })
    }
}
pub fn router(state: ApiState) -> Router {
    let protected = Router::new()
        .route("/status", get(platform_status))
        .route(aml_chain_api::kyt::TRANSACTION_ROUTE, get(kyt_transaction))
        .route(bridge::TRANSACTION_ROUTE, get(bridge_transaction))
        .route(bridge::OPERATION_ROUTE, get(bridge_operation))
        .route(bridge::ALLBRIDGE_OPERATION_ROUTE, get(allbridge_operation))
        .route(bridge::SNAPSHOT_ROUTE, get(bridge_snapshot))
        .route("/api/bsc/wallet/{address}/investigation", get(wallet))
        .route("/api/bsc/wallet/{address}/transfers", get(transfers))
        .route(
            "/api/bsc/wallet/{address}/paths/{target}",
            get(wallet_paths),
        )
        .route("/api/bsc/wallet/{address}/holdings", get(holdings))
        .route(
            "/api/bsc/wallet/{address}/cluster-candidates",
            get(candidates),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(middleware::from_fn(aml_chain_api::no_store));
    Router::new()
        .merge(protected)
        .route(
            "/",
            get(|| async { Html(include_str!("../../web/index.html")) }),
        )
        .route(
            "/health",
            get(|| async {
                Json(json!({"status":"alive","network_id":"eip155:56","service":"bsc-api"}))
            }),
        )
        .route("/ready", get(ready))
        .with_state(state)
}
async fn kyt_transaction(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
) -> Result<Json<aml_chain_api::kyt::TransactionObservation>, aml_chain_api::ApiError> {
    state.db.kyt_transaction(&hash).await.map(Json)
}
async fn bridge_transaction(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
    Query(anchor): Query<bridge::Anchor>,
) -> std::result::Result<Json<bridge::Observation>, aml_chain_api::ApiError> {
    state
        .db
        .bridge_lookup(&hash, bridge::Lookup::Transaction(anchor))
        .await
        .map(Json)
}
async fn bridge_operation(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
    Query(window): Query<bridge::Window>,
) -> std::result::Result<Json<bridge::Observation>, aml_chain_api::ApiError> {
    state
        .db
        .bridge_lookup(&hash, bridge::Lookup::Operation(window))
        .await
        .map(Json)
}
async fn bridge_snapshot(
    State(state): State<ApiState>,
    Query(_): Query<EmptyQuery>,
) -> std::result::Result<Json<Value>, aml_chain_api::ApiError> {
    state.db.bridge_snapshot().await.map(Json)
}
async fn allbridge_operation(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
    Query(window): Query<bridge::Window>,
) -> std::result::Result<Json<bridge::Observation>, aml_chain_api::ApiError> {
    state
        .db
        .bridge_lookup(&hash, bridge::Lookup::AllbridgeOperation(window))
        .await
        .map(Json)
}
async fn authorize(
    State(state): State<ApiState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let incoming = headers
        .get("X-AML-Service-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let a = Sha256::digest(incoming.as_bytes());
    let b = Sha256::digest(state.key.as_bytes());
    if a.iter().zip(b).fold(0u8, |v, (a, b)| v | (a ^ b)) != 0 {
        return error_response(StatusCode::UNAUTHORIZED, "unauthorized service request");
    }
    let Ok(_permit) = state.slots.clone().try_acquire_owned() else {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "BSC query capacity reached");
    };
    match tokio::time::timeout(Duration::from_secs(120), next.run(request)).await {
        Ok(response) => response,
        Err(_) => error_response(
            StatusCode::GATEWAY_TIMEOUT,
            "BSC investigation exceeded time limit",
        ),
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error_response(self.0, self.1)
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(error=%error,"BSC query failed");
        Self(
            StatusCode::SERVICE_UNAVAILABLE,
            "BSC evidence unavailable; check service logs".into(),
        )
    }
}
fn address(raw: &str) -> std::result::Result<String, ApiError> {
    normalize_evm_address(raw)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid BSC address".into()))
}
fn bad(error: anyhow::Error) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, error.to_string())
}
async fn ready(State(state): State<ApiState>) -> Response {
    match state.db.rows("SELECT 1 AS ok", &[]).await {
        Ok(_)=>Json(json!({"status":"ready","network_id":"eip155:56","dependencies":{"clickhouse":"ready"},"graph_storage":"main_vm"})).into_response(),
        Err(error)=>ApiError::from(error).into_response()
    }
}
async fn platform_status(State(state): State<ApiState>) -> ApiResult {
    Ok(Json(status(&state.db).await?))
}
async fn wallet(
    State(state): State<ApiState>,
    Path(raw): Path<String>,
    Query(options): Query<SearchOptions>,
) -> ApiResult {
    let address = address(&raw)?;
    options.validate().map_err(bad)?;
    let (data, holdings) = tokio::join!(
        investigate(&state.db, &address, &options),
        metadata::snapshot(&state.db, &state.rpc, &address)
    );
    let mut data = data?;
    data["holdings"] = holdings;
    data["operational_snapshot"] = data["data_coverage"]["operational_snapshot"].clone();
    super::wallet::check_epoch(&state.db, &data["data_coverage"]).await?;
    Ok(Json(aml_chain_api::response::normalize(
        aml_warehouse::Network::Bsc,
        aml_chain_api::response::Mode::Wallet,
        data,
    )?))
}

async fn transfers(
    State(state): State<ApiState>,
    Path(raw): Path<String>,
    Query(query): Query<aml_warehouse::relationships::PageQuery>,
) -> Result<Json<Value>, aml_chain_api::ApiError> {
    let address = address(&raw).map_err(|e| aml_chain_api::ApiError::new(e.0, e.1))?;
    let subject =
        aml_warehouse::relationships::Subject::address(aml_warehouse::Network::Bsc, &address)
            .map_err(|e| aml_chain_api::ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .db
        .transfer_page(&subject, &query)
        .await
        .map(Json)
        .map_err(aml_chain_api::transfer_error)
}
async fn wallet_paths(
    State(state): State<ApiState>,
    Path((source, target)): Path<(String, String)>,
    Query(options): Query<PathOptions>,
) -> ApiResult {
    let source = address(&source)?;
    let target = address(&target)?;
    options.validate().map_err(bad)?;
    if source == target || source == super::ZERO_ADDRESS || target == super::ZERO_ADDRESS {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "distinct nonzero addresses required".into(),
        ));
    }
    let coverage = status(&state.db).await?;
    let height = coverage["last_synced_block"].as_u64().unwrap_or(0);
    let mut result = paths(&state.db, &source, &target, &options, height).await?;
    super::wallet::check_epoch(&state.db, &coverage).await?;
    result["source_snapshot"] = coverage["source_snapshot"].clone();
    result["operational_snapshot"] = coverage["operational_snapshot"].clone();
    Ok(Json(aml_chain_api::response::normalize(
        aml_warehouse::Network::Bsc,
        aml_chain_api::response::Mode::Paths,
        result,
    )?))
}
async fn holdings(
    State(state): State<ApiState>,
    Path(raw): Path<String>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response, ApiError> {
    let address = address(&raw)?;
    let value = metadata::snapshot(&state.db, &state.rpc, &address).await;
    let status = if value["status"] == "unavailable" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    Ok((status, Json(value)).into_response())
}
async fn candidates(State(state): State<ApiState>, Path(raw): Path<String>) -> ApiResult {
    let address = address(&raw)?;
    Ok(Json(
        json!({"address":address,"network_id":"eip155:56","candidates":intelligence::candidates(&state.db,&address).await?,
        "status":"unreviewed_leads","ownership_claimed":false}),
    ))
}
pub async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_ = tokio::signal::ctrl_c()=>{},_ = signal.recv()=>{}}
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
pub async fn run() -> Result<()> {
    let addr = setting("BSC_API_ADDR")?
        .unwrap_or_else(|| "127.0.0.1:6001".into())
        .parse::<std::net::SocketAddr>()
        .context("invalid BSC_API_ADDR")?;
    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .build()?
            .get(format!("http://127.0.0.1:{}/ready", addr.port()))
            .send()
            .await?
            .error_for_status()?;
        return Ok(());
    }
    let config = ClickHouseConfig::from_env()?;
    validate_bsc_schema(&config).await?;
    let state = ApiState::new(
        Warehouse::new(config)?,
        setting("AML_SERVICE_KEY")?.context("AML_SERVICE_KEY is required")?,
        metadata::TokenRpc::new(AppConfig::from_env()?)?,
    )?;
    tracing::info!(%addr,"BSC API listening; graph snapshots belong to main VM");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn api_boundary_contract() -> anyhow::Result<()> {
        use super::*;
        use crate::config::{DeploymentMode, RpcEndpoint, TraceMode};
        let db = Warehouse::new(ClickHouseConfig::fixture(
            "http://127.0.0.1:9",
            "fixture",
            "fixture-only",
            "bsc_aml",
        ))?;
        let config = AppConfig {
            mode: DeploymentMode::Development,
            rpc_endpoint: RpcEndpoint::parse("http://127.0.0.1:9")?,
            fallback_rpc_endpoints: vec![],
            rpc_provider: "fixture".into(),
            rpc_timeout: Duration::from_secs(1),
            trace_mode: TraceMode::Disabled,
            require_block_receipts: false,
            trace_probe_block: 0,
        };
        let key = "api-contract-fixture-key-0123456789";
        let app = router(ApiState::new(
            db,
            key.into(),
            metadata::TokenRpc::new(config)?,
        )?);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let wallet = "/api/bsc/wallet/0x1111111111111111111111111111111111111111";
        let paths = "paths/0x2222222222222222222222222222222222222222";
        for (suffix, code) in [
            ("investigation?unknown=1", "invalid_query"),
            ("investigation?limit=no", "invalid_query"),
            ("investigation?limit=-1", "invalid_query"),
            ("investigation?limit=1&limit=2", "invalid_query"),
            ("investigation?limit=0", "invalid_request"),
            ("investigation?limit=5001", "invalid_request"),
            ("investigation?from_ms=2&to_ms=1", "invalid_request"),
            (
                "investigation?asset_id=eip155:1/native:eth",
                "invalid_request",
            ),
            ("holdings?limit=10", "invalid_query"),
            ("transfers?unknown=1", "invalid_query"),
            ("transfers?limit=1&limit=2", "invalid_query"),
            ("transfers?direction=typo", "invalid_query"),
            ("transfers?limit=0", "invalid_request"),
            ("transfers?limit=101", "invalid_request"),
            ("transfers?from_block=2&to_block=1", "invalid_request"),
            ("transfers?from_unix_ms=2&to_unix_ms=1", "invalid_request"),
            ("transfers?asset_id=native:BTC", "invalid_request"),
            ("transfers?cursor=00", "invalid_request"),
        ]
        .into_iter()
        .map(|(s, c)| (s.to_string(), c))
        .chain([
            (format!("{paths}?unknown=1"), "invalid_query"),
            (format!("{paths}?max_hops=1&max_hops=2"), "invalid_query"),
            (format!("{paths}?max_hops=0"), "invalid_request"),
            (format!("{paths}?max_hops=11"), "invalid_request"),
            (format!("{paths}?limit=101"), "invalid_request"),
            (format!("{paths}?limit=1&max_paths=2"), "invalid_query"),
            (format!("{paths}?direction=typo"), "invalid_query"),
        ]) {
            let response = client
                .get(format!("{base}{wallet}/{suffix}"))
                .header("X-AML-Service-Key", key)
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{suffix}");
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: Value = response.json().await?;
            assert_eq!(body["api_version"], "1.0");
            assert_eq!(body["code"], code, "{suffix}");
            assert_eq!(body["status"], 400);
            assert!(body["error"].is_string());
        }
        for (path, key, status, code) in [
            (
                "/api/bsc/wallet/invalid/investigation?limit=no",
                "",
                401,
                "unauthorized",
            ),
            ("/status", "wrong", 401, "unauthorized"),
            (
                "/api/bsc/wallet/%FF/investigation",
                key,
                400,
                "invalid_path",
            ),
            ("/status", key, 503, "evidence_unavailable"),
        ] {
            let response = client
                .get(format!("{base}{path}"))
                .header("X-AML-Service-Key", key)
                .send()
                .await?;
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: Value = response.json().await?;
            assert_eq!(body["code"], code);
            assert_eq!(body["status"], status);
            assert_eq!(body["api_version"], "1.0");
        }
        server.abort();
        Ok(())
    }
    #[tokio::test]
    async fn wrong_chain_holdings_are_unavailable_and_never_zero() -> anyhow::Result<()> {
        use super::*;
        use crate::config::{AppConfig, DeploymentMode, RpcEndpoint, TraceMode};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let rpc_url = format!("http://{}", listener.local_addr()?);
        let rpc = Router::new().route(
            "/",
            axum::routing::post(|Json(request): Json<Value>| async move {
                Json(json!({"jsonrpc":"2.0","id":request["id"],"result":"0x1"}))
            }),
        );
        let mock = tokio::spawn(async move { axum::serve(listener, rpc).await.unwrap() });
        let config = AppConfig {
            mode: DeploymentMode::Development,
            rpc_endpoint: RpcEndpoint::parse(&rpc_url)?,
            fallback_rpc_endpoints: vec![],
            rpc_provider: "wrong-chain-fixture".into(),
            rpc_timeout: Duration::from_secs(2),
            trace_mode: TraceMode::Disabled,
            require_block_receipts: false,
            trace_probe_block: 0,
        };
        let db = Warehouse::new(ClickHouseConfig::fixture(
            "http://127.0.0.1:9",
            "fixture",
            "fixture-only",
            "bsc_aml",
        ))?;
        let key = "bsc-api-fixture-service-key-0123456789";
        let app = router(ApiState::new(
            db,
            key.into(),
            metadata::TokenRpc::new(config)?,
        )?);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!(
            "http://{}/api/bsc/wallet/0x1111111111111111111111111111111111111111/holdings",
            listener.local_addr()?
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let response = reqwest::Client::new()
            .get(url)
            .header("X-AML-Service-Key", key)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let value: Value = response.json().await?;
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["assets"], json!([]));
        assert_eq!(value["unavailable_is_not_zero"], true);
        server.abort();
        mock.abort();
        Ok(())
    }
}
