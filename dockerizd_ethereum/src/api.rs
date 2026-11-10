use aml_chain_api::{ApiPath as Path, ApiQuery as Query, EmptyQuery, bounded, bridge};
use std::{str::FromStr, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
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
    ethereum::holdings,
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

async fn kyt_transaction(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
) -> Result<Json<aml_chain_api::kyt::TransactionObservation>, aml_chain_api::ApiError> {
    let db = database_client(&state.config);
    aml_chain_api::kyt::transaction(&db, db.database(), aml_chain_api::Network::Ethereum, &hash)
        .await
        .map(Json)
}

async fn bridge_transaction(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
    Query(anchor): Query<bridge::Anchor>,
) -> Result<Json<bridge::Observation>, aml_chain_api::ApiError> {
    let db = database_client(&state.config);
    bridge::lookup(
        &db,
        db.database(),
        aml_chain_api::Network::Ethereum,
        &hash,
        bridge::Lookup::Transaction(anchor),
    )
    .await
    .map(Json)
}

async fn bridge_operation(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
    Query(window): Query<bridge::Window>,
) -> Result<Json<bridge::Observation>, aml_chain_api::ApiError> {
    let db = database_client(&state.config);
    bridge::lookup(
        &db,
        db.database(),
        aml_chain_api::Network::Ethereum,
        &hash,
        bridge::Lookup::Operation(window),
    )
    .await
    .map(Json)
}

async fn bridge_snapshot(
    State(state): State<ApiState>,
    Query(_): Query<EmptyQuery>,
) -> Result<Json<serde_json::Value>, aml_chain_api::ApiError> {
    let db = database_client(&state.config);
    bridge::snapshot(&db, db.database(), aml_chain_api::Network::Ethereum)
        .await
        .map(Json)
}

async fn tbtc_reveals(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
    Query(window): Query<bridge::tbtc::RevealWindow>,
) -> Result<Json<bridge::Observation>, aml_chain_api::ApiError> {
    let db = database_client(&state.config);
    bridge::lookup(
        &db,
        db.database(),
        aml_chain_api::Network::Ethereum,
        &hash,
        bridge::Lookup::TbtcReveal(window),
    )
    .await
    .map(Json)
}

async fn tbtc_sweeps(
    State(state): State<ApiState>,
    Path(hash): Path<String>,
    Query(window): Query<bridge::tbtc::SweepWindow>,
) -> Result<Json<bridge::Observation>, aml_chain_api::ApiError> {
    let db = database_client(&state.config);
    bridge::lookup(
        &db,
        db.database(),
        aml_chain_api::Network::Ethereum,
        &hash,
        bridge::Lookup::TbtcSweep(window),
    )
    .await
    .map(Json)
}

async fn tbtc_mints(
    State(state): State<ApiState>,
    Path(key): Path<String>,
    Query(window): Query<bridge::tbtc::MintWindow>,
) -> Result<Json<bridge::tbtc::MintObservation>, aml_chain_api::ApiError> {
    let db = database_client(&state.config);
    bridge::tbtc::mints(&db, db.database(), &key, window)
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InvestigationQuery {
    limit: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathQuery {
    max_hops: Option<u8>,
    limit: Option<usize>,
    per_address_limit: Option<u64>,
    direction: Option<String>,
}

pub async fn build_router(config: AppConfig) -> anyhow::Result<Router> {
    let service_auth = ServiceAuth::from_env()?;
    Ok(router(config, service_auth))
}

fn router(config: AppConfig, service_auth: ServiceAuth) -> Router {
    let investigation = InvestigationService::new(
        database_client(&config),
        None,
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
        .route(aml_chain_api::kyt::TRANSACTION_ROUTE, get(kyt_transaction))
        .route(bridge::TRANSACTION_ROUTE, get(bridge_transaction))
        .route(bridge::OPERATION_ROUTE, get(bridge_operation))
        .route(bridge::SNAPSHOT_ROUTE, get(bridge_snapshot))
        .route(bridge::tbtc::REVEALS_ROUTE, get(tbtc_reveals))
        .route(bridge::tbtc::SWEEPS_ROUTE, get(tbtc_sweeps))
        .route(bridge::tbtc::MINTS_ROUTE, get(tbtc_mints))
        .route(
            "/api/ethereum/wallet/{address}/transfers",
            get(wallet_transfers),
        )
        .route(
            "/api/ethereum/wallet/{address}/holdings",
            get(wallet_holdings),
        )
        .route("/ethereum/wallet/{address}/holdings", get(wallet_holdings))
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
            post(central_projection_only),
        )
        .route(
            "/ethereum/wallet/{source}/paths/{target}/neo4j/import",
            post(central_projection_only),
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
            post(central_projection_only),
        )
        .route(
            "/api/ethereum/wallet/{source}/paths/{target}/neo4j/import",
            post(central_projection_only),
        )
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            service_auth,
            require_service_auth,
        ))
        .layer(middleware::from_fn(aml_chain_api::no_store));

    public_routes.merge(protected_routes)
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
    let clickhouse_ready = matches!(clickhouse.await, Ok(Ok(())));
    let ready = clickhouse_ready;

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
                "graph_storage": "central_analytical_node"
            }
        })),
    )
        .into_response()
}

