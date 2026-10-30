//! Synchronous evidence write, verification and final publication fence.
use crate::{
    db::warehouse::{LegacyBlock, map_evidence, validate_evidence},
    db::{Database, SyncState, timestamp},
    domain::model::Evidence,
};
use aml_warehouse::{
    UnixMillis,
    publication::{self, Block},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub struct WriteContext<'a> {
    pub observed: u64,
    pub start: u64,
    pub source_id: &'a str,
    pub legacy_fence: Option<&'a SyncState>,
}

pub async fn write_block(
    db: &Database,
    previous: &SyncState,
    source: &LegacyBlock,
    evidence: &Evidence,
    context: WriteContext<'_>,
) -> Result<SyncState> {
    let WriteContext {
        observed,
        start,
        source_id,
        legacy_fence,
    } = context;
    validate_evidence(source, evidence)?;
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
    let tables = map_evidence(&block, evidence)?;
    let connection = db.warehouse_connection();
    publication::reserve(&connection, db.database(), &block).await?;
    for (table, rows) in &tables {
        publication::facts(&connection, db.database(), &block, table, rows).await?;
    }
    // Verify every persisted column, including UInt256 decimal strings, before
    // writing the complete marker or moving the visibility fence.
    verify_rows(db, &block, &tables).await?;
    let counts = tables
        .iter()
        .map(|(table, rows)| (table.to_string(), rows.len() as u64))
        .collect();
    let mut source_evidence = serde_json::to_value(source)?;
    source_evidence["semantic_detection"] = crate::semantics::block_manifest(evidence);
    aml_warehouse::core::complete(
        &connection,
        db.database(),
        &block,
        &counts,
        &source_evidence.to_string(),
    )
    .await?;
    if let Some(expected) = legacy_fence {
        ensure!(
            db.legacy_state().await? == *expected,
            "legacy checkpoint changed before publication; stop all legacy writers"
        );
    }
    let next = SyncState {
        id: 0,
        revision,
        initialized: 1,
        start_height: if previous.initialized == 1 {
            previous.start_height
        } else {
            start
        },
        tip_height: source.block_height,
        tip_hash: source.block_hash.clone(),
        observed_rpc_height: observed,
        published_at: timestamp(),
    };
    db.publish_after(previous, &next).await?;
    Ok(next)
}

pub async fn verify_rows(
    db: &Database,
    block: &Block,
    tables: &BTreeMap<&str, Vec<Value>>,
) -> Result<()> {
    for (table, expected) in tables {
        let actual: Vec<Value> = db.rows(&format!("SELECT * EXCEPT network_id FROM {table} FINAL WHERE network_id={{network:String}} AND block_number={{height:UInt64}} AND block_hash={{hash:String}} AND commit_id={{commit:String}} AND revision={{revision:UInt64}}"),&[("network",crate::NETWORK_ID.into()),("height",block.block_number.to_string()),("hash",block.block_hash.clone()),("commit",block.commit_id.clone()),("revision",block.revision.to_string())]).await?;
        let normalize = |mut row: Value| -> Result<String> {
            row.as_object_mut()
                .context("expected evidence object")?
                .remove("network_id");
            for key in ["amount_raw", "fee_raw", "amount_in", "amount_out"] {
                if let Some(value) = row.get_mut(key)
                    && !value.is_null()
                {
                    *value = Value::String(
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string()),
                    );
                }
            }
            if let Some(value) = row.get_mut("confidence") {
                *value = json!(value.as_f64().context("invalid confidence")?);
            }
            Ok(serde_json::to_string(&row)?)
        };
        let mut expected = expected
            .iter()
            .cloned()
            .map(&normalize)
            .collect::<Result<Vec<_>>>()?;
        let mut actual = actual
            .into_iter()
            .map(normalize)
            .collect::<Result<Vec<_>>>()?;
        expected.sort();
        actual.sort();
        ensure!(
            actual == expected,
            "warehouse evidence verification failed: {table}"
        );
    }
    Ok(())
}
