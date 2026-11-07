//! BSC live writer and recovery operations over the common publication fence.
use super::{
    model::{CanonicalBlock, SourceEvidence},
    transfers::ExtractedBlockEvidence,
};
use crate::{
    BSC_NETWORK_ID,
    config::ClickHouseConfig,
    db::{Client, database_client, now_ms, row},
    semantic::{
        ProtocolContract, ProtocolImportResult, ProtocolRegistryInput, RegistryValidationError,
        SemanticBlockEvidence,
    },
};
use aml_warehouse::{
    Network,
    publication::{self, Block},
};
use anyhow::Context;
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
#[derive(Clone)]
pub struct ClickHouseIngestionStore {
    client: Client,
    database: String,
    #[cfg(test)]
    fail_point: Arc<AtomicU8>,
}
impl ClickHouseIngestionStore {
    pub fn new(config: &ClickHouseConfig) -> Self {
        Self {
            client: database_client(config),
            database: config.database().into(),
            #[cfg(test)]
            fail_point: Arc::new(AtomicU8::new(0)),
        }
    }
    #[cfg(test)]
    pub(crate) fn for_test(client: clickhouse::Client, database: impl Into<String>) -> Self {
        let database = database.into();
        Self {
            client: Client::from_inner(client, &database),
            database,
            fail_point: Arc::new(AtomicU8::new(0)),
        }
    }
    #[cfg(test)]
    pub(crate) fn inject_failure_once(&self, point: TestFailPoint) {
        self.fail_point.store(point as u8, Ordering::SeqCst);
    }
    pub(crate) fn ingestion_lock(&self) -> Result<std::fs::File, StoreError> {
        aml_warehouse::contract::identifier(&self.database)?;
        let root = std::env::var_os("BSC_WRITER_LOCK_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("aml-bsc-writer"));
        std::fs::create_dir_all(&root).context("create BSC writer lock directory")?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(format!("{}.lock", self.database)))
            .context("open BSC writer lock")?;
        lock.try_lock()
            .context("another BSC ingestor owns this database cursor")?;
        Ok(lock)
    }
    pub async fn protocol_contracts(&self) -> Result<Vec<ProtocolContract>, StoreError> {
        self.client.query("SELECT network_id,contract_address,protocol,protocol_type,contract_role,decoder,remote_network_id,remote_contract_address,method_ids,method_event_types,event_topics,event_types,remote_receiver_topic_index,message_topic_index,source_id,source_reference,review_status,evidence_confidence,enabled,registry_revision,reviewed_by,review_note,created_at_unix_ms FROM bsc_protocols WHERE network_id=? ORDER BY contract_address").bind(BSC_NETWORK_ID).fetch_all().await.map_err(|source|StoreError::ClickHouse{operation:"read common protocol registry",source})
    }
    pub async fn import_protocol_contract(
        &self,
        input: ProtocolRegistryInput,
    ) -> Result<ProtocolImportResult, StoreError> {
        let record = input.validate()?;
        let revision = crate::db::records::protocol(
            &self.client,
            &serde_json::to_value(&record).map_err(anyhow::Error::from)?,
        )
        .await?;
        let active=self.client.query("SELECT count() FROM aml_protocol_registry_active WHERE network_id=? AND subject_id=? AND revision=?")
            .bind(BSC_NETWORK_ID).bind(&record.contract_address).bind(revision).fetch_one::<u64>().await
            .map_err(|source|StoreError::ClickHouse{operation:"verify active protocol publication",source})?==1;
        Ok(ProtocolImportResult {
            contract_address: record.contract_address,
            registry_revision: revision,
            active,
            review_status: record.review_status,
        })
    }
    pub(crate) async fn commit_block(
        &self,
        block: &CanonicalBlock,
        evidence: &ExtractedBlockEvidence,
        semantic: &SemanticBlockEvidence,
        rpc_provider: &str,
        rpc_client_version: &str,
        source: &SourceEvidence,
        observed: u64,
    ) -> Result<BlockCommit, StoreError> {
        let previous = publication::state(&self.client, &self.database, Network::Bsc).await?;
        let revision = publication::next_revision(
            &self.client,
            &self.database,
            Network::Bsc,
            previous.revision,
        )
        .await?;
        let header = Block {
            network_id: BSC_NETWORK_ID.into(),
            block_number: block.number,
            block_hash: block.hash.clone(),
            parent_hash: block.parent_hash.clone(),
            block_timestamp_unix_ms: block.timestamp_unix_ms,
            commit_id: format!("{revision}-{}", block.hash),
            revision,
            canonical: 1,
            ingestion_status: "complete".into(),
            transaction_count: count(block.transactions.len())?,
            receipt_coverage: Some(1),
            trace_coverage: Some(u8::from(evidence.trace_data_complete)),
            prevout_coverage: None,
            source_id: rpc_provider.into(),
            created_at_unix_ms: now_ms(),
        };
        let facts = super::mapping::evidence(&header, block, evidence, semantic, source)?;
        let original =
            json!({"bsc_block":source.block,"rpc_client_version":rpc_client_version}).to_string();
        let writer = CrashBoundary {
            inner: &self.client,
            #[cfg(test)]
            point: &self.fail_point,
        };
        if previous.initialized == 1 && block.number <= previous.last_block_number {
            aml_warehouse::core::refresh_observed_batch(
                &writer,
                &self.database,
                &previous,
                &header,
                &facts,
                &original,
                observed,
            )
            .await?;
        } else {
            aml_warehouse::core::publish_observed_batch(
                &writer,
                &self.database,
                &previous,
                &header,
                &facts,
                &original,
                observed,
            )
            .await?;
        }
        Ok(BlockCommit {
            block_number: block.number,
            block_hash: block.hash.clone(),
            state_revision: revision,
            transaction_count: header.transaction_count,
            log_count: count(block.logs.len())?,
            relationship_count: count(evidence.relationships.len())?,
            feature_count: count(semantic.transaction_features.len())?,
            semantic_event_count: count(semantic.events.len())?,
            trace_data_complete: evidence.trace_data_complete,
        })
    }
    pub(crate) async fn canonical_block(
        &self,
        height: u64,
    ) -> Result<Option<StoredBlock>, StoreError> {
        Ok(self.client.query("SELECT block_number,block_hash,parent_hash,block_timestamp_unix_ms,transaction_count,log_count,receipt_data_complete,trace_data_complete,rpc_provider,rpc_client_version,current_revision FROM bsc_blocks WHERE network_id=? AND block_number=?").bind(BSC_NETWORK_ID).bind(height).fetch_optional().await.map_err(|source|StoreError::ClickHouse{operation:"read common block marker",source})?)
    }
    pub(crate) async fn observed_start(&self) -> Result<u64, StoreError> {
        Ok(
            publication::state(&self.client, &self.database, Network::Bsc)
                .await?
                .start_block,
        )
    }
    pub(crate) async fn checkpoint(&self) -> Result<Option<SyncCheckpoint>, StoreError> {
        let state = publication::state(&self.client, &self.database, Network::Bsc).await?;
        if state.initialized == 0 {
            // A published whole-range rollback retains the original start so a
            // restart can refill that range without requiring genesis or a flag.
            return Ok((state.revision > 0).then_some(SyncCheckpoint {
                next_block: state.start_block,
                last_finalized_block: 0,
                last_finalized_block_hash: String::new(),
                state_revision: state.revision,
            }));
        }
        Ok(Some(SyncCheckpoint {
            next_block: state
                .last_block_number
                .checked_add(1)
                .ok_or(StoreError::BlockNumberOverflow)?,
            last_finalized_block: state.last_block_number,
            last_finalized_block_hash: state.last_block_hash,
            state_revision: state.revision,
        }))
    }
    // Publication is atomic with fact verification. Callers cannot expose a marker
    // independently of the cursor; this compatibility method only verifies it.
    pub(crate) async fn advance_checkpoint(
        &self,
        height: u64,
        hash: &str,
    ) -> Result<SyncCheckpoint, StoreError> {
        let state = self
            .checkpoint()
            .await?
            .context("cannot advance an unpublished block")?;
        let block = self
            .canonical_block(height)
            .await?
            .context("published block missing")?;
        (state.last_finalized_block >= height && block.block_hash == hash)
            .then_some(())
            .context("cursor does not cover the requested canonical block")?;
        Ok(state)
    }
    pub(crate) async fn rewind_checkpoint(
        &self,
        next_block: u64,
        ancestor: Option<&StoredBlock>,
    ) -> Result<SyncCheckpoint, StoreError> {
        let previous = publication::state(&self.client, &self.database, Network::Bsc).await?;
        let mut next = previous.clone();
        next.revision = publication::next_revision(
            &self.client,
            &self.database,
            Network::Bsc,
            previous.revision,
        )
        .await?;
        next.published_at_unix_ms = now_ms();
        if let Some(ancestor) = ancestor {
            (ancestor.block_number.checked_add(1) == Some(next_block))
                .then_some(())
                .context("invalid recovery ancestor")?;
            next.last_block_number = ancestor.block_number;
            next.last_block_hash = ancestor.block_hash.clone();
        } else {
            next.initialized = 0;
            next.start_block = next_block;
            next.last_block_number = 0;
            next.last_block_hash.clear();
        }
        // Shared rollback writes suffix tombstones above the old fence and only
        // then publishes the new state. Interrupted recovery retains the old view.
        publication::publish(&self.client, &self.database, Network::Bsc, &previous, &next).await?;
        Ok(SyncCheckpoint {
            next_block,
            last_finalized_block: next.last_block_number,
            last_finalized_block_hash: next.last_block_hash,
            state_revision: next.revision,
        })
    }
    async fn failure(&self, height: u64) -> Result<Option<Value>, StoreError> {
        Ok(self
            .client
            .rows(
                "aml_ingestion_failures",
                &format!("failure_id={}", publication::literal(&failure_id(height))),
            )
            .await?
            .pop())
    }
    pub(crate) async fn record_failure(
        &self,
        input: FailureInput<'_>,
        maximum: u32,
    ) -> Result<(), StoreError> {
        let old = self.failure(input.block_number).await?;
        let attempts = old
            .as_ref()
            .and_then(|v| v["attempts"].as_u64())
            .unwrap_or(0)
            .saturating_add(1);
        let retryable = input.retryable && attempts < u64::from(maximum);
        let now = now_ms();
        let value = row(
            "aml_ingestion_failures",
            json!({"failure_id":failure_id(input.block_number),"block_number":input.block_number,"block_hash":input.block_hash,"operation":input.stage,"status":if retryable{"retry"}else{"dead_letter"},"attempts":attempts,"error_code":input.error_class,"error_message":sanitize_summary(input.error_summary),"next_attempt_unix_ms":0,"retryable":u8::from(retryable),"first_failed_at_unix_ms":old.as_ref().and_then(|v|v["first_failed_at_unix_ms"].as_u64()).unwrap_or(now),"last_failed_at_unix_ms":now,"resolved_at_unix_ms":null}),
            self.client.revision("aml_ingestion_failures").await?,
            now,
        )?;
        self.client
            .insert_rows("aml_ingestion_failures", &[value])
            .await?;
        Ok(())
    }
    async fn set_failure(&self, height: u64, requeue: bool) -> Result<(), StoreError> {
        let Some(mut record) = self.failure(height).await? else {
            return Ok(());
        };
        if !requeue && record["status"] == "resolved" {
            return Ok(());
        }
        record["revision"] = json!(self.client.revision("aml_ingestion_failures").await?);
        record["status"] = json!(if requeue { "retry" } else { "resolved" });
        if requeue {
            record["retryable"] = json!(1);
            record["resolved_at_unix_ms"] = Value::Null;
        } else {
            record["resolved_at_unix_ms"] = json!(now_ms());
        }
        self.client
            .insert_rows("aml_ingestion_failures", &[record])
            .await?;
        Ok(())
    }
    pub(crate) async fn resolve_failure(&self, height: u64) -> Result<(), StoreError> {
        self.set_failure(height, false).await
    }
    pub(crate) async fn requeue_failure(&self, height: u64) -> Result<(), StoreError> {
        self.set_failure(height, true).await
    }
    pub(crate) async fn find_gaps(
        &self,
        start: u64,
        end: u64,
        trace: bool,
        limit: u64,
    ) -> Result<Vec<u64>, StoreError> {
        (start <= end && end - start < 10000 && limit <= 10000)
            .then_some(())
            .context("repair exceeds bounded common scope")?;
        let heights=self.client.query("SELECT block_number FROM bsc_blocks WHERE network_id=? AND block_number BETWEEN ? AND ? AND receipt_data_complete=1 AND (?=0 OR trace_data_complete=1)").bind(BSC_NETWORK_ID).bind(start).bind(end).bind(u8::from(trace)).fetch_all::<u64>().await.map_err(|source|StoreError::ClickHouse{operation:"find incomplete BSC blocks",source})?.into_iter().collect::<HashSet<_>>();
        Ok((start..=end)
            .filter(|h| !heights.contains(h))
            .take(limit as usize)
            .collect())
    }
    pub(crate) async fn pending_failure_blocks(
        &self,
        start: u64,
        end: u64,
        include_dead: bool,
        limit: u64,
    ) -> Result<Vec<u64>, StoreError> {
        Ok(self.client.query("SELECT block_number FROM aml_ingestion_failures_current WHERE network_id=? AND block_number BETWEEN ? AND ? AND (status IN ('retry','gap','repairing') OR (?=1 AND status='dead_letter')) ORDER BY block_number LIMIT ?").bind(BSC_NETWORK_ID).bind(start).bind(end).bind(u8::from(include_dead)).bind(limit).fetch_all().await.map_err(|source|StoreError::ClickHouse{operation:"read pending common failures",source})?)
    }
    pub(crate) async fn dead_failure_blocks(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<u64>, StoreError> {
        Ok(self.client.query("SELECT block_number FROM aml_ingestion_failures_current WHERE network_id=? AND block_number BETWEEN ? AND ? AND status='dead_letter'").bind(BSC_NETWORK_ID).bind(start).bind(end).fetch_all().await.map_err(|source|StoreError::ClickHouse{operation:"read common dead letters",source})?)
    }
    pub(crate) async fn storage_snapshot(&self) -> Result<StorageSnapshot, StoreError> {
        Ok(self.client.query("SELECT coalesce(sum(rows),0) AS rows,coalesce(sum(data_compressed_bytes),0) AS compressed_bytes,coalesce(sum(data_uncompressed_bytes),0) AS uncompressed_bytes FROM system.parts WHERE active AND database=? AND startsWith(table,'aml_')").bind(&self.database).fetch_one().await.map_err(|source|StoreError::ClickHouse{operation:"read common storage metrics",source})?)
    }
    pub(crate) async fn canonical_range_span(
        &self,
        start: u64,
        end: u64,
    ) -> Result<CanonicalRangeSpan, StoreError> {
        Ok(self.client.query("SELECT count() AS block_count,coalesce(min(block_timestamp_unix_ms),0) AS first_timestamp_unix_ms,coalesce(max(block_timestamp_unix_ms),0) AS last_timestamp_unix_ms FROM bsc_blocks WHERE network_id=? AND block_number BETWEEN ? AND ?").bind(BSC_NETWORK_ID).bind(start).bind(end).fetch_one().await.map_err(|source|StoreError::ClickHouse{operation:"read common benchmark range",source})?)
    }
    pub(crate) async fn insert_benchmark(&self, v: &BenchmarkRow) -> Result<(), StoreError> {
        let value = serde_json::to_value(v).map_err(anyhow::Error::from)?;
        let record = row(
            "aml_ingestion_benchmarks",
            json!({"benchmark_id":v.benchmark_id,"start_block":v.start_block,"end_block":v.end_block,"blocks":v.completed_blocks,"transactions":v.transaction_count,"rows_written":v.transaction_count+v.log_count+v.relationship_count+v.feature_count+v.semantic_event_count,"elapsed_ms":v.elapsed_ms,"bytes_written":v.compressed_bytes,"settings_json":"{}","source_kind":"rpc","status":"complete","completed_at_unix_ms":now_ms(),"metrics_json":value.to_string(),"error_message":"","raw_evidence_json":value.to_string(),"blocks_per_second":v.blocks_per_second,"rows_per_second":v.rows_per_second}),
            self.client.revision("aml_ingestion_benchmarks").await?,
            now_ms(),
        )?;
        self.client
            .insert_rows("aml_ingestion_benchmarks", &[record])
            .await?;
        Ok(())
    }
}
fn count(value: usize) -> Result<u32, StoreError> {
    u32::try_from(value).map_err(|_| StoreError::CountOverflow {
        field: "fact count",
    })
}
struct CrashBoundary<'a> {
    inner: &'a Client,
    #[cfg(test)]
    point: &'a AtomicU8,
}
impl aml_warehouse::Connection for CrashBoundary<'_> {
    async fn execute(&self, sql: &str) -> anyhow::Result<()> {
        self.inner.execute(sql).await?;
        #[cfg(test)]
        let point = if sql.contains("INSERT INTO") {
            if sql.contains(".aml_transactions ") {
                Some(TestFailPoint::Transactions)
            } else if sql.contains(".aml_evm_logs ") {
                Some(TestFailPoint::Logs)
            } else if sql.contains(".aml_address_relationships ") {
                Some(TestFailPoint::Relationships)
            } else if sql.contains(".aml_metadata_discoveries ") {
                Some(TestFailPoint::TokenDiscoveries)
            } else if sql.contains(".aml_transaction_features ")
                || sql.contains(".aml_semantic_aml_events ")
            {
                Some(TestFailPoint::SemanticEvidence)
            } else if sql.contains(".aml_ingested_blocks ")
                && sql.contains("\"ingestion_status\":\"complete\"")
            {
                Some(TestFailPoint::CompleteMarker)
            } else {
                None
            }
        } else {
            None
        };
        #[cfg(test)]
        if let Some(point) = point {
            if self
                .point
                .compare_exchange(point as u8, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Err(StoreError::InjectedFailure {
                    point: point.name(),
                }
                .into());
            }
        }
        Ok(())
    }
    async fn query(&self, sql: &str) -> anyhow::Result<Vec<String>> {
        aml_warehouse::Connection::query(self.inner, sql).await
    }
}

