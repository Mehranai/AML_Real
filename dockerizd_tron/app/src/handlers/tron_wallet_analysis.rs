use axum::{
    Json,
    extract::{Path, Query},
};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{TronApiError, clickhouse_client, normalize_wallet_address};
use crate::services::tron::{
    analytical_node::{WalletAnalysisSnapshotResponse, get_or_create_wallet_analysis_snapshot},
    wallet_investigation::WalletInvestigationOptions,
};

#[derive(Debug, Deserialize)]
pub struct WalletAnalysisQuery {
    pub refresh: Option<bool>,
    pub depth: Option<u8>,
    pub limit: Option<u64>,
    pub window_days: Option<u16>,
    pub top_counterparties: Option<usize>,
    pub max_events: Option<u64>,
    pub holdings_limit: Option<u64>,
}

pub async fn tron_wallet_analysis_snapshot(
    Path(address): Path<String>,
    Query(params): Query<WalletAnalysisQuery>,
) -> Result<Json<WalletAnalysisSnapshotResponse>, TronApiError> {
    let config = AppConfig::from_env();
    let address = normalize_wallet_address(&address)?;
    let clickhouse = clickhouse_client(&config);
    let options = WalletInvestigationOptions::new(
        params.depth,
        params.limit,
        params.window_days,
        params.top_counterparties,
        params.max_events,
        params.holdings_limit,
    );

    let snapshot = get_or_create_wallet_analysis_snapshot(
        clickhouse,
        &address,
        options,
        params.refresh.unwrap_or(false),
    )
    .await
    .map_err(TronApiError::internal)?;

    Ok(Json(snapshot))
}
