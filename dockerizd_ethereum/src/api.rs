use std::{str::FromStr, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use tokio::time::timeout;

use crate::{
    api_security::{ServiceAuth, require_service_auth},
    config::AppConfig,
    db::database_client,
    domain::{AddressId, NetworkId},
    graph::EthereumGraph,
    investigation::{InvestigationService, PathDirection},
    risk::EvidenceRiskEngine,
};

#[derive(Clone)]
struct ApiState {
    config: AppConfig,
    investigation: Arc<InvestigationService>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

#[derive(Debug, Deserialize)]
struct InvestigationQuery {
    limit: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PathQuery {
    max_hops: Option<u8>,
    limit: Option<usize>,
    per_address_limit: Option<u64>,
    direction: Option<String>,
}

pub async fn build_router(config: AppConfig) -> anyhow::Result<Router> {
    let service_auth = ServiceAuth::from_env()?;
    let graph = EthereumGraph::connect(
        &config.neo4j_uri,
        &config.neo4j_username,
        &config.neo4j_password,
    )
    .await?;
    let investigation = InvestigationService::new(
        database_client(&config),
        graph,
        config.eth_network_id.clone(),
        config.eth_graph_max_edges,
        EvidenceRiskEngine::new(&config),
    );
    let state = ApiState {
        config,
        investigation: Arc::new(investigation),
    };

    let public_routes = Router::new()
        .route("/", get(dashboard))
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .with_state(state.clone());
    let protected_routes = Router::new()
        .route("/status", get(status))
        .route(
            "/ethereum/wallet/{address}/investigation",
            get(wallet_investigation),
        )
        .route(
            "/ethereum/wallet/{source}/paths/{target}",
            get(wallet_paths),
        )
        .route(
            "/ethereum/wallet/{address}/neo4j/import",
            post(wallet_investigation_import),
        )
        .route(
            "/ethereum/wallet/{source}/paths/{target}/neo4j/import",
            post(wallet_paths_import),
        )
        .route(
            "/api/ethereum/wallet/{address}/investigation",
            get(wallet_investigation),
        )
        .route(
            "/api/ethereum/wallet/{source}/paths/{target}",
            get(wallet_paths),
        )
        .route(
            "/api/ethereum/wallet/{address}/neo4j/import",
            post(wallet_investigation_import),
        )
        .route(
            "/api/ethereum/wallet/{source}/paths/{target}/neo4j/import",
            post(wallet_paths_import),
        )
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            service_auth,
            require_service_auth,
        ));

    Ok(public_routes.merge(protected_routes))
}

async fn dashboard() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "alive", "network": "ethereum"}))
}

async fn readiness(State(state): State<ApiState>) -> Response {
    let clickhouse = timeout(
        Duration::from_secs(4),
        state.investigation.probe_clickhouse(),
    );
    let neo4j = timeout(Duration::from_secs(4), state.investigation.probe_neo4j());
    let (clickhouse, neo4j) = tokio::join!(clickhouse, neo4j);
    let clickhouse_ready = matches!(clickhouse, Ok(Ok(())));
    let neo4j_ready = matches!(neo4j, Ok(Ok(())));
    let ready = clickhouse_ready && neo4j_ready;

    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({
            "status": if ready { "ready" } else { "not_ready" },
            "network_id": state.config.eth_network_id,
            "dependencies": {
                "clickhouse": if clickhouse_ready { "ready" } else { "unavailable" },
                "neo4j": if neo4j_ready { "ready" } else { "unavailable" }
            }
        })),
    )
        .into_response()
}

async fn status(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let status = state
        .investigation
        .status()
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(status))
}

async fn wallet_investigation(
    State(state): State<ApiState>,
    Path(address): Path<String>,
    Query(query): Query<InvestigationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let address = normalize_address(&state.config, &address)?;
    let investigation = state
        .investigation
        .investigate_wallet(&address, query.limit)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(investigation))
}

async fn wallet_investigation_import(
    State(state): State<ApiState>,
    Path(address): Path<String>,
    Query(query): Query<InvestigationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let address = normalize_address(&state.config, &address)?;
    let investigation = state
        .investigation
        .investigate_wallet_and_project(&address, query.limit)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(investigation))
}

async fn wallet_paths(
    State(state): State<ApiState>,
    Path((source, target)): Path<(String, String)>,
    Query(query): Query<PathQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let source = normalize_address(&state.config, &source)?;
    let target = normalize_address(&state.config, &target)?;
    let direction =
        PathDirection::parse(query.direction.as_deref()).map_err(ApiError::bad_request)?;
    let paths = state
        .investigation
        .find_paths(
            &source,
            &target,
            query.max_hops,
            query.limit,
            query.per_address_limit,
            direction,
        )
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(paths))
}

async fn wallet_paths_import(
    State(state): State<ApiState>,
    Path((source, target)): Path<(String, String)>,
    Query(query): Query<PathQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let source = normalize_address(&state.config, &source)?;
    let target = normalize_address(&state.config, &target)?;
    let direction =
        PathDirection::parse(query.direction.as_deref()).map_err(ApiError::bad_request)?;
    let paths = state
        .investigation
        .find_paths_and_project(
            &source,
            &target,
            query.max_hops,
            query.limit,
            query.per_address_limit,
            direction,
        )
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(paths))
}

fn normalize_address(config: &AppConfig, value: &str) -> Result<String, ApiError> {
    let network = NetworkId::from_str(&config.eth_network_id).map_err(ApiError::bad_request)?;
    AddressId::parse_evm(network, value)
        .map(|address| address.address().to_string())
        .map_err(ApiError::bad_request)
}

impl ApiError {
    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "Ethereum API request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal server error".to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
                "status": self.status.as_u16()
            })),
        )
            .into_response()
    }
}