fn failure_id(block_number: u64) -> String {
    format!("{BSC_NETWORK_ID}:canonical_ingestion:{block_number}")
}

fn sanitize_summary(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(320)
        .collect()
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum TestFailPoint {
    Transactions = 1,
    Logs = 2,
    Relationships = 3,
    TokenDiscoveries = 4,
    SemanticEvidence = 5,
    CompleteMarker = 6,
}

#[cfg(test)]
impl TestFailPoint {
    #[cfg(test)]
    fn name(self) -> &'static str {
        match self {
            Self::Transactions => "after_transactions",
            Self::Logs => "after_logs",
            Self::Relationships => "after_relationships",
            Self::TokenDiscoveries => "after_token_discoveries",
            Self::SemanticEvidence => "after_semantic_evidence",
            Self::CompleteMarker => "after_block_marker",
        }
    }
}

pub(crate) struct FailureInput<'a> {
    pub block_number: u64,
    pub block_hash: Option<&'a str>,
    pub stage: &'static str,
    pub error_class: &'static str,
    pub error_summary: &'a str,
    pub retryable: bool,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct StoredBlock {
    pub block_number: u64,
    pub block_hash: String,
    pub parent_hash: String,
    pub block_timestamp_unix_ms: u64,
    pub transaction_count: u32,
    pub log_count: u32,
    pub receipt_data_complete: u8,
    pub trace_data_complete: u8,
    pub rpc_provider: String,
    pub rpc_client_version: String,
    pub current_revision: u64,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct SyncCheckpoint {
    pub next_block: u64,
    pub last_finalized_block: u64,
    pub last_finalized_block_hash: String,
    pub state_revision: u64,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct StorageSnapshot {
    pub rows: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct CanonicalRangeSpan {
    pub block_count: u64,
    pub first_timestamp_unix_ms: u64,
    pub last_timestamp_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Row)]
pub(crate) struct BenchmarkRow {
    pub benchmark_id: String,
    pub network_id: String,
    pub start_block: u64,
    pub end_block: u64,
    pub completed_blocks: u64,
    pub transaction_count: u64,
    pub log_count: u64,
    pub relationship_count: u64,
    pub feature_count: u64,
    pub semantic_event_count: u64,
    pub elapsed_ms: u64,
    pub blocks_per_second: f64,
    pub observed_live_blocks_per_second: f64,
    pub live_rate_multiple: f64,
    pub rows_per_second: f64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlockCommit {
    pub block_number: u64,
    pub block_hash: String,
    pub state_revision: u64,
    pub transaction_count: u32,
    pub log_count: u32,
    pub relationship_count: u32,
    pub feature_count: u32,
    pub semantic_event_count: u32,
    pub trace_data_complete: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Shared(#[from] anyhow::Error),
    #[error(transparent)]
    RegistryValidation(#[from] RegistryValidationError),
    #[error("ClickHouse failed to {operation}")]
    ClickHouse {
        operation: &'static str,
        #[source]
        source: clickhouse::error::Error,
    },
    #[error("{field} exceeds its storage type")]
    CountOverflow { field: &'static str },
    #[error("block state revision overflow at block {block_number}")]
    StateRevisionOverflow { block_number: u64 },
    #[error("protocol registry revision overflow for {contract_address}")]
    RegistryRevisionOverflow { contract_address: String },
    #[error("block number overflow while advancing the sync cursor")]
    BlockNumberOverflow,
    #[error("sync state revision overflow")]
    SyncStateRevisionOverflow,
    #[error(
        "sync cursor expected block {expected_block}, but block {attempted_block} was committed"
    )]
    CursorGap {
        expected_block: u64,
        attempted_block: u64,
    },
    #[error("sync cursor at {current_next} cannot move backward through block {attempted_block}")]
    CursorRegression {
        current_next: u64,
        attempted_block: u64,
    },
    #[cfg(test)]
    #[error("injected ingestion crash boundary {point}")]
    InjectedFailure { point: &'static str },
    #[error("system clock is before Unix epoch")]
    Clock(#[source] std::time::SystemTimeError),
    #[error("system clock timestamp exceeds UInt64 milliseconds")]
    ClockOverflow,
}
