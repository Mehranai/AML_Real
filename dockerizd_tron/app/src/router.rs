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
    Ok(router(service_auth))
}

fn router(service_auth: ServiceAuth) -> Router {
    let public_routes = Router::new()
        .route("/", get(dashboard::dashboard))
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check));
    let protected_routes = Router::new()
        .route("/status", get(status::status))
        .route(
            aml_chain_api::bridge::TRANSACTION_ROUTE,
            get(status::bridge_transaction),
        )
        .route(
            aml_chain_api::bridge::ALLBRIDGE_OPERATION_ROUTE,
            get(status::allbridge_operation),
        )
        .route(
            aml_chain_api::bridge::SNAPSHOT_ROUTE,
            get(status::bridge_snapshot),
        )
        .route(
            aml_chain_api::kyt::TRANSACTION_ROUTE,
            get(status::kyt_transaction),
        )
        .route(
            "/api/tron/wallet/{address}/transfers",
            get(tron_wallet_investigation::tron_wallet_transfers),
        )
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
        ))
        .layer(middleware::from_fn(aml_chain_api::no_store));

    public_routes.merge(protected_routes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn api_boundary_contract() -> anyhow::Result<()> {
        let key = "api-contract-fixture-key-0123456789";
        let app = router(ServiceAuth::required(key)?);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        let address =
            crate::utils::tron_address::hex_to_base58(&format!("41{}", "11".repeat(20))).unwrap();
        let target =
            crate::utils::tron_address::hex_to_base58(&format!("41{}", "22".repeat(20))).unwrap();
        let wallet = format!("/api/tron/wallet/{address}");
        let paths = format!("paths/{target}");
        for (suffix, code) in [
            ("investigation?unknown=1", "invalid_query"),
            ("investigation?limit=no", "invalid_query"),
            ("investigation?limit=-1", "invalid_query"),
            ("investigation?limit=1&limit=2", "invalid_query"),
            ("investigation?limit=0", "invalid_request"),
            ("investigation?limit=2001", "invalid_request"),
            ("investigation?depth=7", "invalid_request"),
            ("investigation?max_events=99", "invalid_request"),
            ("investigation?max_events=250001", "invalid_request"),
            ("investigation?window_days=3651", "invalid_request"),
            ("investigation?top_counterparties=101", "invalid_request"),
            ("investigation?holdings_limit=251", "invalid_request"),
            ("holdings?asset=unsupported", "invalid_query"),
            ("transfers?unknown=1", "invalid_query"),
            ("transfers?limit=1&limit=2", "invalid_query"),
            ("transfers?direction=typo", "invalid_query"),
            ("transfers?limit=0", "invalid_request"),
            ("transfers?limit=101", "invalid_request"),
            ("transfers?from_block=2&to_block=1", "invalid_request"),
            ("transfers?from_unix_ms=2&to_unix_ms=1", "invalid_request"),
            ("transfers?asset_id=native:BTC", "invalid_request"),
            ("transfers?cursor=00", "invalid_request"),
            ("holdings?limit=0", "invalid_request"),
            ("holdings?limit=251", "invalid_request"),
        ]
        .into_iter()
        .map(|(s, c)| (s.to_string(), c))
        .chain([
            (format!("{paths}?unknown=1"), "invalid_query"),
            (format!("{paths}?max_hops=1&max_hops=2"), "invalid_query"),
            (format!("{paths}?max_hops=1&max_depth=2"), "invalid_query"),
            (format!("{paths}?max_hops=0"), "invalid_request"),
            (format!("{paths}?max_hops=11"), "invalid_request"),
            (format!("{paths}?max_depth=11"), "invalid_request"),
            (format!("{paths}?limit=51"), "invalid_request"),
            (format!("{paths}?per_address_limit=2001"), "invalid_request"),
            (format!("{paths}?direction=typo"), "invalid_request"),
            (format!("paths/{address}"), "invalid_request"),
        ]) {
            let response = client
                .get(format!("{base}{wallet}/{suffix}"))
                .header("X-AML-Service-Key", key)
                .send()
                .await?;
            assert_eq!(response.status().as_u16(), 400, "{suffix}");
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: Value = response.json().await?;
            assert_eq!(body["api_version"], "1.0");
            assert_eq!(body["status"], 400);
            assert_eq!(body["code"], code, "{suffix}");
            assert!(body["error"].is_string());
        }
        for (path, key, status, code) in [
            (
                "/api/tron/wallet/invalid/investigation?limit=no",
                "",
                401,
                "unauthorized",
            ),
            ("/status", "wrong", 401, "unauthorized"),
            ("/internal/bridge/snapshot", "wrong", 401, "unauthorized"),
            (
                "/api/tron/wallet/%FF/investigation",
                key,
                400,
                "invalid_path",
            ),
            (
                "/api/tron/wallet/invalid/holdings",
                key,
                400,
                "invalid_request",
            ),
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
            assert_eq!(body["api_version"], "1.0");
            assert_eq!(body["status"], status);
        }
        server.abort();
        Ok(())
    }
}
