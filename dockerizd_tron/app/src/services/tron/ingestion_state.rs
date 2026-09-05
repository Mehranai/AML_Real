use anyhow::Context;
use chrono::Utc;
use clickhouse::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::models::tron::modules::{IngestedBlockRow, IngestionFailureRow};

#[derive(Debug, thiserror::Error)]
#[error(
    "finalized TRON block {block_number} changed hash from {existing_hash} to {observed_hash}; explicit reconciliation is required"
)]
pub struct FinalizedHashConflict {
    pub block_number: u64,
    pub existing_hash: String,
    pub observed_hash: String,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct StoredBlockState {
    block_hash: String,
    ingestion_status: String,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct StoredFailureState {
    attempt_count: u32,
    first_failed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockIngestionDecision {
    Ingest,
    Skip,
}

fn decide_block_ingestion(
    block_number: u64,
    block_hash: &str,
    existing: Option<&StoredBlockState>,
    force_replay: bool,
) -> Result<BlockIngestionDecision, FinalizedHashConflict> {
    match existing {
        None => Ok(BlockIngestionDecision::Ingest),
        Some(row) if row.block_hash == block_hash && row.ingestion_status == "COMPLETE" => {
            if force_replay {
                Ok(BlockIngestionDecision::Ingest)
            } else {
                Ok(BlockIngestionDecision::Skip)
            }
        }
        Some(row) if row.block_hash == block_hash => Ok(BlockIngestionDecision::Ingest),
        Some(row) => Err(FinalizedHashConflict {
            block_number,
            existing_hash: row.block_hash.clone(),
            observed_hash: block_hash.to_string(),
        }),
    }
}

pub async fn should_ingest_block(
    clickhouse: &Client,
    block_number: u64,
    block_hash: &str,
    force_replay: bool,
) -> anyhow::Result<bool> {
    let existing = clickhouse
        .query(
            r#"
            SELECT block_hash, ingestion_status
            FROM ingested_blocks FINAL
            WHERE chain = 'tron'
              AND block_number = ?
            LIMIT 1
            "#,
        )
        .bind(block_number)
        .fetch_optional::<StoredBlockState>()
        .await
        .with_context(|| format!("failed to load ingestion state for block {block_number}"))?;

    Ok(matches!(
        decide_block_ingestion(block_number, block_hash, existing.as_ref(), force_replay)?,
        BlockIngestionDecision::Ingest
    ))
}

pub async fn record_processing_block(
    clickhouse: &Client,
    block_number: u64,
    block_hash: String,
    parent_hash: String,
    block_timestamp: u64,
    transaction_count: u32,
) -> anyhow::Result<()> {
    record_block_state(
        clickhouse,
        block_number,
        block_hash,
        parent_hash,
        block_timestamp,
        transaction_count,
        "PROCESSING",
        String::new(),
    )
    .await
}

pub async fn record_failed_block(
    clickhouse: &Client,
    block_number: u64,
    block_hash: String,
    parent_hash: String,
    block_timestamp: u64,
    transaction_count: u32,
    error_message: String,
) -> anyhow::Result<()> {
    record_block_state(
        clickhouse,
        block_number,
        block_hash,
        parent_hash,
        block_timestamp,
        transaction_count,
        "FAILED",
        truncate_error(&error_message),
    )
    .await
}

pub async fn record_ingested_block(
    clickhouse: &Client,
    block_number: u64,
    block_hash: String,
    parent_hash: String,
    block_timestamp: u64,
    transaction_count: u32,
) -> anyhow::Result<()> {
    record_block_state(
        clickhouse,
        block_number,
        block_hash,
        parent_hash,
        block_timestamp,
        transaction_count,
        "COMPLETE",
        String::new(),
    )
    .await?;
    resolve_ingestion_failures(clickhouse, block_number).await
}

#[allow(clippy::too_many_arguments)]
async fn record_block_state(
    clickhouse: &Client,
    block_number: u64,
    block_hash: String,
    parent_hash: String,
    block_timestamp: u64,
    transaction_count: u32,
    ingestion_status: &str,
    error_message: String,
) -> anyhow::Result<()> {
    let row = IngestedBlockRow {
        chain: "tron".to_string(),
        block_number,
        block_hash,
        parent_hash,
        block_timestamp,
        transaction_count,
        finality_status: "SOLID".to_string(),
        ingestion_status: ingestion_status.to_string(),
        error_message,
        indexed_at_unix_ms: now_unix_ms(),
    };
    let mut insert = clickhouse
        .insert::<IngestedBlockRow>("ingested_blocks")
        .await?;

    insert.write(&row).await?;
    insert.end().await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn record_ingestion_failure(
    clickhouse: &Client,
    block_number: u64,
    block_hash: &str,
    tx_hash: &str,
    stage: &str,
    error_class: &str,
    error_message: &str,
    retryable: bool,
) -> anyhow::Result<()> {
    let failure_id = failure_id(block_number, tx_hash, stage);
    let existing = clickhouse
        .query(
            r#"
            SELECT attempt_count, first_failed_at_unix_ms
            FROM ingestion_failures FINAL
            WHERE chain = 'tron'
              AND failure_id = ?
            LIMIT 1
            "#,
        )
        .bind(&failure_id)
        .fetch_optional::<StoredFailureState>()
        .await
        .with_context(|| {
            format!("failed to load ingestion failure state for block {block_number}")
        })?;
    let now = now_unix_ms();
    let (attempt_count, first_failed_at_unix_ms) = existing.map_or((1, now), |row| {
        (
            row.attempt_count.saturating_add(1),
            row.first_failed_at_unix_ms,
        )
    });
    let row = IngestionFailureRow {
        failure_id,
        chain: "tron".to_string(),
        block_number,
        block_hash: block_hash.to_string(),
        tx_hash: tx_hash.to_string(),
        stage: stage.to_string(),
        error_class: error_class.to_string(),
        error_message: truncate_error(error_message),
        retryable: u8::from(retryable),
        attempt_count,
        status: "OPEN".to_string(),
        first_failed_at_unix_ms,
        last_failed_at_unix_ms: now,
        resolved_at_unix_ms: 0,
    };
    let mut insert = clickhouse
        .insert::<IngestionFailureRow>("ingestion_failures")
        .await?;

    insert.write(&row).await?;
    insert.end().await?;

    Ok(())
}

pub async fn resolve_ingestion_failures(
    clickhouse: &Client,
    block_number: u64,
) -> anyhow::Result<()> {
    let failures = clickhouse
        .query(
            r#"
            SELECT
                failure_id,
                chain,
                block_number,
                block_hash,
                tx_hash,
                stage,
                error_class,
                error_message,
                retryable,
                attempt_count,
                status,
                first_failed_at_unix_ms,
                last_failed_at_unix_ms,
                resolved_at_unix_ms
            FROM ingestion_failures FINAL
            WHERE chain = 'tron'
              AND block_number = ?
              AND status = 'OPEN'
            "#,
        )
        .bind(block_number)
        .fetch_all::<IngestionFailureRow>()
        .await
        .with_context(|| format!("failed to load open failures for block {block_number}"))?;

    if failures.is_empty() {
        return Ok(());
    }

    let resolved_at_unix_ms = now_unix_ms();
    let mut insert = clickhouse
        .insert::<IngestionFailureRow>("ingestion_failures")
        .await?;

    for mut failure in failures {
        failure.status = "RESOLVED".to_string();
        failure.resolved_at_unix_ms = resolved_at_unix_ms;
        insert.write(&failure).await?;
    }

    insert.end().await?;

    Ok(())
}

fn failure_id(block_number: u64, tx_hash: &str, stage: &str) -> String {
    let identity = format!("tron|{block_number}|{tx_hash}|{stage}");
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn truncate_error(error: &str) -> String {
    error.chars().take(4096).collect()
}

fn now_unix_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(hash: &str, status: &str) -> StoredBlockState {
        StoredBlockState {
            block_hash: hash.to_string(),
            ingestion_status: status.to_string(),
        }
    }

    #[test]
    fn normal_ingestion_skips_a_complete_matching_block() {
        let row = stored("abc", "COMPLETE");

        assert_eq!(
            decide_block_ingestion(10, "abc", Some(&row), false).unwrap(),
            BlockIngestionDecision::Skip
        );
    }

    #[test]
    fn forced_replay_ingests_a_complete_matching_block() {
        let row = stored("abc", "COMPLETE");

        assert_eq!(
            decide_block_ingestion(10, "abc", Some(&row), true).unwrap(),
            BlockIngestionDecision::Ingest
        );
    }

    #[test]
    fn failed_matching_block_is_retried() {
        let row = stored("abc", "FAILED");

        assert_eq!(
            decide_block_ingestion(10, "abc", Some(&row), false).unwrap(),
            BlockIngestionDecision::Ingest
        );
    }

    #[test]
    fn finalized_hash_conflict_is_never_implicitly_repaired() {
        let row = stored("abc", "COMPLETE");
        let error = decide_block_ingestion(10, "def", Some(&row), true).unwrap_err();

        assert_eq!(error.block_number, 10);
        assert_eq!(error.existing_hash, "abc");
        assert_eq!(error.observed_hash, "def");
    }

    #[test]
    fn failure_identity_is_stable_per_block_transaction_and_stage() {
        assert_eq!(
            failure_id(10, "tx", "PROCESS_TX"),
            failure_id(10, "tx", "PROCESS_TX")
        );
        assert_ne!(
            failure_id(10, "tx", "PROCESS_TX"),
            failure_id(10, "other", "PROCESS_TX")
        );
    }
}
