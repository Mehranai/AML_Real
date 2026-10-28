//! Durable operational state in the existing shared failure contract.
use crate::{
    config,
    db::{Database, SyncState, timestamp},
    model,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
pub struct Policy {
    pub max_attempts: u32,
    pub retry_seconds: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            retry_seconds: 1,
        }
    }
}
impl Policy {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            max_attempts: config::bounded("BITCOIN_FAILURE_MAX_ATTEMPTS", 10, 1, 1000)? as u32,
            retry_seconds: config::bounded("BITCOIN_FAILURE_RETRY_SECONDS", 1, 1, 3600)?,
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=1000).contains(&self.max_attempts) && (1..=3600).contains(&self.retry_seconds),
            "invalid failure retry policy"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub network_id: String,
    pub failure_id: String,
    pub block_number: u64,
    pub block_hash: Option<String>,
    pub operation: String,
    pub status: String,
    pub attempts: u32,
    pub error_code: String,
    pub error_message: String,
    pub next_attempt_unix_ms: u64,
    pub revision: u64,
    pub created_at_unix_ms: u64,
    pub tx_hash: Option<String>,
    pub retryable: Option<u8>,
    pub first_failed_at_unix_ms: Option<u64>,
    pub last_failed_at_unix_ms: Option<u64>,
    pub resolved_at_unix_ms: Option<u64>,
    pub raw_evidence_json: String,
}
#[derive(Clone, Copy)]
pub enum Stage {
    Source,
    Fetch,
    Resolve,
    Commit,
    Reconcile,
    Gap,
}
impl Stage {
    fn description(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Source => (
                "source",
                "rpc_source_unavailable",
                "RPC identity, height or checkpoint verification failed at the next unpublished block.",
            ),
            Self::Fetch => (
                "fetch_window",
                "block_window_unavailable",
                "Fetching or validating the bounded block window failed; this is the blocked frontier, not an inferred bad block.",
            ),
            Self::Resolve => (
                "resolve",
                "block_evidence_unavailable",
                "Prevout resolution, block continuity or evidence validation failed; no facts for this block were published.",
            ),
            Self::Commit => (
                "commit",
                "publication_uncertain",
                "A database write or publication check failed; inspect canonical state before retrying.",
            ),
            Self::Reconcile => (
                "reconcile",
                "reorg_reconciliation_failed",
                "Primary chain reconciliation failed; operator recovery may be required.",
            ),
            Self::Gap => (
                "gap",
                "historical_gap",
                "Published block continuity or transaction count is incomplete; independent replay is required.",
            ),
        }
    }
}
pub fn id(height: u64) -> String {
    format!("bitcoin:block:{height}")
}
pub fn height_from_id(value: &str) -> Result<u64> {
    let height = value
        .strip_prefix("bitcoin:block:")
        .context("invalid Bitcoin failure id")?
        .parse::<u64>()?;
    ensure!(
        height <= u32::MAX as u64 && value == id(height),
        "invalid Bitcoin failure id"
    );
    Ok(height)
}
fn revision(previous: u64) -> Result<u64> {
    Ok(previous
        .checked_add(1)
        .context("failure revision overflow")?
        .max(
            timestamp()
                .checked_mul(1000)
                .context("failure clock overflow")?,
        ))
}
fn increment(value: u32) -> Result<u32> {
    value
        .checked_add(1)
        .context("failure attempt counter overflow")
}
async fn save(db: &Database, rows: &[Record]) -> Result<()> {
    let values = rows
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    aml_warehouse::publication::insert(
        &db.warehouse_connection(),
        db.database(),
        "aml_ingestion_failures",
        &values,
    )
    .await
}
pub async fn current(db: &Database, height: u64) -> Result<Option<Record>> {
    ensure!(height <= u32::MAX as u64, "failure height out of range");
    Ok(db.rows("SELECT * FROM aml_ingestion_failures_current WHERE network_id={network:String} AND failure_id={id:String}",&[("network",crate::NETWORK_ID.into()),("id",id(height))]).await?.pop())
}
pub async fn record(
    db: &Database,
    height: u64,
    hash: Option<&str>,
    stage: Stage,
    policy: &Policy,
    count_attempt: bool,
) -> Result<Record> {
    policy.validate()?;
    let old = current(db, height).await?;
    let (operation, code, message) = stage.description();
    let attempts = if count_attempt {
        increment(old.as_ref().map_or(0, |r| r.attempts))?
    } else {
        old.as_ref()
            .context("repair attempt was not reserved")?
            .attempts
    };
    let now = timestamp();
    let row = Record {
        network_id: crate::NETWORK_ID.into(),
        failure_id: id(height),
        block_number: height,
        block_hash: hash.map(model::hash).transpose()?,
        operation: operation.into(),
        status: if attempts >= policy.max_attempts {
            "dead_letter"
        } else {
            "retry"
        }
        .into(),
        attempts,
        error_code: code.into(),
        error_message: message.into(),
        next_attempt_unix_ms: if attempts >= policy.max_attempts {
            0
        } else {
            now + policy.retry_seconds * 1000
        },
        revision: revision(old.as_ref().map_or(0, |r| r.revision))?,
        created_at_unix_ms: old.as_ref().map_or(now, |r| r.created_at_unix_ms),
        tx_hash: old.as_ref().and_then(|r| r.tx_hash.clone()),
        retryable: Some(u8::from(attempts < policy.max_attempts)),
        first_failed_at_unix_ms: old
            .as_ref()
            .map_or(Some(now), |r| r.first_failed_at_unix_ms),
        last_failed_at_unix_ms: Some(
            now.max(
                old.as_ref()
                    .and_then(|r| r.last_failed_at_unix_ms)
                    .unwrap_or(0),
            ),
        ),
        resolved_at_unix_ms: None,
        raw_evidence_json: serde_json::to_string(
            &json!({"operation":operation,"error_code":code,"attempts":attempts,"max_attempts":policy.max_attempts}),
        )?,
    };
    save(db, std::slice::from_ref(&row)).await?;
    Ok(row)
}
pub async fn resolved(db: &Database, height: u64, hash: &str, count_attempt: bool) -> Result<()> {
    let Some(mut row) = current(db, height).await? else {
        return Ok(());
    };
    if row.status == "resolved" {
        return Ok(());
    }
    row.block_hash = Some(model::hash(hash)?);
    row.status = "resolved".into();
    row.resolved_at_unix_ms = Some(timestamp().max(row.last_failed_at_unix_ms.unwrap_or(0)));
    row.retryable = Some(0);
    row.next_attempt_unix_ms = 0;
    if count_attempt {
        row.attempts = increment(row.attempts)?;
    }
    row.revision = revision(row.revision)?;
    save(db, &[row]).await
}
pub async fn begin_repair(db: &Database, height: u64) -> Result<Record> {
    let mut row = current(db, height)
        .await?
        .context("selected failure does not exist")?;
    ensure!(
        row.status != "resolved",
        "selected failure is already resolved"
    );
    row.status = "repairing".into();
    row.resolved_at_unix_ms = None;
    row.attempts = increment(row.attempts)?;
    row.next_attempt_unix_ms = 0;
    row.revision = revision(row.revision)?;
    save(db, std::slice::from_ref(&row)).await?;
    Ok(row)
}
pub async fn mark_gap(db: &Database, height: u64, hash: Option<String>) -> Result<()> {
    let old = current(db, height).await?;
    let (_, code, message) = Stage::Gap.description();
    let now = timestamp();
    let row = Record {
        network_id: crate::NETWORK_ID.into(),
        failure_id: id(height),
        block_number: height,
        block_hash: hash,
        operation: "gap".into(),
        status: "gap".into(),
        attempts: old.as_ref().map_or(0, |r| r.attempts),
        error_code: code.into(),
        error_message: message.into(),
        next_attempt_unix_ms: 0,
        revision: revision(old.as_ref().map_or(0, |r| r.revision))?,
        created_at_unix_ms: old.as_ref().map_or(now, |r| r.created_at_unix_ms),
        tx_hash: old.as_ref().and_then(|r| r.tx_hash.clone()),
        retryable: Some(0),
        first_failed_at_unix_ms: old
            .as_ref()
            .map_or(Some(now), |r| r.first_failed_at_unix_ms),
        last_failed_at_unix_ms: Some(
            now.max(
                old.as_ref()
                    .and_then(|r| r.last_failed_at_unix_ms)
                    .unwrap_or(0),
            ),
        ),
        resolved_at_unix_ms: None,
        raw_evidence_json: serde_json::to_string(
            &json!({"operation":"gap","replay_required":true}),
        )?,
    };
    save(db, &[row]).await
}
pub async fn gate(
    db: &Database,
    height: u64,
    policy: &Policy,
    once: bool,
) -> Result<Option<&'static str>> {
    let Some(mut row) = current(db, height).await? else {
        return Ok(None);
    };
    if row.status == "resolved" {
        return Ok(None);
    }
    if row.status == "gap" {
        return Ok(Some("historical_gap"));
    }
    if row.status == "dead_letter" {
        return Ok(Some("dead_letter"));
    }
    if row.attempts >= policy.max_attempts {
        row.status = "dead_letter".into();
        row.next_attempt_unix_ms = 0;
        row.revision = revision(row.revision)?;
        save(db, &[row]).await?;
        return Ok(Some("dead_letter"));
    }
    if !once && row.next_attempt_unix_ms > timestamp() {
        return Ok(Some("retry_not_due"));
    }
    Ok(None)
}
pub async fn recover_interrupted(db: &Database, policy: &Policy) -> Result<()> {
    let mut rows:Vec<Record>=db.rows("SELECT * FROM aml_ingestion_failures_current WHERE network_id={network:String} AND status='repairing' ORDER BY block_number LIMIT 1000",&[("network",crate::NETWORK_ID.into())]).await?;
    for row in &mut rows {
        row.status = if row.attempts >= policy.max_attempts {
            "dead_letter"
        } else {
            "retry"
        }
        .into();
        row.error_code = "interrupted_repair".into();
        row.retryable = Some(u8::from(row.attempts < policy.max_attempts));
        row.last_failed_at_unix_ms = Some(timestamp().max(row.last_failed_at_unix_ms.unwrap_or(0)));
        row.resolved_at_unix_ms = None;
        row.raw_evidence_json = serde_json::to_string(
            &json!({"operation":"repair","error_code":"interrupted_repair","attempts":row.attempts,"max_attempts":policy.max_attempts}),
        )?;
        row.error_message="Previous repair ended before its outcome was recorded; canonical state must be checked.".into();
        row.next_attempt_unix_ms = timestamp();
        row.revision = revision(row.revision)?;
    }
    save(db, &rows).await
}
pub async fn settle_published(db: &Database, state: &SyncState) -> Result<()> {
    if state.initialized == 0 {
        return Ok(());
    }
    let rows:Vec<Record>=db.rows("SELECT * FROM aml_ingestion_failures_current WHERE network_id={network:String} AND status IN ('retry','dead_letter','repairing') AND block_number BETWEEN {start:UInt64} AND {tip:UInt64} ORDER BY block_number DESC LIMIT 64",&[("network",crate::NETWORK_ID.into()),("start",state.start_height.to_string()),("tip",state.tip_height.to_string())]).await?;
    for row in rows {
        if let Some(hash) = db.block_hash(row.block_number).await? {
            resolved(db, row.block_number, &hash, false).await?;
        }
    }
    Ok(())
}

