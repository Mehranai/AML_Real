use std::sync::Arc;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use clickhouse::Client;
use serde_json::json;

use crate::{
    config::AppConfig, services::tron::neo4j::client::Neo4jClient,
    utils::tron_address::normalize_tron_address,
};

#[derive(Debug)]
pub struct TronApiError {
    status: StatusCode,
    message: String,
}

impl TronApiError {
    pub fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    pub fn internal(err: anyhow::Error) -> Self {
        eprintln!("TRON API internal error: {err:#}");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal server error".to_string(),
        }
    }
}

impl IntoResponse for TronApiError {
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

pub fn normalize_wallet_address(address: &str) -> Result<String, TronApiError> {
    normalize_tron_address(address)
        .ok_or_else(|| TronApiError::bad_request(format!("invalid Tron wallet address: {address}")))
}

pub fn clickhouse_client(config: &AppConfig) -> Arc<Client> {
    Arc::new(
        Client::default()
            .with_url(&config.clickhouse_url)
            .with_user(&config.clickhouse_user)
            .with_password(&config.clickhouse_pass)
            .with_database(&config.clickhouse_db_tron)
            .with_option("join_algorithm", "auto"),
    )
}

pub async fn neo4j_client(config: &AppConfig) -> Result<Neo4jClient, TronApiError> {
    Neo4jClient::new(
        &config.neo4j_uri,
        &config.neo4j_username,
        &config.neo4j_password,
    )
    .await
    .map_err(TronApiError::internal)
}
