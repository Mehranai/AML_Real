use axum::{Json, extract::Query};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{TronApiError, clickhouse_client};
use crate::services::tron::ingestion_health::{
    IngestionHealthOptions, TronIngestionHealth, load_tron_ingestion_health,
};

#[derive(Debug, Deserialize)]
pub struct TronIngestionHealthQuery {
    pub gap_window_blocks: Option<u64>,
    pub stale_after_seconds: Option<u64>,
    pub max_lag_blocks: Option<u64>,
}

pub async fn tron_ingestion_health(
    Query(params): Query<TronIngestionHealthQuery>,
) -> Result<Json<TronIngestionHealth>, TronApiError> {
    let config = AppConfig::from_env();
    let clickhouse = clickhouse_client(&config);
    let options = IngestionHealthOptions::bounded(
        params.gap_window_blocks,
        params.stale_after_seconds,
        params.max_lag_blocks,
    );
    let health = load_tron_ingestion_health(&config, &clickhouse, options)
        .await
        .map_err(TronApiError::internal)?;

    Ok(Json(health))
}