#[derive(Clone, Default, Deserialize)]
pub struct Query {
    pub status: Option<String>,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub after_block: Option<u64>,
    pub limit: Option<usize>,
}
impl Query {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.limit.is_none_or(|n| (1..=200).contains(&n)),
            "failure limit must be 1..200"
        );
        ensure!(
            self.status.as_deref().is_none_or(|s| matches!(
                s,
                "retry" | "dead_letter" | "repairing" | "gap" | "resolved"
            )),
            "invalid failure status"
        );
        ensure!(
            self.from_block.unwrap_or(0) <= self.to_block.unwrap_or(u32::MAX as u64)
                && [self.from_block, self.to_block, self.after_block]
                    .into_iter()
                    .flatten()
                    .all(|n| n <= u32::MAX as u64),
            "invalid failure block range"
        );
        Ok(())
    }
}
pub async fn list(db: &Database, query: &Query) -> Result<Value> {
    query.validate()?;
    let limit = query.limit.unwrap_or(50);
    let mut params = vec![
        ("network", crate::NETWORK_ID.into()),
        ("from", query.from_block.unwrap_or(0).to_string()),
        ("to", query.to_block.unwrap_or(u32::MAX as u64).to_string()),
        ("limit", (limit + 1).to_string()),
    ];
    let mut filter = String::new();
    if let Some(status) = &query.status {
        filter.push_str(" AND status={status:String}");
        params.push(("status", status.clone()));
    }
    if let Some(after) = query.after_block {
        filter.push_str(" AND block_number>{after:UInt64}");
        params.push(("after", after.to_string()));
    }
    let mut rows:Vec<Record>=db.rows(&format!("SELECT * FROM aml_ingestion_failures_current WHERE network_id={{network:String}} AND block_number BETWEEN {{from:UInt64}} AND {{to:UInt64}}{filter} ORDER BY block_number LIMIT {{limit:UInt64}}"),&params).await?;
    let truncated = rows.len() > limit;
    rows.truncate(limit);
    let next = if truncated {
        rows.last().map(|r| r.block_number)
    } else {
        None
    };
    Ok(
        json!({"network_id":crate::NETWORK_ID,"failures":rows,"truncated":truncated,"next_after_block":next,"scope":"latest_persisted_failure_state"}),
    )
}
pub async fn summary(db: &Database, state: &SyncState) -> Result<Value> {
    let rows:Vec<Value>=db.rows("SELECT status,count() AS n FROM aml_ingestion_failures_current WHERE network_id={network:String} GROUP BY status",&[("network",crate::NETWORK_ID.into())]).await?;
    let mut counts: BTreeMap<String, u64> =
        ["retry", "dead_letter", "repairing", "gap", "resolved"]
            .into_iter()
            .map(|s| (s.into(), 0))
            .collect();
    for row in rows {
        counts.insert(
            row["status"]
                .as_str()
                .context("invalid failure status")?
                .into(),
            row["n"].as_u64().context("invalid failure count")?,
        );
    }
    let next = if state.initialized == 1 {
        state.tip_height + 1
    } else {
        state.start_height
    };
    let frontier = if state.initialized == 0 && state.revision == 0 {
        db.rows::<Record>("SELECT * FROM aml_ingestion_failures_current WHERE network_id={network:String} AND status!='resolved' ORDER BY block_number LIMIT 1",&[("network",crate::NETWORK_ID.into())]).await?.pop()
    } else if next <= u32::MAX as u64 {
        current(db, next).await?.filter(|r| r.status != "resolved")
    } else {
        None
    };
    Ok(
        json!({"counts":counts,"next_failure":frontier,"scope":"latest_persisted_failure_state","historical_gaps_require_replay":true}),
    )
}

