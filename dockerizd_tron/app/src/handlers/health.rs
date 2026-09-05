use std::time::Duration;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use clickhouse::Client;
use neo4rs::query;
use serde::Deserialize;
use serde_json::json;
use tokio::time::timeout;

pub async fn health_check() -> Json<serde_json::Value> {
    Json(json!({"status":"alive"}))
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ReadinessProbe {
    value: u8,
}

pub async fn readiness_check() -> impl IntoResponse {
    let config = crate::config::AppConfig::from_env();
    let clickhouse = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass)
        .with_database(&config.clickhouse_db_tron);

    let clickhouse_ready = timeout(Duration::from_secs(3), async {
        clickhouse
            .query("SELECT toUInt8(1) AS value")
            .fetch_one::<ReadinessProbe>()
            .await
            .map(|probe| probe.value == 1)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or(false);

    let neo4j_ready = timeout(Duration::from_secs(3), async {
        let client = crate::services::tron::neo4j::client::Neo4jClient::new(
            &config.neo4j_uri,
            &config.neo4j_username,
            &config.neo4j_password,
        )
        .await?;
        client.graph.run(query("RETURN 1 AS value")).await?;
        Ok::<bool, anyhow::Error>(true)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or(false);

    let ready = clickhouse_ready && neo4j_ready;
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(json!({
            "status": if ready { "ready" } else { "not_ready" },
            "dependencies": {
                "clickhouse": if clickhouse_ready { "ready" } else { "unavailable" },
                "neo4j": if neo4j_ready { "ready" } else { "unavailable" }
            }
        })),
    )
}
