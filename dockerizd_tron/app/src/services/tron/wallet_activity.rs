use std::sync::Arc;

use anyhow::Context;
use chrono::Utc;
use clickhouse::Client;
use serde::{Deserialize, Serialize};

use crate::services::tron::observation_window::{
    ALL_HISTORY_WINDOW_DAYS, normalize_window_days, window_start_unix_ms,
};

#[derive(Debug, Clone, Serialize)]
pub struct WalletActivity {
    pub address: String,
    pub window_days: u16,
    pub bucket_unit: String,
    pub summary: WalletActivitySummary,
    pub trend: Vec<WalletFlowTrendBucket>,
    pub recent_semantic_events: Vec<WalletSemanticEvent>,
    pub semantic_event_limit: u64,
    pub semantic_events_truncated: bool,
    pub generated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WalletActivitySummary {
    pub incoming_transfers: u64,
    pub outgoing_transfers: u64,
    pub unique_transactions: u64,
    pub swap_transactions: u64,
    pub bridge_transactions: u64,
    pub mint_transactions: u64,
    pub burn_transactions: u64,
    pub liquidity_add_transactions: u64,
    pub liquidity_remove_transactions: u64,
    pub contract_call_transactions: u64,
    pub semantic_event_count: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, clickhouse::Row)]
pub struct WalletFlowTrendBucket {
    pub bucket_start_unix_ms: u64,
    pub incoming_transfers: u64,
    pub outgoing_transfers: u64,
    pub unique_transactions: u64,
    pub swap_transactions: u64,
    pub bridge_transactions: u64,
    pub mint_transactions: u64,
    pub burn_transactions: u64,
    pub liquidity_add_transactions: u64,
    pub liquidity_remove_transactions: u64,
    pub contract_call_transactions: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, clickhouse::Row)]
pub struct WalletSemanticEvent {
    pub event_id: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub event_type: String,
    pub protocol: String,
    pub asset_in: String,
    pub asset_out: String,
    pub detector: String,
    pub detector_version: String,
    pub confidence: f32,
    pub evidence_json: String,
}

#[derive(Debug, Clone, Copy)]
enum TrendBucket {
    Day,
    Week,
    Month,
}

impl TrendBucket {
    fn for_window(window_days: u16) -> Self {
        if window_days == ALL_HISTORY_WINDOW_DAYS || window_days > 730 {
            Self::Month
        } else if window_days > 120 {
            Self::Week
        } else {
            Self::Day
        }
    }