#[derive(Deserialize)]
struct BlockRow {
    block_number: u64,
    block_hash: String,
    parent_hash: String,
    transaction_count: u32,
}
pub async fn healthy(db: &Database, height: u64, state: &SyncState) -> Result<bool> {
    if state.initialized == 0 || height < state.start_height || height > state.tip_height {
        return Ok(false);
    }
    let rows:Vec<BlockRow>=db.rows("SELECT block_number,block_hash,parent_hash,transaction_count FROM aml_ingested_blocks_canonical WHERE network_id={network:String} AND block_number={height:UInt64}",&[("network",crate::NETWORK_ID.into()),("height",height.to_string())]).await?;
    let Some(row) = rows.first() else {
        return Ok(false);
    };
    let count:Vec<Value>=db.rows("SELECT count() AS n FROM aml_transactions_canonical WHERE network_id={network:String} AND block_number={height:UInt64} AND block_hash={hash:String}",&[("network",crate::NETWORK_ID.into()),("height",height.to_string()),("hash",row.block_hash.clone())]).await?;
    if count[0]["n"].as_u64() != Some(row.transaction_count as u64) {
        return Ok(false);
    }
    if height > state.start_height
        && db.block_hash(height - 1).await?.as_deref() != Some(&row.parent_hash)
    {
        return Ok(false);
    }
    Ok(height != state.tip_height || state.tip_hash == row.block_hash)
}
/// Caller owns the writer lock, so scan and append cannot race a supported writer.
pub async fn scan_gaps(db: &Database, from: u64, to: u64, max_blocks: u64) -> Result<Value> {
    ensure!(
        (1..=10000).contains(&max_blocks)
            && from <= to
            && to <= u32::MAX as u64
            && to - from < max_blocks,
        "gap scan must contain 1..max-blocks heights, max-blocks <= 10000"
    );
    let state = db.state().await?;
    ensure!(
        state.initialized == 1 && from >= state.start_height && to <= state.tip_height,
        "gap scan must stay inside published history"
    );
    let params = [
        ("network", crate::NETWORK_ID.into()),
        ("from", from.to_string()),
        ("to", to.to_string()),
    ];
    let blocks:Vec<BlockRow>=db.rows("SELECT block_number,block_hash,parent_hash,transaction_count FROM aml_ingested_blocks_canonical WHERE network_id={network:String} AND block_number BETWEEN {from:UInt64} AND {to:UInt64} ORDER BY block_number",&params).await?;
    let counts:Vec<Value>=db.rows("SELECT block_number,count() AS n FROM aml_transactions_canonical WHERE network_id={network:String} AND block_number BETWEEN {from:UInt64} AND {to:UInt64} GROUP BY block_number",&params).await?;
    let old:Vec<Record>=db.rows("SELECT * FROM aml_ingestion_failures_current WHERE network_id={network:String} AND block_number BETWEEN {from:UInt64} AND {to:UInt64}",&params).await?;
    let blocks: HashMap<_, _> = blocks.into_iter().map(|r| (r.block_number, r)).collect();
    let counts: HashMap<_, _> = counts
        .into_iter()
        .map(|r| {
            (
                r["block_number"].as_u64().unwrap(),
                r["n"].as_u64().unwrap(),
            )
        })
        .collect();
    let old: HashMap<_, _> = old.into_iter().map(|r| (r.block_number, r)).collect();
    let mut parent = if from > state.start_height {
        db.block_hash(from - 1).await?
    } else {
        None
    };
    let mut writes = Vec::new();
    let mut ids = Vec::new();
    let (_, code, message) = Stage::Gap.description();
    for height in from..=to {
        let block = blocks.get(&height);
        let healthy = block.is_some_and(|b| {
            counts.get(&height).copied() == Some(b.transaction_count as u64)
                && parent.as_ref().is_none_or(|hash| hash == &b.parent_hash)
                && (height != state.tip_height || state.tip_hash == b.block_hash)
        });
        parent = block.map(|b| b.block_hash.clone());
        if !healthy {
            let previous = old.get(&height);
            ids.push(id(height));
            writes.push(Record {
                network_id: crate::NETWORK_ID.into(),
                failure_id: id(height),
                block_number: height,
                block_hash: block.map(|b| b.block_hash.clone()),
                operation: "gap".into(),
                status: "gap".into(),
                attempts: previous.map_or(0, |r| r.attempts),
                error_code: code.into(),
                error_message: message.into(),
                next_attempt_unix_ms: 0,
                revision: revision(previous.map_or(0, |r| r.revision))?,
                created_at_unix_ms: previous.map_or(timestamp(), |r| r.created_at_unix_ms),
                tx_hash: previous.and_then(|r| r.tx_hash.clone()),
                retryable: Some(0),
                first_failed_at_unix_ms: previous
                    .map_or(Some(timestamp()), |r| r.first_failed_at_unix_ms),
                last_failed_at_unix_ms: Some(
                    timestamp().max(previous.and_then(|r| r.last_failed_at_unix_ms).unwrap_or(0)),
                ),
                resolved_at_unix_ms: None,
                raw_evidence_json: serde_json::to_string(
                    &json!({"operation":"gap","replay_required":true}),
                )?,
            });
        }
    }
    ensure!(
        db.state().await?.revision == state.revision,
        "checkpoint changed during gap scan; retry"
    );
    save(db, &writes).await?;
    let total = ids.len();
    ids.truncate(200);
    Ok(
        json!({"network_id":crate::NETWORK_ID,"from_block":from,"to_block":to,"scanned_blocks":to-from+1,"gaps_found":total,"failure_ids":ids,"truncated":total>200,"checkpoint_changed":false,"historical_gaps_require_replay":true}),
    )
}
