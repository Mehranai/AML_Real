use axum::{
    Json,
    extract::{Path, Query},
};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{TronApiError, clickhouse_client, normalize_wallet_address};
use crate::services::tron::wallet_investigation::{
    WalletInvestigation, WalletInvestigationOptions, build_wallet_investigation,
};

#[derive(Debug, Deserialize)]
pub struct WalletInvestigationQuery {
    pub depth: Option<u8>,
    pub limit: Option<u64>,
    pub window_days: Option<u16>,
    pub top_counterparties: Option<usize>,
    pub max_events: Option<u64>,
    pub holdings_limit: Option<u64>,
}

pub async fn tron_wallet_investigation(
    Path(address): Path<String>,
    Query(params): Query<WalletInvestigationQuery>,
) -> Result<Json<WalletInvestigation>, TronApiError> {
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

    let investigation = build_wallet_investigation(clickhouse, &address, options)
        .await
        .map_err(TronApiError::internal)?;

    Ok(Json(investigation))
}