    fn duration_ms(self) -> u64 {
        match self {
            Self::Day => 86_400_000,
            Self::Week => 604_800_000,
            Self::Month => 2_592_000_000,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "30_days",
        }
    }
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct CountRow {
    value: u64,
}

pub async fn build_wallet_activity(
    clickhouse: Arc<Client>,
    address: &str,
    window_days: Option<u16>,
    semantic_event_limit: Option<u64>,
) -> anyhow::Result<WalletActivity> {
    let window_days = normalize_window_days(window_days);
    let semantic_event_limit = semantic_event_limit.unwrap_or(100).clamp(1, 500);
    let generated_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let window_start_unix_ms = window_start_unix_ms(generated_at_unix_ms, window_days);
    let bucket = TrendBucket::for_window(window_days);

    let (trend, recent_semantic_events, semantic_event_count) = tokio::try_join!(
        load_flow_trend(
            &clickhouse,
            address,
            window_start_unix_ms,
            bucket.duration_ms()
        ),
        load_semantic_events(
            &clickhouse,
            address,
            window_start_unix_ms,
            semantic_event_limit
        ),
        load_semantic_event_count(&clickhouse, address, window_start_unix_ms),
    )?;

    let mut summary = summarize_trend(&trend);
    summary.semantic_event_count = semantic_event_count;

    Ok(WalletActivity {
        address: address.to_string(),
        window_days,
        bucket_unit: bucket.label().to_string(),
        summary,
        trend,
        semantic_events_truncated: semantic_event_count > recent_semantic_events.len() as u64,
        recent_semantic_events,
        semantic_event_limit,
        generated_at_unix_ms,
    })
}

async fn load_flow_trend(
    clickhouse: &Client,
    address: &str,
    window_start_unix_ms: u64,
    bucket_duration_ms: u64,
) -> anyhow::Result<Vec<WalletFlowTrendBucket>> {
    let query = format!(
        r#"
        SELECT
            intDiv(ar.timestamp, {bucket_duration_ms}) * {bucket_duration_ms}
                AS bucket_start_unix_ms,
            countIf(ar.to_address = ?) AS incoming_transfers,
            countIf(ar.from_address = ?) AS outgoing_transfers,
            uniqExact(ar.tx_hash) AS unique_transactions,
            uniqExactIf(ar.tx_hash, ifNull(feature.is_swap, toUInt8(0)) > 0)
                AS swap_transactions,
            uniqExactIf(ar.tx_hash, ifNull(feature.is_bridge, toUInt8(0)) > 0)
                AS bridge_transactions,
            uniqExactIf(ar.tx_hash, ifNull(feature.is_mint, toUInt8(0)) > 0)
                AS mint_transactions,
            uniqExactIf(ar.tx_hash, ifNull(feature.is_burn, toUInt8(0)) > 0)
                AS burn_transactions,
            uniqExactIf(ar.tx_hash, ifNull(feature.is_liquidity_add, toUInt8(0)) > 0)
                AS liquidity_add_transactions,
            uniqExactIf(ar.tx_hash, ifNull(feature.is_liquidity_remove, toUInt8(0)) > 0)
                AS liquidity_remove_transactions,
            uniqExactIf(ar.tx_hash, ifNull(feature.is_contract_call, toUInt8(0)) > 0)
                AS contract_call_transactions
        FROM address_relationships_canonical AS ar
        LEFT JOIN
        (
            SELECT
                tx_hash,
                argMax(is_swap, inserted_at) AS is_swap,
                argMax(is_bridge, inserted_at) AS is_bridge,
                argMax(is_mint, inserted_at) AS is_mint,
                argMax(is_burn, inserted_at) AS is_burn,
                argMax(is_liquidity_add, inserted_at) AS is_liquidity_add,
                argMax(is_liquidity_remove, inserted_at) AS is_liquidity_remove,
                argMax(is_contract_call, inserted_at) AS is_contract_call
            FROM transaction_features
            GROUP BY tx_hash
        ) AS feature ON feature.tx_hash = ar.tx_hash
        WHERE (ar.from_address = ? OR ar.to_address = ?)
          AND ar.timestamp >= ?
        GROUP BY bucket_start_unix_ms
        ORDER BY bucket_start_unix_ms
        "#
    );

    clickhouse
        .query(&query)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(address)
        .bind(window_start_unix_ms)
        .fetch_all::<WalletFlowTrendBucket>()
        .await
        .context("failed to load TRON wallet fund-flow trend")
}

async fn load_semantic_events(
    clickhouse: &Client,
    address: &str,
    window_start_unix_ms: u64,
    limit: u64,
) -> anyhow::Result<Vec<WalletSemanticEvent>> {
    clickhouse
        .query(
            r#"
            SELECT
                event_id,
                tx_hash,
                block_number,
                timestamp,
                event_type,
                protocol,
                asset_in,
                asset_out,
                detector,
                detector_version,
                confidence,
                evidence_json
            FROM semantic_aml_events FINAL
            WHERE subject_address = ?
              AND timestamp >= ?
            ORDER BY timestamp DESC, event_id
            LIMIT ?
            "#,
        )
        .bind(address)
        .bind(window_start_unix_ms)
        .bind(limit)
        .fetch_all::<WalletSemanticEvent>()
        .await
        .context("failed to load TRON wallet semantic AML events")
}

async fn load_semantic_event_count(
    clickhouse: &Client,
    address: &str,
    window_start_unix_ms: u64,
) -> anyhow::Result<u64> {
    clickhouse
        .query(
            r#"
            SELECT count() AS value
            FROM semantic_aml_events FINAL
            WHERE subject_address = ?
              AND timestamp >= ?
            "#,
        )
        .bind(address)
        .bind(window_start_unix_ms)
        .fetch_one::<CountRow>()
        .await
        .map(|row| row.value)
        .context("failed to count TRON wallet semantic AML events")
}

fn summarize_trend(trend: &[WalletFlowTrendBucket]) -> WalletActivitySummary {
    let mut summary = WalletActivitySummary::default();

    for bucket in trend {
        summary.incoming_transfers = summary
            .incoming_transfers
            .saturating_add(bucket.incoming_transfers);
        summary.outgoing_transfers = summary
            .outgoing_transfers
            .saturating_add(bucket.outgoing_transfers);
        summary.unique_transactions = summary
            .unique_transactions
            .saturating_add(bucket.unique_transactions);
        summary.swap_transactions = summary
            .swap_transactions
            .saturating_add(bucket.swap_transactions);
        summary.bridge_transactions = summary
            .bridge_transactions
            .saturating_add(bucket.bridge_transactions);
        summary.mint_transactions = summary
            .mint_transactions
            .saturating_add(bucket.mint_transactions);
        summary.burn_transactions = summary
            .burn_transactions
            .saturating_add(bucket.burn_transactions);
        summary.liquidity_add_transactions = summary
            .liquidity_add_transactions
            .saturating_add(bucket.liquidity_add_transactions);
        summary.liquidity_remove_transactions = summary
            .liquidity_remove_transactions
            .saturating_add(bucket.liquidity_remove_transactions);
        summary.contract_call_transactions = summary
            .contract_call_transactions
            .saturating_add(bucket.contract_call_transactions);
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_bucket_scales_with_window() {
        assert_eq!(TrendBucket::for_window(0).label(), "30_days");
        assert_eq!(TrendBucket::for_window(90).label(), "day");
        assert_eq!(TrendBucket::for_window(365).label(), "week");
        assert_eq!(TrendBucket::for_window(3_650).label(), "30_days");
    }

    #[test]
    fn trend_summary_adds_operation_counts() {
        let trend = vec![WalletFlowTrendBucket {
            bucket_start_unix_ms: 1,
            incoming_transfers: 2,
            outgoing_transfers: 3,
            unique_transactions: 4,
            swap_transactions: 1,
            bridge_transactions: 1,
            mint_transactions: 0,
            burn_transactions: 0,
            liquidity_add_transactions: 1,
            liquidity_remove_transactions: 0,
            contract_call_transactions: 2,
        }];

        let summary = summarize_trend(&trend);
        assert_eq!(summary.incoming_transfers, 2);
        assert_eq!(summary.outgoing_transfers, 3);
        assert_eq!(summary.swap_transactions, 1);
        assert_eq!(summary.contract_call_transactions, 2);
    }
}
