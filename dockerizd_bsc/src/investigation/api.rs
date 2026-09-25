use super::{PathOptions, SearchOptions, investigate, paths, status};
use crate::{
    config::{ClickHouseConfig, setting},
    db::{validate_bsc_schema, warehouse::Warehouse},
    domain::normalize_evm_address,
    intelligence, metadata,
};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
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
    key: Arc<String>,
    slots: Arc<Semaphore>,
}
impl ApiState {
    pub fn new(db: Warehouse, key: String) -> Result<Self> {
        ensure!(
            key.len() >= 32,
            "AML_SERVICE_KEY must have at least 32 characters"
        );
        Ok(Self {
            db,
            key: Arc::new(key),
            slots: Arc::new(Semaphore::new(4)),
        })
    }
}
pub fn router(state: ApiState) -> Router {
    let protected = Router::new()
        .route("/status", get(platform_status))
        .route("/api/bsc/wallet/{address}/investigation", get(wallet))
        .route(
            "/api/bsc/wallet/{address}/paths/{target}",
            get(wallet_paths),
        )
        .route("/api/bsc/wallet/{address}/holdings", get(holdings))
        .route(
            "/api/bsc/wallet/{address}/cluster-candidates",
            get(candidates),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));
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
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )
            .into_response();
    }
    let Ok(_permit) = state.slots.clone().try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":"BSC query capacity reached"})),
        )
            .into_response();
    };
    match tokio::time::timeout(Duration::from_secs(120), next.run(request)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({"error":"BSC investigation exceeded time limit"})),
        )
            .into_response(),
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
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
        metadata::snapshot(&state.db, &address)
    );
    let mut data = data?;
    data["holdings"] = holdings;
    Ok(Json(data))
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
    let result = paths(&state.db, &source, &target, &options, height).await?;
    super::wallet::check_epoch(&state.db, &coverage).await?;
    Ok(Json(result))
}
async fn holdings(State(state): State<ApiState>, Path(raw): Path<String>) -> ApiResult {
    let address = address(&raw)?;
    Ok(Json(metadata::snapshot(&state.db, &address).await))
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
    )?;
    tracing::info!(%addr,"BSC API listening; graph snapshots belong to main VM");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}
