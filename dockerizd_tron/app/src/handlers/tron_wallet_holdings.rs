use axum::{
    extract::{Path, Query},
    response::Json,
};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{TronApiError, clickhouse_client, normalize_wallet_address};
use crate::services::tron::wallet_holdings::{WalletHoldings, build_wallet_holdings};

#[derive(Debug, Deserialize)]
pub struct WalletHoldingsQuery {
    pub limit: Option<u64>,
}

pub async fn tron_wallet_holdings(
    Path(address): Path<String>,
    Query(params): Query<WalletHoldingsQuery>,
) -> Result<Json<WalletHoldings>, TronApiError> {
    let config = AppConfig::from_env();
    let address = normalize_wallet_address(&address)?;
    let clickhouse = clickhouse_client(&config);

    let holdings = build_wallet_holdings(clickhouse, &address, params.limit)
        .await
        .map_err(TronApiError::internal)?;

    Ok(Json(holdings))
}