async fn status(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let db = database_client(&state.config);
    let operations = aml_warehouse::analytics::operational_snapshot(
        &db,
        db.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await
    .map_err(ApiError::internal)?;
    let status = state
        .investigation
        .status()
        .await
        .map_err(ApiError::internal)?;
    let after = aml_warehouse::analytics::operational_snapshot(
        &db,
        db.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await
    .map_err(ApiError::internal)?;
    if after != operations {
        return Err(ApiError::internal("operational evidence changed; retry"));
    }
    let mut value = serde_json::to_value(status).map_err(ApiError::internal)?;
    value["operational_snapshot"] = operations;
    Ok(Json(value))
}

async fn wallet_investigation(
    State(state): State<ApiState>,
    Path(address): Path<String>,
    Query(query): Query<InvestigationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    bounded(
        "limit",
        query.limit,
        1,
        crate::investigation::MAX_EDGE_LIMIT,
    )
    .map_err(ApiError::bad_request)?;
    let address = normalize_address(&state.config, &address)?;
    let db = database_client(&state.config);
    let operations = aml_warehouse::analytics::operational_snapshot(
        &db,
        db.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await
    .map_err(ApiError::internal)?;
    let (investigation, holdings) = tokio::join!(
        state
            .investigation
            .investigate_wallet(&address, query.limit),
        holdings::snapshot(&state.config, &db, &address)
    );
    let mut data = serde_json::to_value(investigation.map_err(ApiError::internal)?)
        .map_err(ApiError::internal)?;
    data["holdings"] = holdings;
    data["activity"]=serde_json::to_value(db.query("SELECT intDiv(block_timestamp_unix_ms,86400000)*86400000 AS day_unix_ms,countIf(to_address=?) AS received,countIf(from_address=?) AS sent,uniqExact(tx_hash) AS transactions FROM aml_address_relationships_canonical WHERE network_id='eip155:1' AND (from_address=? OR to_address=?) GROUP BY day_unix_ms ORDER BY day_unix_ms DESC LIMIT 366")
        .bind(&address).bind(&address).bind(&address).bind(&address).fetch_all::<ActivityBucket>().await.map_err(ApiError::internal)?).map_err(ApiError::internal)?;
    data["operational_snapshot"] = operations.clone();
    data["query_scope"] = json!({"graph_requested_limit":query.limit,"activity":"all_indexed_transfer_days","activity_day_limit":366,"holdings":"current_finalized_not_historical_window"});
    let mut data = aml_chain_api::response::normalize(
        aml_warehouse::Network::Ethereum,
        aml_chain_api::response::Mode::Wallet,
        data,
    )
    .map_err(ApiError::internal)?;
    data["nft_marketplace_evidence"] =
        aml_chain_api::marketplace::observe(&db, db.database(), &data).await;
    let source =
        aml_warehouse::visibility::capture(&db, db.database(), aml_warehouse::Network::Ethereum)
            .await
            .map_err(ApiError::internal)?;
    let after = aml_warehouse::analytics::operational_snapshot(
        &db,
        db.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await
    .map_err(ApiError::internal)?;
    if after != operations || source.value() != data["source_snapshot"] {
        return Err(ApiError::internal(
            "source evidence changed; retry investigation",
        ));
    }
    Ok(Json(data))
}

#[derive(serde::Serialize, serde::Deserialize, clickhouse::Row)]
struct ActivityBucket {
    day_unix_ms: u64,
    received: u64,
    sent: u64,
    transactions: u64,
}

async fn wallet_transfers(
    State(state): State<ApiState>,
    Path(raw): Path<String>,
    Query(query): Query<aml_warehouse::relationships::PageQuery>,
) -> Result<Json<serde_json::Value>, aml_chain_api::ApiError> {
    let address = normalize_address(&state.config, &raw)
        .map_err(|e| aml_chain_api::ApiError::new(e.status, e.message))?;
    let subject =
        aml_warehouse::relationships::Subject::address(aml_warehouse::Network::Ethereum, &address)
            .map_err(|e| aml_chain_api::ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let db = database_client(&state.config);
    aml_warehouse::relationships::read_page(
        &db,
        db.database(),
        aml_warehouse::Network::Ethereum,
        &subject,
        &query,
    )
    .await
    .map(Json)
    .map_err(aml_chain_api::transfer_error)
}

async fn wallet_holdings(
    State(state): State<ApiState>,
    Path(address): Path<String>,
    Query(_): Query<EmptyQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let address = normalize_address(&state.config, &address)?;
    let data = holdings::snapshot(&state.config, &database_client(&state.config), &address).await;
    let status = if data["status"] == "unavailable" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    Ok((status, Json(data)))
}

async fn central_projection_only() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(
            json!({"error":"Use the main VM investigation Export endpoint; chain APIs do not persist graphs."}),
        ),
    )
}

async fn wallet_paths(
    State(state): State<ApiState>,
    Path((source, target)): Path<(String, String)>,
    Query(query): Query<PathQuery>,
) -> Result<impl IntoResponse, ApiError> {
    bounded("max_hops", query.max_hops, 1, 10).map_err(ApiError::bad_request)?;
    bounded(
        "limit",
        query.limit,
        1,
        crate::investigation::MAX_PATH_LIMIT,
    )
    .map_err(ApiError::bad_request)?;
    bounded(
        "per_address_limit",
        query.per_address_limit,
        1,
        crate::investigation::MAX_PER_ADDRESS_LIMIT,
    )
    .map_err(ApiError::bad_request)?;
    let source = normalize_address(&state.config, &source)?;
    let target = normalize_address(&state.config, &target)?;
    if source == target {
        return Err(ApiError::bad_request("distinct addresses required"));
    }
    let direction =
        PathDirection::parse(query.direction.as_deref()).map_err(ApiError::bad_request)?;
    let db = database_client(&state.config);
    let operations = aml_warehouse::analytics::operational_snapshot(
        &db,
        db.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await
    .map_err(ApiError::internal)?;
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
    let after = aml_warehouse::analytics::operational_snapshot(
        &db,
        db.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await
    .map_err(ApiError::internal)?;
    if after != operations {
        return Err(ApiError::internal(
            "operational evidence changed; retry path search",
        ));
    }
    let mut value = serde_json::to_value(paths).map_err(ApiError::internal)?;
    value["operational_snapshot"] = operations;
    Ok(Json(
        aml_chain_api::response::normalize(
            aml_warehouse::Network::Ethereum,
            aml_chain_api::response::Mode::Paths,
            value,
        )
        .map_err(ApiError::internal)?,
    ))
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
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "Ethereum evidence unavailable; check service logs".to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        aml_chain_api::error_response(self.status, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn api_boundary_contract() -> anyhow::Result<()> {
        let mut config = AppConfig::from_env()?;
        config.clickhouse_url = "http://127.0.0.1:9".into();
        config.eth_rpc_url = "http://127.0.0.1:9".into();
        let key = "api-contract-fixture-key-0123456789";
        let app = router(config, ServiceAuth::required(key)?);
        let wallet = "/api/ethereum/wallet/0x1111111111111111111111111111111111111111";
        let paths = "paths/0x2222222222222222222222222222222222222222";
        for (suffix, code) in [
            ("investigation?unknown=1", "invalid_query"),
            ("investigation?limit=no", "invalid_query"),
            ("investigation?limit=-1", "invalid_query"),
            ("investigation?limit=1&limit=2", "invalid_query"),
            ("investigation?limit=0", "invalid_request"),
            ("investigation?limit=5001", "invalid_request"),
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
            (format!("{paths}?limit=26"), "invalid_request"),
            (format!("{paths}?per_address_limit=2001"), "invalid_request"),
            (format!("{paths}?direction=typo"), "invalid_request"),
        ]) {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("{wallet}/{suffix}"))
                        .header("X-AML-Service-Key", key)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{suffix}");
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
            assert_eq!(body["api_version"], "1.0");
            assert_eq!(body["status"], 400);
            assert_eq!(body["code"], code, "{suffix}");
            assert!(body["error"].is_string());
        }
        for (path, key, status, code) in [
            (
                "/api/ethereum/wallet/invalid/investigation?limit=no",
                "",
                401,
                "unauthorized",
            ),
            ("/status", "wrong", 401, "unauthorized"),
            (
                "/api/ethereum/wallet/%FF/investigation",
                key,
                400,
                "invalid_path",
            ),
            ("/status", key, 503, "evidence_unavailable"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("X-AML-Service-Key", key)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
            assert_eq!(body["code"], code);
            assert_eq!(body["api_version"], "1.0");
            assert_eq!(body["status"], status);
        }
        Ok(())
    }
}
