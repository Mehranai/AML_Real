use std::sync::Arc;

use chrono::Utc;
use clickhouse::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct WalletHoldings {
    pub address: String,
    pub limit: u64,
    pub total_asset_count: u64,
    pub returned_asset_count: usize,
    pub native_balance: Option<WalletAssetHolding>,
    pub assets: Vec<WalletAssetHolding>,
    pub metadata_gap_count: usize,
    pub incomplete_balance_count: usize,
    pub source: String,
    pub generated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletAssetHolding {
    pub asset_type: String,
    pub asset_id: String,
    pub asset_symbol: String,
    pub asset_name: String,
    pub decimals: u8,
    pub balance_raw: String,
    pub balance_decimal: f64,
    pub balance_incomplete: bool,
    pub metadata_available: bool,
    pub metadata_verified: bool,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct WalletAssetHoldingRow {
    asset_type: String,
    asset_id: String,
    asset_symbol: String,
    asset_name: String,
    decimals: u8,
    balance_raw: String,
    balance_decimal: f64,
    balance_incomplete: u8,
    metadata_available: u8,
    metadata_verified: u8,
}

pub async fn build_wallet_holdings(
    clickhouse: Arc<Client>,
    address: &str,
    limit: Option<u64>,
) -> anyhow::Result<WalletHoldings> {
    let limit = limit.unwrap_or(50).clamp(1, 250);
    let generated_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;

    let total_asset_count = clickhouse
        .query(
            r#"
            SELECT count()
            FROM wallet_asset_balances
            WHERE address = ?
            "#,
        )
        .bind(address)
        .fetch_one::<u64>()
        .await?;

    let rows = clickhouse
        .query(
            r#"
            SELECT
                asset_type,
                asset_id,
                asset_symbol,
                asset_name,
                decimals,
                toString(balance_raw) AS balance_raw,
                balance_decimal,
                balance_incomplete,
                metadata_verified,
                if(
                    asset_type = 'native'
                    OR asset_name != ''
                    OR asset_symbol != asset_id,
                    1,
                    0
                ) AS metadata_available
            FROM wallet_asset_balances
            WHERE address = ?
            ORDER BY
                if(asset_type = 'native', 0, 1),
                balance_decimal DESC,
                asset_symbol ASC
            LIMIT ?
            "#,
        )
        .bind(address)
        .bind(limit)
        .fetch_all::<WalletAssetHoldingRow>()
        .await?;

    let assets = rows
        .into_iter()
        .map(|row| WalletAssetHolding {
            asset_type: row.asset_type,
            asset_id: row.asset_id,
            asset_symbol: row.asset_symbol,
            asset_name: row.asset_name,
            decimals: row.decimals,
            balance_raw: row.balance_raw,
            balance_decimal: row.balance_decimal,
            balance_incomplete: row.balance_incomplete == 1,
            metadata_available: row.metadata_available == 1,
            metadata_verified: row.metadata_verified == 1,
        })
        .collect::<Vec<_>>();

    let native_balance = assets
        .iter()
        .find(|asset| asset.asset_type == "native")
        .cloned();
    let metadata_gap_count = assets
        .iter()
        .filter(|asset| asset.asset_type != "native" && !asset.metadata_available)
        .count();
    let incomplete_balance_count = assets
        .iter()
        .filter(|asset| asset.balance_incomplete)
        .count();

    Ok(WalletHoldings {
        address: address.to_string(),
        limit,
        total_asset_count,
        returned_asset_count: assets.len(),
        native_balance,
        assets,
        metadata_gap_count,
        incomplete_balance_count,
        source: "wallet_asset_balances".to_string(),
        generated_at_unix_ms,
    })
}
