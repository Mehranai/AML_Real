use crate::{
    api_security::{ServiceAuth, require_service_auth},
    handlers::{
        dashboard, health, status, tron_graph, tron_ingestion_health, tron_wallet_ai_risk,
        tron_wallet_analysis, tron_wallet_fingerprint, tron_wallet_holdings,
        tron_wallet_investigation, tron_wallet_paths,
    },
};
use axum::{
    Router, middleware,
    routing::{get, post},
};

pub fn build_router() -> anyhow::Result<Router> {
    let service_auth = ServiceAuth::from_env()?;
    let public_routes = Router::new()
        .route("/", get(dashboard::dashboard))
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check));
    let protected_routes = Router::new()
        .route("/status", get(status::status))
        .route(
            "/tron/ingestion/health",
            get(tron_ingestion_health::tron_ingestion_health),
        )
        .route(
            "/tron/wallet/{address}/graph",
            get(tron_graph::tron_wallet_graph),
        )
        .route(
            "/tron/wallet/{address}/fingerprint",
            get(tron_wallet_fingerprint::tron_wallet_fingerprint),
        )
        .route(
            "/tron/wallet/{address}/ai-risk",
            get(tron_wallet_ai_risk::tron_wallet_ai_risk),
        )
        .route(
            "/tron/wallet/{address}/holdings",
            get(tron_wallet_holdings::tron_wallet_holdings),
        )
        .route(
            "/tron/wallet/{address}/investigation",
            get(tron_wallet_investigation::tron_wallet_investigation),
        )
        .route(
            "/analysis/tron/wallet/{address}",
            get(tron_wallet_analysis::tron_wallet_analysis_snapshot),
        )
        .route(
            "/tron/wallet/{address}/neo4j/import",
            post(health::central_projection_only),
        )
        .route(
            "/tron/wallet/{source}/paths/{target}",
            get(tron_wallet_paths::tron_wallet_paths),
        )
        .route(
            "/tron/wallet/{source}/paths/{target}/neo4j/import",
            post(health::central_projection_only),
        )
        .route(
            "/api/tron/wallet/{address}/graph",
            get(tron_graph::tron_wallet_graph),
        )
        .route(
            "/api/tron/ingestion/health",
            get(tron_ingestion_health::tron_ingestion_health),
        )
        .route(
            "/api/tron/wallet/{address}/fingerprint",
            get(tron_wallet_fingerprint::tron_wallet_fingerprint),
        )
        .route(
            "/api/tron/wallet/{address}/ai-risk",
            get(tron_wallet_ai_risk::tron_wallet_ai_risk),
        )
        .route(
            "/api/tron/wallet/{address}/holdings",
            get(tron_wallet_holdings::tron_wallet_holdings),
        )
        .route(
            "/api/tron/wallet/{address}/investigation",
            get(tron_wallet_investigation::tron_wallet_investigation),
        )
        .route(
            "/api/analysis/tron/wallet/{address}",
            get(tron_wallet_analysis::tron_wallet_analysis_snapshot),
        )
        .route(
            "/api/tron/wallet/{address}/neo4j/import",
            post(health::central_projection_only),
        )
        .route(
            "/api/tron/wallet/{source}/paths/{target}",
            get(tron_wallet_paths::tron_wallet_paths),
        )
        .route(
            "/api/tron/wallet/{source}/paths/{target}/neo4j/import",
            post(health::central_projection_only),
        )
        .layer(middleware::from_fn_with_state(
            service_auth,
            require_service_auth,
        ));

    Ok(public_routes.merge(protected_routes))
}
