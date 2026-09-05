use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use clickhouse::Client;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::config::AppConfig;
use crate::helper::tron::TronClient;
use crate::services::tron::performance::{StorageSnapshot, load_storage_snapshot};

#[derive(Debug, Clone, Copy)]
pub struct IngestionHealthOptions {
    pub gap_window_blocks: u64,
    pub stale_after_seconds: u64,
    pub max_lag_blocks: u64,
}

impl IngestionHealthOptions {
    pub fn bounded(
        gap_window_blocks: Option<u64>,
        stale_after_seconds: Option<u64>,
        max_lag_blocks: Option<u64>,
    ) -> Self {
        Self {
            gap_window_blocks: gap_window_blocks.unwrap_or(1_000).clamp(1, 10_000),
            stale_after_seconds: stale_after_seconds.unwrap_or(600).clamp(30, 86_400),
            max_lag_blocks: max_lag_blocks.unwrap_or(20).clamp(1, 100_000),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TronIngestionHealth {
    pub status: String,
    pub source_kind: String,
    pub source_available: bool,
    pub source_error: String,
    pub latest_solid_block: Option<u64>,
    pub checkpoint_block: Option<u64>,
    pub lag_blocks: Option<u64>,
    pub max_lag_blocks: u64,
    pub block_journal: BlockJournalHealth,
    pub failures: FailureHealth,
    pub storage: StorageSnapshot,
    pub generated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockJournalHealth {
    pub processing_blocks: u64,
    pub stale_processing_blocks: u64,
    pub failed_blocks: u64,
    pub gap_window_start: Option<u64>,
    pub gap_window_end: Option<u64>,
    pub complete_blocks_in_window: u64,
    pub missing_block_count: u64,
    pub missing_ranges: Vec<BlockRange>,
    pub missing_ranges_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockRange {
    pub start_block: u64,
    pub end_block: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureHealth {
    pub open_failures: u64,
    pub retryable_failures: u64,
    pub non_retryable_failures: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct CheckpointRow {
    last_synced_block: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct JournalSummaryRow {
    processing_blocks: u64,
    stale_processing_blocks: u64,
    failed_blocks: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct FailureSummaryRow {
    open_failures: u64,
    retryable_failures: u64,
    non_retryable_failures: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct BlockNumberRow {
    block_number: u64,
}

pub async fn load_tron_ingestion_health(
    config: &AppConfig,
    clickhouse: &Client,
    options: IngestionHealthOptions,
) -> anyhow::Result<TronIngestionHealth> {
    let checkpoint_block = load_checkpoint(clickhouse).await?;
    let now_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
    let stale_before_unix_ms =
        now_unix_ms.saturating_sub(options.stale_after_seconds.saturating_mul(1_000));
    let journal_summary = load_journal_summary(clickhouse, stale_before_unix_ms).await?;
    let failure_summary = load_failure_summary(clickhouse).await?;
    let gap_health =
        load_gap_health(clickhouse, checkpoint_block, options.gap_window_blocks).await?;
    let storage = load_storage_snapshot(clickhouse, &config.clickhouse_db_tron).await?;
    let source_kind = source_kind(config.tron_rpc_url.as_deref().unwrap_or_default());
    let (latest_solid_block, source_error) = load_latest_solid_block(config).await;
    let lag_blocks = latest_solid_block
        .zip(checkpoint_block)
        .map(|(latest, checkpoint)| latest.saturating_sub(checkpoint));
    let status = health_status(
        latest_solid_block,
        checkpoint_block,
        lag_blocks,
        options.max_lag_blocks,
        &journal_summary,
        &failure_summary,
        gap_health.missing_block_count,
    );

    Ok(TronIngestionHealth {
        status,
        source_kind,
        source_available: latest_solid_block.is_some(),
        source_error,
        latest_solid_block,
        checkpoint_block,
        lag_blocks,
        max_lag_blocks: options.max_lag_blocks,
        block_journal: BlockJournalHealth {
            processing_blocks: journal_summary.processing_blocks,
            stale_processing_blocks: journal_summary.stale_processing_blocks,
            failed_blocks: journal_summary.failed_blocks,
            gap_window_start: gap_health.window_start,
            gap_window_end: gap_health.window_end,
            complete_blocks_in_window: gap_health.complete_blocks,
            missing_block_count: gap_health.missing_block_count,
            missing_ranges: gap_health.missing_ranges,
            missing_ranges_truncated: gap_health.missing_ranges_truncated,
        },
        failures: FailureHealth {
            open_failures: failure_summary.open_failures,
            retryable_failures: failure_summary.retryable_failures,
            non_retryable_failures: failure_summary.non_retryable_failures,
        },
        storage,
        generated_at_unix_ms: now_unix_ms,
    })
}

async fn load_checkpoint(clickhouse: &Client) -> anyhow::Result<Option<u64>> {
    clickhouse
        .query(
            r#"
            SELECT argMax(last_synced_block, updated_at) AS last_synced_block
            FROM sync_state
            WHERE chain = 'tron'
            GROUP BY chain
            "#,
        )
        .fetch_optional::<CheckpointRow>()
        .await
        .map(|row| row.map(|row| row.last_synced_block))
        .context("failed to load TRON ingestion checkpoint")
}

async fn load_journal_summary(
    clickhouse: &Client,
    stale_before_unix_ms: u64,
) -> anyhow::Result<JournalSummaryRow> {
    clickhouse
        .query(
            r#"
            SELECT
                countIf(ingestion_status = 'PROCESSING') AS processing_blocks,
                countIf(
                    ingestion_status = 'PROCESSING'
                    AND indexed_at_unix_ms < ?
                ) AS stale_processing_blocks,
                countIf(ingestion_status = 'FAILED') AS failed_blocks
            FROM ingested_blocks FINAL
            WHERE chain = 'tron'
            "#,
        )
        .bind(stale_before_unix_ms)
        .fetch_one::<JournalSummaryRow>()
        .await
        .context("failed to summarize TRON block journal")
}

async fn load_failure_summary(clickhouse: &Client) -> anyhow::Result<FailureSummaryRow> {
    clickhouse
        .query(
            r#"
            SELECT
                countIf(status = 'OPEN') AS open_failures,
                countIf(status = 'OPEN' AND retryable = 1) AS retryable_failures,
                countIf(status = 'OPEN' AND retryable = 0) AS non_retryable_failures
            FROM ingestion_failures FINAL
            WHERE chain = 'tron'
            "#,
        )
        .fetch_one::<FailureSummaryRow>()
        .await
        .context("failed to summarize TRON ingestion failures")
}

struct GapHealth {
    window_start: Option<u64>,
    window_end: Option<u64>,
    complete_blocks: u64,
    missing_block_count: u64,
    missing_ranges: Vec<BlockRange>,
    missing_ranges_truncated: bool,
}

async fn load_gap_health(
    clickhouse: &Client,
    checkpoint_block: Option<u64>,
    gap_window_blocks: u64,
) -> anyhow::Result<GapHealth> {
    let Some(window_end) = checkpoint_block else {
        return Ok(GapHealth {
            window_start: None,
            window_end: None,
            complete_blocks: 0,
            missing_block_count: 0,
            missing_ranges: Vec::new(),
            missing_ranges_truncated: false,
        });
    };
    let window_start = window_end.saturating_sub(gap_window_blocks.saturating_sub(1));
    let complete = clickhouse
        .query(
            r#"
            SELECT block_number
            FROM ingested_blocks FINAL
            WHERE chain = 'tron'
              AND ingestion_status = 'COMPLETE'
              AND block_number BETWEEN ? AND ?
            ORDER BY block_number
            "#,
        )
        .bind(window_start)
        .bind(window_end)
        .fetch_all::<BlockNumberRow>()
        .await
        .context("failed to load TRON block journal window")?
        .into_iter()
        .map(|row| row.block_number)
        .collect::<HashSet<_>>();
    let (missing_ranges, missing_block_count, missing_ranges_truncated) =
        find_missing_ranges(window_start, window_end, &complete, 100);

    Ok(GapHealth {
        window_start: Some(window_start),
        window_end: Some(window_end),
        complete_blocks: complete.len() as u64,
        missing_block_count,
        missing_ranges,
        missing_ranges_truncated,
    })
}

fn find_missing_ranges(
    start_block: u64,
    end_block: u64,
    complete: &HashSet<u64>,
    max_ranges: usize,
) -> (Vec<BlockRange>, u64, bool) {
    let mut ranges = Vec::new();
    let mut current_start = None;
    let mut missing_count = 0_u64;
    let mut truncated = false;

    for block_number in start_block..=end_block {
        if complete.contains(&block_number) {
            if let Some(range_start) = current_start.take() {
                if ranges.len() < max_ranges {
                    ranges.push(BlockRange {
                        start_block: range_start,
                        end_block: block_number - 1,
                    });
                } else {
                    truncated = true;
                }
            }
        } else {
            missing_count = missing_count.saturating_add(1);
            current_start.get_or_insert(block_number);
        }
    }

    if let Some(range_start) = current_start {
        if ranges.len() < max_ranges {
            ranges.push(BlockRange {
                start_block: range_start,
                end_block,
            });
        } else {
            truncated = true;
        }
    }

    (ranges, missing_count, truncated)
}

async fn load_latest_solid_block(config: &AppConfig) -> (Option<u64>, String) {
    let Some(rpc_url) = config.tron_rpc_url.as_deref() else {
        return (None, "TRON_RPC_URL is not configured".to_string());
    };
    let client = match TronClient::new(
        rpc_url,
        config.tron_api_key.clone(),
        config.rpc_timeout_seconds.min(8),
    ) {
        Ok(client) => client,
        Err(error) => return (None, format!("{error:#}")),
    };

    match timeout(Duration::from_secs(8), client.get_solid_block_number()).await {
        Ok(Ok(block_number)) => (Some(block_number), String::new()),
        Ok(Err(error)) => (None, format!("{error:#}")),
        Err(_) => (
            None,
            "TRON solid-head request timed out after 8 seconds".to_string(),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn health_status(
    latest_solid_block: Option<u64>,
    checkpoint_block: Option<u64>,
    lag_blocks: Option<u64>,
    max_lag_blocks: u64,
    journal: &JournalSummaryRow,
    failures: &FailureSummaryRow,
    missing_block_count: u64,
) -> String {
    if checkpoint_block.is_none() {
        return "NOT_STARTED".to_string();
    }

    if latest_solid_block.is_none()
        || journal.stale_processing_blocks > 0
        || journal.failed_blocks > 0
        || failures.non_retryable_failures > 0
        || missing_block_count > 0
    {
        return "DEGRADED".to_string();
    }

    if lag_blocks.is_some_and(|lag| lag > max_lag_blocks) {
        return "LAGGING".to_string();
    }

    "HEALTHY".to_string()
}

fn source_kind(rpc_url: &str) -> String {
    let rpc_url = rpc_url.to_ascii_lowercase();
    if rpc_url.contains("localhost")
        || rpc_url.contains("127.0.0.1")
        || rpc_url.contains("host.docker.internal")
    {
        "local_node".to_string()
    } else {
        "remote_api".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_missing_blocks_into_ranges() {
        let complete = HashSet::from([10, 11, 14, 17]);
        let (ranges, count, truncated) = find_missing_ranges(10, 17, &complete, 100);

        assert_eq!(count, 4);
        assert!(!truncated);
        assert_eq!(
            ranges,
            vec![
                BlockRange {
                    start_block: 12,
                    end_block: 13,
                },
                BlockRange {
                    start_block: 15,
                    end_block: 16,
                },
            ]
        );
    }

    #[test]
    fn reports_lag_only_after_integrity_checks_pass() {
        let journal = JournalSummaryRow {
            processing_blocks: 0,
            stale_processing_blocks: 0,
            failed_blocks: 0,
        };
        let failures = FailureSummaryRow {
            open_failures: 0,
            retryable_failures: 0,
            non_retryable_failures: 0,
        };

        assert_eq!(
            health_status(Some(200), Some(100), Some(100), 20, &journal, &failures, 0),
            "LAGGING"
        );
        assert_eq!(
            health_status(Some(110), Some(100), Some(10), 20, &journal, &failures, 1),
            "DEGRADED"
        );
    }
}
