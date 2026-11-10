//! Synchronous evidence write, verification and final publication fence.
use crate::{
    db::warehouse::{BlockHeader, from_state, map_evidence, to_state, validate_evidence},
    db::{Database, SyncState, timestamp},
    domain::model::Evidence,
};
use aml_warehouse::{UnixMillis, publication::Block};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::BTreeMap;
pub struct WriteContext<'a> {
    pub observed: u64,
    pub start: u64,
    pub source_id: &'a str,
}

pub async fn write_block(
    db: &Database,
    previous: &SyncState,
    source: &BlockHeader,
    evidence: &Evidence,
    context: WriteContext<'_>,
) -> Result<SyncState> {
    write_block_mode(db, previous, source, evidence, context, false).await
}
pub(crate) async fn write_block_mode(
    db: &Database,
    previous: &SyncState,
    source: &BlockHeader,
    evidence: &Evidence,
    context: WriteContext<'_>,
    refresh: bool,
) -> Result<SyncState> {
    let WriteContext {
        observed,
        start,
        source_id,
    } = context;
    validate_evidence(source, evidence)?;
    if !refresh {
        ensure!(
            source.block_height
                == if previous.initialized == 1 {
                    previous
                        .tip_height
                        .checked_add(1)
                        .context("height exhausted")?
                } else {
                    start
                },
            "non-contiguous warehouse block"
        );
        if previous.initialized == 1 {
            ensure!(
                source.parent_hash == previous.tip_hash,
                "warehouse parent hash mismatch"
            );
        }
    }
    let revision = db.next_revision(previous.revision).await?;
    let block = Block {
        network_id: crate::NETWORK_ID.into(),
        block_number: source.block_height,
        block_hash: source.block_hash.clone(),
        block_timestamp_unix_ms: UnixMillis::from_seconds(source.block_time)?.0,
        commit_id: format!("{revision}-{}", source.block_hash),
        revision,
        parent_hash: source.parent_hash.clone(),
        canonical: 1,
        ingestion_status: "complete".into(),
        transaction_count: source.transaction_count,
        receipt_coverage: None,
        trace_coverage: None,
        prevout_coverage: Some(1),
        source_id: source_id.into(),
        created_at_unix_ms: timestamp(),
    };
    let tables = map_evidence(&block, evidence)?
        .into_iter()
        .map(|(table, rows)| (table.to_owned(), rows))
        .collect();
    let connection = db.warehouse_connection();
    let mut source_evidence = serde_json::to_value(source)?;
    source_evidence["semantic_detection"] = crate::semantics::block_manifest(evidence);
    let old = to_state(previous);
    let raw = source_evidence.to_string();
    let next = if refresh {
        aml_warehouse::core::refresh_observed_batch(
            &connection,
            db.database(),
            &old,
            &block,
            &tables,
            &raw,
            observed,
        )
        .await?
    } else {
        aml_warehouse::core::publish_observed_batch(
            &connection,
            db.database(),
            &old,
            &block,
            &tables,
            &raw,
            observed,
        )
        .await?
    };
    Ok(from_state(next))
}

pub async fn verify_rows(
    db: &Database,
    block: &Block,
    tables: &BTreeMap<&str, Vec<Value>>,
) -> Result<()> {
    aml_warehouse::core::verify_facts(&db.warehouse_connection(), db.database(), block, tables)
        .await
}
