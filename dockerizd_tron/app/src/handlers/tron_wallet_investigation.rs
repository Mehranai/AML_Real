use aml_chain_api::{ApiPath as Path, ApiQuery as Query, bounded};
use axum::Json;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{TronApiError, clickhouse_client, normalize_wallet_address};
use crate::services::tron::wallet_investigation::{
    WalletInvestigationOptions, build_wallet_investigation,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalletInvestigationQuery {
    pub depth: Option<u8>,
    pub limit: Option<u64>,
    pub window_days: Option<u16>,
    pub top_counterparties: Option<usize>,
    pub max_events: Option<u64>,
    pub holdings_limit: Option<u64>,
}

impl WalletInvestigationQuery {
    fn validate(&self) -> Result<(), TronApiError> {
        let validate = || -> Result<(), aml_chain_api::ApiError> {
            bounded("depth", self.depth, 1, 6)?;
            bounded("limit", self.limit, 1, 2_000)?;
            bounded(
                "window_days",
                self.window_days,
                0,
                crate::services::tron::observation_window::MAX_BOUNDED_WINDOW_DAYS,
            )?;
            bounded("top_counterparties", self.top_counterparties, 1, 100)?;
            bounded("max_events", self.max_events, 100, 250_000)?;
            bounded("holdings_limit", self.holdings_limit, 1, 250)
        };
        validate().map_err(|e| TronApiError::bad_request(e.message))
    }
}

pub async fn tron_wallet_investigation(
    Path(address): Path<String>,
    Query(params): Query<WalletInvestigationQuery>,
) -> Result<Json<serde_json::Value>, TronApiError> {
    params.validate()?;
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

    let value =
        serde_json::to_value(investigation).map_err(|e| TronApiError::internal(e.into()))?;
    Ok(Json(
        aml_chain_api::response::normalize(
            aml_warehouse::Network::Tron,
            aml_chain_api::response::Mode::Wallet,
            value,
        )
        .map_err(TronApiError::internal)?,
    ))
}

pub async fn tron_wallet_transfers(
    Path(raw): Path<String>,
    Query(query): Query<aml_warehouse::relationships::PageQuery>,
) -> Result<Json<serde_json::Value>, aml_chain_api::ApiError> {
    let address = crate::utils::tron_address::normalize_tron_address(&raw).ok_or_else(|| {
        aml_chain_api::ApiError::new(axum::http::StatusCode::BAD_REQUEST, "invalid TRON address")
    })?;
    let subject =
        aml_warehouse::relationships::Subject::address(aml_warehouse::Network::Tron, &address)
            .map_err(|e| {
                aml_chain_api::ApiError::new(axum::http::StatusCode::BAD_REQUEST, e.to_string())
            })?;
    let config = AppConfig::from_env();
    let db = clickhouse_client(&config);
    aml_warehouse::relationships::read_page(
        &*db,
        db.database(),
        aml_warehouse::Network::Tron,
        &subject,
        &query,
    )
    .await
    .map(Json)
    .map_err(aml_chain_api::transfer_error)
}
