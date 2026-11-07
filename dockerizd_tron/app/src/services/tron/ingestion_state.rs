use crate::db::Client;
use anyhow::Context;
use serde::Deserialize;
use sha2::{Digest, Sha256};

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

#[allow(clippy::too_many_arguments)]
pub async fn record_ingestion_failure(
    client: &Client,
    block_number: u64,
    block_hash: &str,
    tx_hash: &str,
    stage: &str,
    error_class: &str,
    error_message: &str,
    retryable: bool,
) -> anyhow::Result<()> {
    let id = failure_id(block_number, tx_hash, stage);
    let old = client
        .rows(
            "aml_ingestion_failures",
            &format!("failure_id={}", aml_warehouse::publication::literal(&id)),
        )
        .await?
        .pop();
    let now = crate::db::now_ms();
    let hash =
        |s: &str| aml_warehouse::values::normalize_hash(aml_warehouse::Network::Tron, s).ok();
    let previous = old.as_ref();
    let data = serde_json::json!({"failure_id":id,"block_number":block_number,"block_hash":hash(block_hash),"tx_hash":hash(tx_hash),"operation":stage,"status":if retryable{"retry"}else{"dead_letter"},"attempts":previous.and_then(|v|v["attempts"].as_u64()).unwrap_or(0).saturating_add(1).min(u32::MAX as u64),"error_code":error_class,"error_message":truncate_error(error_message),"next_attempt_unix_ms":if retryable{now.saturating_add(1000)}else{0},"retryable":u8::from(retryable),"first_failed_at_unix_ms":previous.and_then(|v|v["first_failed_at_unix_ms"].as_u64()).unwrap_or(now),"last_failed_at_unix_ms":now,"resolved_at_unix_ms":null,"raw_evidence_json":serde_json::json!({"block_hash":block_hash,"tx_hash":tx_hash,"stage":stage}).to_string()});
    let row = crate::db::row(
        "aml_ingestion_failures",
        data,
        client.revision("aml_ingestion_failures").await?,
        now,
    )?;
    client.insert_rows("aml_ingestion_failures", &[row]).await
}
/// An empty stage list is reserved for a fully republished block. Header-only
/// verification must not resolve transaction/receipt or publication failures.
pub async fn resolve_ingestion_failures(
    client: &Client,
    block_number: u64,
    stages: &[&str],
) -> anyhow::Result<()> {
    let mut filter = format!("block_number={block_number} AND status!='resolved'");
    if !stages.is_empty() {
        filter.push_str(&format!(
            " AND operation IN ({})",
            stages
                .iter()
                .map(|stage| aml_warehouse::publication::literal(stage))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    let failures = client.rows("aml_ingestion_failures", &filter).await?;
    for mut row in failures {
        row["status"] = serde_json::json!("resolved");
        row["resolved_at_unix_ms"] = serde_json::json!(crate::db::now_ms());
        row["next_attempt_unix_ms"] = serde_json::json!(0);
        row["revision"] = serde_json::json!(client.revision("aml_ingestion_failures").await?);
        client.insert_rows("aml_ingestion_failures", &[row]).await?;
    }
    Ok(())
}

fn failure_id(block_number: u64, tx_hash: &str, stage: &str) -> String {
    let identity = format!("tron|{block_number}|{tx_hash}|{stage}");
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn truncate_error(error: &str) -> String {
    error.chars().take(4096).collect()
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
