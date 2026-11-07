use std::sync::Arc;

use crate::db::Client;
use chrono::Utc;
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
    pub decimals: Option<u8>,
    pub balance_raw: String,
    pub balance_decimal: Option<f64>,
    pub balance_incomplete: bool,
    pub metadata_available: bool,
    pub metadata_verified: bool,
    pub decimals_known: bool,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct LedgerLeg {
    asset_id: String,
    incoming: u8,
    amount: String,
}
#[derive(Debug, Deserialize, clickhouse::Row)]
struct Metadata {
    symbol: String,
    name: String,
    decimals: Option<u8>,
    is_verified: u8,
}

pub async fn build_wallet_holdings(
    clickhouse: Arc<Client>,
    address: &str,
    limit: Option<u64>,
) -> anyhow::Result<WalletHoldings> {
    let read_fence = crate::db::client::ReadFence::capture(&clickhouse).await?;
    use aml_warehouse::{Network, visibility};
    use num_bigint::BigInt;
    use std::collections::BTreeMap;
    let limit = limit.unwrap_or(50).clamp(1, 250);
    let snapshot = visibility::capture(&*clickhouse, clickhouse.database(), Network::Tron).await?;
    let rows=clickhouse.query("SELECT asset_id,toUInt8(ifNull(to_address=?,0)) AS incoming,toString(amount_raw) AS amount FROM aml_address_relationships_canonical WHERE network_id='tron:mainnet' AND (from_address=? OR to_address=?) AND from_address!=to_address LIMIT 100001").bind(address).bind(address).bind(address).fetch_all::<LedgerLeg>().await?;
    anyhow::ensure!(
        rows.len() <= 100000,
        "indexed netflow exceeds bounded source scope"
    );
    let mut balances = BTreeMap::<String, BigInt>::new();
    for leg in rows {
        let amount = leg.amount.parse::<BigInt>()?;
        let current = balances.entry(leg.asset_id).or_default();
        if leg.incoming == 1 {
            *current += amount;
        } else {
            *current -= amount;
        }
    }
    let total_asset_count = balances.len() as u64;
    let mut assets = Vec::new();
    for (asset_id, amount) in balances {
        let native = asset_id == "native:TRX";
        let (kind, symbol, name, decimals, verified, known) = if native {
            (
                "native".to_owned(),
                "TRX".to_owned(),
                "TRON".to_owned(),
                6,
                true,
                true,
            )
        } else if let Some(contract) = asset_id.strip_prefix("trc20:") {
            let contract = aml_warehouse::values::tron_wallet(contract)?;
            let metadata=clickhouse.query("SELECT symbol,name,decimals,is_verified FROM token_metadata FINAL WHERE token_address=? LIMIT 1").bind(&contract).fetch_optional::<Metadata>().await?;
            if let Some(m) = metadata {
                (
                    "trc20".into(),
                    m.symbol,
                    m.name,
                    m.decimals.unwrap_or(0),
                    m.is_verified == 1,
                    m.decimals.is_some(),
                )
            } else {
                ("trc20".into(), contract, String::new(), 0, false, false)
            }
        } else if asset_id.starts_with("trc721:") {
            (
                "trc721".into(),
                asset_id.clone(),
                String::new(),
                0,
                false,
                true,
            )
        } else {
            (
                "trc10".into(),
                asset_id.clone(),
                String::new(),
                0,
                false,
                false,
            )
        };
        let raw = amount.to_string();
        let approximate = if known {
            Some(raw.parse::<f64>()? / 10f64.powi(decimals as i32))
        } else {
            None
        };
        let metadata_available = native || !name.is_empty() || (kind == "trc20" && known);
        assets.push(WalletAssetHolding {
            asset_type: kind,
            asset_id,
            asset_symbol: symbol,
            asset_name: name,
            decimals: known.then_some(decimals),
            balance_raw: raw,
            balance_decimal: approximate,
            balance_incomplete: true,
            metadata_available,
            metadata_verified: verified,
            decimals_known: known,
        });
    }
    assets.sort_by(|a, b| {
        (a.asset_type != "native", &a.asset_id).cmp(&(b.asset_type != "native", &b.asset_id))
    });
    assets.truncate(limit as usize);
    visibility::unchanged(&*clickhouse, clickhouse.database(), &snapshot).await?;
    read_fence.unchanged(&clickhouse).await?;
    Ok(WalletHoldings {
        address: address.into(),
        limit,
        total_asset_count,
        returned_asset_count: assets.len(),
        native_balance: assets.iter().find(|a| a.asset_type == "native").cloned(),
        metadata_gap_count: assets
            .iter()
            .filter(|a| a.asset_type != "native" && !a.metadata_available)
            .count(),
        incomplete_balance_count: assets.len(),
        assets,
        source: "canonical_indexed_netflow_not_rpc_balance".into(),
        generated_at_unix_ms: Utc::now().timestamp_millis().max(0) as u64,
    })
}
