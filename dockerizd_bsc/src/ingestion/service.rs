use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::{sync::Mutex, task::JoinSet, time::sleep};

use crate::{
    BSC_NETWORK_ID,
    config::{AppConfig, IngestionConfig},
    semantic::{SemanticRegistry, classify_block},
};

use super::{
    model::{CanonicalBlock, DecodeError, normalize_block},
    rpc::{CanonicalRpcClient, RpcFetchError},
    store::{
        BenchmarkRow, BlockCommit, ClickHouseIngestionStore, FailureInput, StoreError, StoredBlock,
    },
    transfers::{ExtractedBlockEvidence, TransferDecodeError, extract_block_evidence},
};

pub struct CanonicalIngestor {
    rpc: CanonicalRpcClient,
    store: ClickHouseIngestionStore,
    rpc_provider: String,
    rpc_client_version: String,
    settings: IngestionConfig,
    trace_required: bool,
    registry_cache: Mutex<RegistryCache>,
}

impl CanonicalIngestor {
    pub fn new(
        app_config: &AppConfig,
        ingestion_config: IngestionConfig,
        store: ClickHouseIngestionStore,
        rpc_client_version: String,
    ) -> Result<Self, IngestionError> {
        let rpc = CanonicalRpcClient::new(app_config, ingestion_config.clone())?;
        Ok(Self {
            rpc,
            store,
            rpc_provider: app_config.rpc_provider.clone(),
            rpc_client_version,
            settings: ingestion_config,
            trace_required: app_config.trace_mode.required(),
            registry_cache: Mutex::new(RegistryCache::default()),
        })
    }

    pub async fn finalized_height(&self) -> Result<u64, IngestionError> {
        Ok(self.rpc.finalized_height().await?)
    }

    pub async fn ingest_range(
        &self,
        start_block: u64,
        end_block: u64,
    ) -> Result<IngestionReport, IngestionError> {
        self.process_range(start_block, end_block, false, false, None)
            .await
    }

    pub async fn replay_range(
        &self,
        start_block: u64,
        end_block: u64,
    ) -> Result<IngestionReport, IngestionError> {
        self.process_range(start_block, end_block, true, false, None)
            .await
    }

    pub async fn replay_hash(&self, block_hash: &str) -> Result<IngestionReport, IngestionError> {
        let identity = self.rpc.block_identity_by_hash(block_hash).await?;
        self.process_range(
            identity.number,
            identity.number,
            true,
            false,
            Some(&identity.hash),
        )
        .await
    }

    pub async fn sync_once(
        &self,
        initial_start: Option<u64>,
        maximum_blocks: Option<u64>,
    ) -> Result<SyncBatchReport, IngestionError> {
        let finalized_height = self.rpc.finalized_height().await?;
        let mut reorg_repaired_from = self.verify_checkpoint().await?;
        let checkpoint = self.store.checkpoint().await?;
        let start_block = checkpoint
            .as_ref()
            .map(|value| value.next_block)
            .or(initial_start)
            .or(self.settings.start_block)
            .ok_or(IngestionError::MissingStartBlock)?;
        if start_block > finalized_height {
            return Ok(SyncBatchReport {
                finalized_height,
                next_block: start_block,
                caught_up: true,
                reorg_repaired_from,
                batch: None,
            });
        }
        let requested = maximum_blocks
            .unwrap_or(self.settings.batch_size)
            .min(self.settings.batch_size)
            .min(self.settings.max_blocks_per_run)
            .max(1);
        let end_block = start_block
            .saturating_add(requested.saturating_sub(1))
            .min(finalized_height);
        let batch = match self
            .process_range(start_block, end_block, false, true, None)
            .await
        {
            Ok(report) => report,
            Err(error) if error.conflict_height().is_some() => {
                let conflict_height = error.conflict_height().unwrap_or(start_block);
                let repair_from = self.repair_reorg(conflict_height).await?;
                reorg_repaired_from = Some(repair_from);
                let restart = self
                    .store
                    .checkpoint()
                    .await?
                    .map_or(repair_from, |value| value.next_block);
                let retry_end = restart
                    .saturating_add(requested.saturating_sub(1))
                    .min(finalized_height);
                self.process_range(restart, retry_end, false, true, None)
                    .await?
            }
            Err(error) => return Err(error),
        };
        let next_block = self
            .store
            .checkpoint()
            .await?
            .map_or(start_block, |value| value.next_block);
        Ok(SyncBatchReport {
            finalized_height,
            next_block,
            caught_up: next_block > finalized_height,
            reorg_repaired_from,
            batch: Some(batch),
        })
    }

    pub async fn follow(
        &self,
        initial_start: Option<u64>,
        maximum_blocks: Option<u64>,
    ) -> Result<FollowReport, IngestionError> {
        let mut completed_blocks = 0_u64;
        let mut batches = 0_u64;
        let mut start = initial_start;
        loop {
            let remaining = maximum_blocks.map(|maximum| maximum.saturating_sub(completed_blocks));
            if remaining == Some(0) {
                break;
            }
            let report = self.sync_once(start.take(), remaining).await?;
            if let Some(batch) = report.batch {
                completed_blocks = completed_blocks
                    .saturating_add(batch.completed_blocks)
                    .saturating_add(batch.skipped_blocks);
                batches = batches.saturating_add(1);
            }
            if maximum_blocks.is_some_and(|maximum| completed_blocks >= maximum) {
                break;
            }
            if report.caught_up {
                sleep(self.settings.poll_interval).await;
            }
        }
        let checkpoint = self.store.checkpoint().await?;
        Ok(FollowReport {
            batches,
            completed_blocks,
            next_block: checkpoint.map(|value| value.next_block),
        })
    }

    pub async fn repair_range(
        &self,
        start_block: u64,
        end_block: u64,
        include_dead: bool,
    ) -> Result<RepairReport, IngestionError> {
        let block_count = validated_block_count(start_block, end_block)?;
        if block_count > self.settings.repair_max_blocks {
            return Err(IngestionError::RepairRangeTooLarge {
                requested: block_count,
                maximum: self.settings.repair_max_blocks,
            });
        }
        let finalized_height = self.rpc.finalized_height().await?;
        if end_block > finalized_height {
            return Err(IngestionError::BeyondFinalized {
                requested_end: end_block,
                finalized_height,
            });
        }
        let mut candidates = self
            .store
            .find_gaps(
                start_block,
                end_block,
                self.trace_required,
                self.settings.repair_max_blocks,
            )
            .await?
            .into_iter()
            .collect::<BTreeSet<_>>();
        candidates.extend(
            self.store
                .pending_failure_blocks(
                    start_block,
                    end_block,
                    include_dead,
                    self.settings.repair_max_blocks,
                )
                .await?,
        );
        let dead = self
            .store
            .dead_failure_blocks(start_block, end_block)
            .await?;
        if include_dead {
            for block_number in candidates.iter().filter(|number| dead.contains(number)) {
                self.store.requeue_failure(*block_number).await?;
            }
        } else {
            candidates.retain(|number| !dead.contains(number));
        }
        let mut repaired_blocks = Vec::with_capacity(candidates.len());
        for block_number in candidates {
            self.process_range(block_number, block_number, true, false, None)
                .await?;
            repaired_blocks.push(block_number);
        }
        Ok(RepairReport {
            start_block,
            end_block,
            repaired_blocks,
            skipped_dead_blocks: dead.len() as u64,
        })
    }

    pub async fn benchmark_range(
        &self,
        start_block: u64,
        end_block: u64,
    ) -> Result<BenchmarkReport, IngestionError> {
        let before = self.store.storage_snapshot().await?;
        let started = Instant::now();
        let ingestion = self.replay_range(start_block, end_block).await?;
        let elapsed_ms = u64::try_from(started.elapsed().as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        let after = self.store.storage_snapshot().await?;
        let compressed_bytes = after
            .compressed_bytes
            .saturating_sub(before.compressed_bytes);
        let uncompressed_bytes = after
            .uncompressed_bytes
            .saturating_sub(before.uncompressed_bytes);
        let total_rows = ingestion
            .completed_blocks
            .saturating_add(ingestion.transaction_count)
            .saturating_add(ingestion.log_count)
            .saturating_add(ingestion.relationship_count);
        let total_rows = total_rows
            .saturating_add(ingestion.feature_count)
            .saturating_add(ingestion.semantic_event_count);
        let blocks_per_second = ingestion.completed_blocks as f64 * 1_000.0 / elapsed_ms as f64;
        let span = self
            .store
            .canonical_range_span(start_block, end_block)
            .await?;
        let chain_elapsed_ms = span
            .last_timestamp_unix_ms
            .saturating_sub(span.first_timestamp_unix_ms);
        let observed_live_blocks_per_second = if span.block_count < 2 || chain_elapsed_ms == 0 {
            0.0
        } else {
            span.block_count.saturating_sub(1) as f64 * 1_000.0 / chain_elapsed_ms as f64
        };
        let live_rate_multiple = if observed_live_blocks_per_second == 0.0 {
            0.0
        } else {
            blocks_per_second / observed_live_blocks_per_second
        };
        let rows_per_second = total_rows as f64 * 1_000.0 / elapsed_ms as f64;
        let benchmark_id = format!(
            "{BSC_NETWORK_ID}:{start_block}:{end_block}:{}:{}",
            std::process::id(),
            now_unix_ms()?
        );
        self.store
            .insert_benchmark(&BenchmarkRow {
                benchmark_id: benchmark_id.clone(),
                network_id: BSC_NETWORK_ID.to_string(),
                start_block,
                end_block,
                completed_blocks: ingestion.completed_blocks,
                transaction_count: ingestion.transaction_count,
                log_count: ingestion.log_count,
                relationship_count: ingestion.relationship_count,
                feature_count: ingestion.feature_count,
                semantic_event_count: ingestion.semantic_event_count,
                elapsed_ms,
                blocks_per_second,
                observed_live_blocks_per_second,
                live_rate_multiple,
                rows_per_second,
                compressed_bytes,
                uncompressed_bytes,
            })
            .await?;
        Ok(BenchmarkReport {
            benchmark_id,
            ingestion,
            elapsed_ms,
            blocks_per_second,
            observed_live_blocks_per_second,
            live_rate_multiple,
            rows_per_second,
            compressed_bytes,
            uncompressed_bytes,
            bytes_per_row: if total_rows == 0 {
                0.0
            } else {
                compressed_bytes as f64 / total_rows as f64
            },
            compression_ratio: if compressed_bytes == 0 {
                0.0
            } else {
                uncompressed_bytes as f64 / compressed_bytes as f64
            },
        })
    }

    async fn process_range(
        &self,
        start_block: u64,
        end_block: u64,
        force: bool,
        advance_cursor: bool,
        required_hash: Option<&str>,
    ) -> Result<IngestionReport, IngestionError> {
        let block_count = validated_block_count(start_block, end_block)?;
        if block_count > self.settings.max_blocks_per_run {
            return Err(IngestionError::RangeTooLarge {
                requested: block_count,
                maximum: self.settings.max_blocks_per_run,
            });
        }
        let finalized_height = self.rpc.finalized_height().await?;
        if end_block > finalized_height {
            return Err(IngestionError::BeyondFinalized {
                requested_end: end_block,
                finalized_height,
            });
        }

        let mut report = IngestionReport {
            start_block,
            end_block,
            finalized_height,
            completed_blocks: 0,
            skipped_blocks: 0,
            transaction_count: 0,
            log_count: 0,
            relationship_count: 0,
            feature_count: 0,
            semantic_event_count: 0,
            receipt_data_complete: true,
            trace_data_complete: true,
        };
        let mut previous_hash = if start_block == 0 {
            None
        } else {
            self.store
                .canonical_block(start_block - 1)
                .await?
                .map(|block| block.block_hash)
        };
        let mut chunk_start = start_block;
        while chunk_start <= end_block {
            let chunk_end = chunk_start
                .saturating_add(self.settings.block_fetch_concurrency as u64 - 1)
                .min(end_block);
            let mut existing_blocks = BTreeMap::<u64, Option<StoredBlock>>::new();
            let mut tasks = JoinSet::new();
            for block_number in chunk_start..=chunk_end {
                let existing = self.store.canonical_block(block_number).await?;
                let complete = existing.as_ref().is_some_and(|block| {
                    block.receipt_data_complete == 1
                        && (!self.trace_required || block.trace_data_complete == 1)
                });
                if force || !complete {
                    let rpc = self.rpc.clone();
                    tasks.spawn(
                        async move { (block_number, prepare_block(rpc, block_number).await) },
                    );
                    if !self.settings.request_delay.is_zero() && block_number < chunk_end {
                        sleep(self.settings.request_delay).await;
                    }
                }
                existing_blocks.insert(block_number, existing);
            }
            let mut prepared_blocks = BTreeMap::new();
            while let Some(result) = tasks.join_next().await {
                let (block_number, prepared) = result.map_err(IngestionError::BlockFetchTask)?;
                prepared_blocks.insert(block_number, prepared);
            }

            for block_number in chunk_start..=chunk_end {
                let existing = existing_blocks
                    .remove(&block_number)
                    .expect("every chunk block has stored state");
                if let (Some(parent), Some(block)) = (previous_hash.as_deref(), existing.as_ref())
                    && block.parent_hash != parent
                {
                    return Err(IngestionError::ParentHashMismatch {
                        block_number,
                        block_hash: block.block_hash.clone(),
                        expected_parent: parent.to_string(),
                        actual_parent: block.parent_hash.clone(),
                    });
                }
                if let (Some(expected), Some(block)) = (required_hash, existing.as_ref())
                    && block.block_hash != expected
                {
                    return Err(IngestionError::RequestedHashMismatch {
                        block_number,
                        requested_hash: expected.to_string(),
                        canonical_hash: block.block_hash.clone(),
                    });
                }
                let complete = existing.as_ref().is_some_and(|block| {
                    block.receipt_data_complete == 1
                        && (!self.trace_required || block.trace_data_complete == 1)
                });
                if !force && complete {
                    let block = existing.expect("complete block was checked above");
                    self.store.resolve_failure(block_number).await?;
                    if advance_cursor {
                        self.store
                            .advance_checkpoint(block_number, &block.block_hash)
                            .await?;
                    }
                    previous_hash = Some(block.block_hash);
                    report.skipped_blocks += 1;
                    continue;
                }
                let attempt = match prepared_blocks
                    .remove(&block_number)
                    .expect("every incomplete block has a fetch result")
                {
                    Ok(prepared) => {
                        self.commit_prepared_block(
                            prepared,
                            previous_hash.as_deref(),
                            existing.as_ref().map(|block| block.block_hash.as_str()),
                            required_hash,
                        )
                        .await
                    }
                    Err(error) => Err(error),
                };
                match attempt {
                    Ok(commit) => {
                        self.store.resolve_failure(block_number).await?;
                        if advance_cursor {
                            self.store
                                .advance_checkpoint(block_number, &commit.block_hash)
                                .await?;
                        }
                        previous_hash = Some(commit.block_hash);
                        report.completed_blocks += 1;
                        report.transaction_count += u64::from(commit.transaction_count);
                        report.log_count += u64::from(commit.log_count);
                        report.relationship_count += u64::from(commit.relationship_count);
                        report.feature_count += u64::from(commit.feature_count);
                        report.semantic_event_count += u64::from(commit.semantic_event_count);
                        report.trace_data_complete &= commit.trace_data_complete;
                    }
                    Err(error) => {
                        let summary = error.to_string();
                        if let Err(record_error) = self
                            .store
                            .record_failure(
                                FailureInput {
                                    block_number,
                                    block_hash: error.block_hash(),
                                    stage: error.stage(),
                                    error_class: error.error_class(),
                                    error_summary: &summary,
                                    retryable: error.retryable(),
                                },
                                self.settings.failure_max_attempts,
                            )
                            .await
                        {
                            return Err(IngestionError::FailureRecording {
                                ingestion: Box::new(error),
                                recording: record_error,
                            });
                        }
                        return Err(error);
                    }
                }
            }
            if chunk_end == end_block {
                break;
            }
            chunk_start = chunk_end
                .checked_add(1)
                .ok_or(IngestionError::BlockNumberOverflow)?;
        }
        Ok(report)
    }

    async fn verify_checkpoint(&self) -> Result<Option<u64>, IngestionError> {
        let Some(checkpoint) = self.store.checkpoint().await? else {
            return Ok(None);
        };
        if checkpoint.last_finalized_block_hash.is_empty() {
            return Ok(None);
        }
        let local = self
            .store
            .canonical_block(checkpoint.last_finalized_block)
            .await?
            .ok_or(IngestionError::CheckpointMarkerMissing {
                block_number: checkpoint.last_finalized_block,
            })?;
        if local.block_hash != checkpoint.last_finalized_block_hash {
            return Err(IngestionError::CheckpointHashMismatch {
                block_number: checkpoint.last_finalized_block,
            });
        }
        let remote = self
            .rpc
            .block_identity(checkpoint.last_finalized_block)
            .await?;
        if remote.hash == local.block_hash {
            return Ok(None);
        }
        self.repair_reorg(checkpoint.last_finalized_block)
            .await
            .map(Some)
    }

    async fn repair_reorg(&self, suspect_tip: u64) -> Result<u64, IngestionError> {
        let checkpoint = self
            .store
            .checkpoint()
            .await?
            .ok_or(IngestionError::MissingCheckpoint)?;
        let local_tip = checkpoint.last_finalized_block.min(suspect_tip);
        let mut candidate = local_tip;
        let mut examined = 0_u64;
        let ancestor = loop {
            let local = self.store.canonical_block(candidate).await?;
            if let Some(local) = local {
                let remote = self.rpc.block_identity(candidate).await?;
                if local.block_hash == remote.hash {
                    break Some(local);
                }
            }
            examined += 1;
            if candidate == 0 {
                break None;
            }
            if examined >= self.settings.reorg_max_depth {
                return Err(IngestionError::ReorgDepthExceeded {
                    suspect_tip,
                    maximum: self.settings.reorg_max_depth,
                });
            }
            candidate -= 1;
        };
        let repair_from = ancestor
            .as_ref()
            .map_or(0, |block| block.block_number.saturating_add(1));
        self.store
            .invalidate_range(
                repair_from,
                checkpoint.last_finalized_block,
                &self.rpc_provider,
                &self.rpc_client_version,
            )
            .await?;
        self.store
            .rewind_checkpoint(repair_from, ancestor.as_ref())
            .await?;
        Ok(repair_from)
    }

    async fn commit_prepared_block(
        &self,
        prepared: PreparedBlock,
        expected_parent: Option<&str>,
        existing_hash: Option<&str>,
        required_hash: Option<&str>,
    ) -> Result<CommittedBlock, IngestionError> {
        let block = prepared.block;
        let block_number = block.number;
        if let Some(existing_hash) = existing_hash
            && block.hash != existing_hash
        {
            return Err(IngestionError::CanonicalHashConflict {
                block_number,
                stored_hash: existing_hash.to_string(),
                remote_hash: block.hash,
            });
        }
        if let Some(required_hash) = required_hash
            && block.hash != required_hash
        {
            return Err(IngestionError::RequestedHashMismatch {
                block_number,
                requested_hash: required_hash.to_string(),
                canonical_hash: block.hash,
            });
        }
        if let Some(expected_parent) = expected_parent
            && block.parent_hash != expected_parent
        {
            return Err(IngestionError::ParentHashMismatch {
                block_number,
                block_hash: block.hash,
                expected_parent: expected_parent.to_string(),
                actual_parent: block.parent_hash,
            });
        }
        let registry = self.semantic_registry(block_number).await?;
        let semantic = classify_block(&registry, &block, &prepared.evidence);
        let commit = self
            .store
            .commit_block(
                &block,
                &prepared.evidence,
                &semantic,
                &self.rpc_provider,
                &self.rpc_client_version,
            )
            .await?;
        Ok(CommittedBlock::from_commit(commit))
    }

    async fn semantic_registry(
        &self,
        block_number: u64,
    ) -> Result<SemanticRegistry, IngestionError> {
        let mut cache = self.registry_cache.lock().await;
        let refresh = cache.loaded_at_block.is_none_or(|loaded_at| {
            loaded_at.abs_diff(block_number) >= self.settings.protocol_registry_refresh_blocks
        });
        if refresh {
            cache.registry =
                SemanticRegistry::from_contracts(self.store.protocol_contracts().await?);
            cache.loaded_at_block = Some(block_number);
        }
        Ok(cache.registry.clone())
    }
}

async fn prepare_block(
    rpc: CanonicalRpcClient,
    block_number: u64,
) -> Result<PreparedBlock, IngestionError> {
    let fetched = rpc.block_with_receipts(block_number).await?;
    let block = normalize_block(fetched.block, fetched.receipts)?;
    let evidence = extract_block_evidence(&block, fetched.traces.as_deref())?;
    Ok(PreparedBlock { block, evidence })
}

struct PreparedBlock {
    block: CanonicalBlock,
    evidence: ExtractedBlockEvidence,
}

#[derive(Default)]
struct RegistryCache {
    loaded_at_block: Option<u64>,
    registry: SemanticRegistry,
}

fn validated_block_count(start_block: u64, end_block: u64) -> Result<u64, IngestionError> {
    end_block
        .checked_sub(start_block)
        .and_then(|difference| difference.checked_add(1))
        .ok_or(IngestionError::InvalidRange {
            start_block,
            end_block,
        })
}

fn now_unix_ms() -> Result<u64, IngestionError> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(IngestionError::Clock)?
        .as_millis();
    u64::try_from(value).map_err(|_| IngestionError::ClockOverflow)
}

struct CommittedBlock {
    block_hash: String,
    transaction_count: u32,
    log_count: u32,
    relationship_count: u32,
    feature_count: u32,
    semantic_event_count: u32,
    trace_data_complete: bool,
}

impl CommittedBlock {
    fn from_commit(commit: BlockCommit) -> Self {
        Self {
            block_hash: commit.block_hash,
            transaction_count: commit.transaction_count,
            log_count: commit.log_count,
            relationship_count: commit.relationship_count,
            feature_count: commit.feature_count,
            semantic_event_count: commit.semantic_event_count,
            trace_data_complete: commit.trace_data_complete,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IngestionReport {
    pub start_block: u64,
    pub end_block: u64,
    pub finalized_height: u64,
    pub completed_blocks: u64,
    pub skipped_blocks: u64,
    pub transaction_count: u64,
    pub log_count: u64,
    pub relationship_count: u64,
    pub feature_count: u64,
    pub semantic_event_count: u64,
    pub receipt_data_complete: bool,
    pub trace_data_complete: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SyncBatchReport {
    pub finalized_height: u64,
    pub next_block: u64,
    pub caught_up: bool,
    pub reorg_repaired_from: Option<u64>,
    pub batch: Option<IngestionReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FollowReport {
    pub batches: u64,
    pub completed_blocks: u64,
    pub next_block: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepairReport {
    pub start_block: u64,
    pub end_block: u64,
    pub repaired_blocks: Vec<u64>,
    pub skipped_dead_blocks: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BenchmarkReport {
    pub benchmark_id: String,
    pub ingestion: IngestionReport,
    pub elapsed_ms: u64,
    pub blocks_per_second: f64,
    pub observed_live_blocks_per_second: f64,
    pub live_rate_multiple: f64,
    pub rows_per_second: f64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub bytes_per_row: f64,
    pub compression_ratio: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum IngestionError {
    #[error("invalid block range {start_block}..={end_block}")]
    InvalidRange { start_block: u64, end_block: u64 },
    #[error("requested {requested} blocks; configured maximum is {maximum}")]
    RangeTooLarge { requested: u64, maximum: u64 },
    #[error("repair requested {requested} blocks; configured maximum is {maximum}")]
    RepairRangeTooLarge { requested: u64, maximum: u64 },
    #[error("auto/follow sync has no checkpoint or configured start block")]
    MissingStartBlock,
    #[error("requested end block {requested_end} is newer than finalized block {finalized_height}")]
    BeyondFinalized {
        requested_end: u64,
        finalized_height: u64,
    },
    #[error(transparent)]
    Rpc(#[from] RpcFetchError),
    #[error("concurrent block fetch task failed")]
    BlockFetchTask(#[source] tokio::task::JoinError),
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    TransferDecode(#[from] TransferDecodeError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(
        "parent hash mismatch at block {block_number}: expected {expected_parent}, received {actual_parent}"
    )]
    ParentHashMismatch {
        block_number: u64,
        block_hash: String,
        expected_parent: String,
        actual_parent: String,
    },
    #[error(
        "block {block_number} changed from stored hash {stored_hash} to remote hash {remote_hash}"
    )]
    CanonicalHashConflict {
        block_number: u64,
        stored_hash: String,
        remote_hash: String,
    },
    #[error(
        "requested block hash {requested_hash} is not canonical at {block_number}; canonical hash is {canonical_hash}"
    )]
    RequestedHashMismatch {
        block_number: u64,
        requested_hash: String,
        canonical_hash: String,
    },
    #[error("sync checkpoint references missing canonical block {block_number}")]
    CheckpointMarkerMissing { block_number: u64 },
    #[error("sync checkpoint hash disagrees with canonical marker at block {block_number}")]
    CheckpointHashMismatch { block_number: u64 },
    #[error("reorg repair requires an existing sync checkpoint")]
    MissingCheckpoint,
    #[error("no common ancestor found below block {suspect_tip} within configured depth {maximum}")]
    ReorgDepthExceeded { suspect_tip: u64, maximum: u64 },
    #[error("system clock is before Unix epoch")]
    Clock(#[source] std::time::SystemTimeError),
    #[error("system clock timestamp exceeds UInt64 milliseconds")]
    ClockOverflow,
    #[error("block number overflow while advancing an ingestion batch")]
    BlockNumberOverflow,
    #[error("ingestion failed and its failure record could not be persisted")]
    FailureRecording {
        ingestion: Box<IngestionError>,
        #[source]
        recording: StoreError,
    },
}

impl IngestionError {
    fn stage(&self) -> &'static str {
        match self {
            Self::Rpc(_) | Self::BlockFetchTask(_) => "fetch",
            Self::Decode(_) | Self::TransferDecode(_) => "decode",
            Self::Store(_) | Self::FailureRecording { .. } => "write",
            Self::ParentHashMismatch { .. }
            | Self::CanonicalHashConflict { .. }
            | Self::CheckpointMarkerMissing { .. }
            | Self::CheckpointHashMismatch { .. }
            | Self::MissingCheckpoint
            | Self::ReorgDepthExceeded { .. } => "continuity",
            Self::RequestedHashMismatch { .. } => "replay_validation",
            Self::InvalidRange { .. }
            | Self::RangeTooLarge { .. }
            | Self::RepairRangeTooLarge { .. }
            | Self::MissingStartBlock
            | Self::BeyondFinalized { .. }
            | Self::Clock(_)
            | Self::ClockOverflow
            | Self::BlockNumberOverflow => "range_validation",
        }
    }

    fn error_class(&self) -> &'static str {
        match self {
            Self::Rpc(_) => "rpc_failure",
            Self::BlockFetchTask(_) => "block_fetch_task_failure",
            Self::Decode(_) | Self::TransferDecode(_) => "invalid_evidence",
            Self::Store(_) => "clickhouse_failure",
            Self::ParentHashMismatch { .. } => "parent_hash_mismatch",
            Self::CanonicalHashConflict { .. } => "canonical_hash_conflict",
            Self::RequestedHashMismatch { .. } => "requested_hash_mismatch",
            Self::CheckpointMarkerMissing { .. } => "checkpoint_marker_missing",
            Self::CheckpointHashMismatch { .. } => "checkpoint_hash_mismatch",
            Self::MissingCheckpoint => "checkpoint_missing",
            Self::ReorgDepthExceeded { .. } => "reorg_depth_exceeded",
            Self::InvalidRange { .. } => "invalid_range",
            Self::RangeTooLarge { .. } => "range_limit",
            Self::RepairRangeTooLarge { .. } => "repair_range_limit",
            Self::MissingStartBlock => "start_block_missing",
            Self::BeyondFinalized { .. } => "not_finalized",
            Self::Clock(_) | Self::ClockOverflow => "clock_failure",
            Self::BlockNumberOverflow => "block_number_overflow",
            Self::FailureRecording { .. } => "failure_recording_failed",
        }
    }

    fn retryable(&self) -> bool {
        match self {
            Self::Rpc(error) => error.retryable(),
            Self::Store(_) => true,
            _ => false,
        }
    }

    fn block_hash(&self) -> Option<&str> {
        match self {
            Self::ParentHashMismatch { block_hash, .. } => Some(block_hash),
            Self::CanonicalHashConflict { remote_hash, .. } => Some(remote_hash),
            _ => None,
        }
    }

    fn conflict_height(&self) -> Option<u64> {
        match self {
            Self::ParentHashMismatch { block_number, .. }
            | Self::CanonicalHashConflict { block_number, .. } => Some(*block_number),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::SystemTime,
    };

    use axum::{Json, Router, extract::State, routing::post};
    use clickhouse::Client;
    use serde_json::{Value, json};

    use super::CanonicalIngestor;
    use crate::{
        config::{AppConfig, DeploymentMode, IngestionConfig, RpcEndpoint, TraceMode},
        db::initialize_test_database,
        ingestion::store::{ClickHouseIngestionStore, FailureInput, TestFailPoint},
        semantic::ProtocolRegistryInput,
    };

    const BLOCK_HASH: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
    const REORG_BLOCK_HASH: &str =
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BLOCK_99_HASH: &str =
        "0x9999999999999999999999999999999999999999999999999999999999999999";
    const PARENT_HASH: &str = BLOCK_99_HASH;
    const TX_SUCCESS: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";
    const TX_FAILED: &str = "0x3333333333333333333333333333333333333333333333333333333333333333";
    const TX_CREATE: &str = "0x4444444444444444444444444444444444444444444444444444444444444444";

    struct MockRpc {
        reorged: AtomicBool,
    }

    async fn rpc(State(state): State<Arc<MockRpc>>, Json(request): Json<Value>) -> Json<Value> {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request
            .get("params")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let response = match method {
            "eth_getBlockByNumber" if params.first() == Some(&json!("finalized")) => {
                json!({ "jsonrpc": "2.0", "id": id, "result": {
                    "number": "0x64", "hash": BLOCK_HASH
                }})
            }
            "eth_getBlockByNumber" if params.first() == Some(&json!("0x64")) => {
                let block = if state.reorged.load(Ordering::SeqCst) {
                    reorg_block()
                } else {
                    block()
                };
                json!({ "jsonrpc": "2.0", "id": id, "result": block })
            }
            "eth_getBlockByNumber" if params.first() == Some(&json!("0x63")) => {
                json!({ "jsonrpc": "2.0", "id": id, "result": block_99() })
            }
            "eth_getBlockReceipts" => json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "method unavailable" }
            }),
            "eth_getTransactionReceipt" => {
                let tx_hash = params.first().and_then(Value::as_str).unwrap_or("");
                let receipt = match tx_hash {
                    TX_SUCCESS => success_receipt(),
                    TX_FAILED => failed_receipt(),
                    TX_CREATE => creation_receipt(),
                    _ => Value::Null,
                };
                json!({ "jsonrpc": "2.0", "id": id, "result": receipt })
            }
            "debug_traceBlockByNumber" => {
                let empty =
                    params.first() == Some(&json!("0x63")) || state.reorged.load(Ordering::SeqCst);
                json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": if empty { json!([]) } else { traces() }
                })
            }
            _ => json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "method unavailable" }
            }),
        };
        Json(response)
    }

    fn block() -> Value {
        json!({
            "number": "0x64",
            "hash": BLOCK_HASH,
            "parentHash": PARENT_HASH,
            "timestamp": "0x6553f100",
            "transactions": [
                transaction(TX_SUCCESS, "0x0", json!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"), "0x2a", "0xa9059cbb"),
                transaction(TX_FAILED, "0x1", json!("0xcccccccccccccccccccccccccccccccccccccccc"), "0x0", "0x12345678"),
                transaction(TX_CREATE, "0x2", Value::Null, "0x0", "0x6000")
            ]
        })
    }

    fn block_99() -> Value {
        json!({
            "number": "0x63",
            "hash": BLOCK_99_HASH,
            "parentHash": "0x9898989898989898989898989898989898989898989898989898989898989898",
            "timestamp": "0x6553f0ff",
            "transactions": []
        })
    }

    fn reorg_block() -> Value {
        json!({
            "number": "0x64",
            "hash": REORG_BLOCK_HASH,
            "parentHash": BLOCK_99_HASH,
            "timestamp": "0x6553f100",
            "transactions": []
        })
    }

    fn transaction(hash: &str, index: &str, to: Value, value: &str, input: &str) -> Value {
        json!({
            "hash": hash,
            "blockHash": BLOCK_HASH,
            "blockNumber": "0x64",
            "transactionIndex": index,
            "from": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "to": to,
            "nonce": index,
            "type": "0x2",
            "value": value,
            "input": input,
            "gas": "0x20000",
            "gasPrice": "0x3b9aca00"
        })
    }

    fn receipt(hash: &str, index: &str, to: Value, status: &str) -> Value {
        json!({
            "transactionHash": hash,
            "transactionIndex": index,
            "blockHash": BLOCK_HASH,
            "blockNumber": "0x64",
            "from": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "to": to,
            "contractAddress": null,
            "status": status,
            "type": "0x2",
            "gasUsed": "0x10000",
            "effectiveGasPrice": "0x3b9aca00",
            "logs": []
        })
    }

    fn success_receipt() -> Value {
        let mut value = receipt(
            TX_SUCCESS,
            "0x0",
            json!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            "0x1",
        );
        value["logs"] = json!([{
            "address": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "topics": [
                "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                "0x000000000000000000000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "0x000000000000000000000000dddddddddddddddddddddddddddddddddddddddd"
            ],
            "data": "0x0000000000000000000000000000000000000000000000000000000000000063",
            "blockNumber": "0x64",
            "blockHash": BLOCK_HASH,
            "transactionHash": TX_SUCCESS,
            "transactionIndex": "0x0",
            "logIndex": "0x0",
            "removed": false
        }]);
        value
    }

    fn failed_receipt() -> Value {
        receipt(
            TX_FAILED,
            "0x1",
            json!("0xcccccccccccccccccccccccccccccccccccccccc"),
            "0x0",
        )
    }

    fn creation_receipt() -> Value {
        let mut value = receipt(TX_CREATE, "0x2", Value::Null, "0x1");
        value["contractAddress"] = json!("0xffffffffffffffffffffffffffffffffffffffff");
        value
    }

    fn traces() -> Value {
        json!([
            {
                "txHash": TX_SUCCESS,
                "result": {
                    "type": "CALL",
                    "from": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "to": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "value": "0x2a",
                    "calls": [{
                        "type": "CALL",
                        "from": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "to": "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                        "value": "0x5"
                    }]
                }
            },
            {
                "txHash": TX_FAILED,
                "result": {
                    "type": "CALL",
                    "from": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "to": "0xcccccccccccccccccccccccccccccccccccccccc",
                    "value": "0x0",
                    "error": "execution reverted"
                }
            },
            {
                "txHash": TX_CREATE,
                "result": {
                    "type": "CREATE",
                    "from": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "to": "0xffffffffffffffffffffffffffffffffffffffff",
                    "value": "0x0"
                }
            }
        ])
    }

    async fn start_mock() -> (String, Arc<MockRpc>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(MockRpc {
            reorged: AtomicBool::new(false),
        });
        let app = Router::new()
            .route("/", post(rpc))
            .with_state(state.clone());
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), state, handle)
    }

    async fn disposable_database(prefix: &str) -> (Client, String) {
        let url = env::var("BSC_TEST_CLICKHOUSE_URL")
            .expect("BSC_TEST_CLICKHOUSE_URL must target a disposable ClickHouse instance");
        let user = env::var("BSC_TEST_CLICKHOUSE_USER").unwrap_or_else(|_| "bsc_admin".into());
        let password = env::var("BSC_TEST_CLICKHOUSE_PASSWORD")
            .expect("BSC_TEST_CLICKHOUSE_PASSWORD is required");
        let admin = Client::default()
            .with_url(&url)
            .with_user(&user)
            .with_password(&password);
        let suffix = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let database = format!("bsc_aml_{prefix}_{}_{}", std::process::id(), suffix);
        initialize_test_database(&admin, &database).await.unwrap();
        (admin, database)
    }

    fn test_app_config(rpc_url: &str) -> AppConfig {
        AppConfig {
            mode: DeploymentMode::Development,
            rpc_endpoint: RpcEndpoint::parse(rpc_url).unwrap(),
            fallback_rpc_endpoints: Vec::new(),
            rpc_provider: "integration_test".to_string(),
            rpc_timeout: std::time::Duration::from_secs(3),
            trace_mode: TraceMode::Required,
            require_block_receipts: false,
            trace_probe_block: 0,
        }
    }

    fn test_ingestion_config() -> IngestionConfig {
        IngestionConfig::from_values(&[
            ("BSC_RECEIPT_FETCH_MODE", "auto"),
            ("BSC_RECEIPT_CONCURRENCY", "2"),
            ("BSC_RPC_MAX_ATTEMPTS", "2"),
            ("BSC_INGEST_BATCH_SIZE", "10"),
        ])
        .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires disposable ClickHouse configured through BSC_TEST_CLICKHOUSE_* settings"]
    async fn fixed_range_replay_has_stable_canonical_counts() {
        let url = env::var("BSC_TEST_CLICKHOUSE_URL")
            .expect("BSC_TEST_CLICKHOUSE_URL must target a disposable ClickHouse instance");
        let user = env::var("BSC_TEST_CLICKHOUSE_USER").unwrap_or_else(|_| "bsc_admin".into());
        let password = env::var("BSC_TEST_CLICKHOUSE_PASSWORD")
            .expect("BSC_TEST_CLICKHOUSE_PASSWORD is required");
        let admin = Client::default()
            .with_url(&url)
            .with_user(&user)
            .with_password(&password);
        let suffix = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let database = format!("bsc_aml_ingest_it_{}_{}", std::process::id(), suffix);
        initialize_test_database(&admin, &database).await.unwrap();

        let (rpc_url, _state, server) = start_mock().await;
        let app_config = AppConfig {
            mode: DeploymentMode::Development,
            rpc_endpoint: RpcEndpoint::parse(&rpc_url).unwrap(),
            fallback_rpc_endpoints: Vec::new(),
            rpc_provider: "integration_test".to_string(),
            rpc_timeout: std::time::Duration::from_secs(3),
            trace_mode: TraceMode::Required,
            require_block_receipts: false,
            trace_probe_block: 0,
        };
        let ingestion_config = IngestionConfig::from_values(&[
            ("BSC_RECEIPT_FETCH_MODE", "auto"),
            ("BSC_RECEIPT_CONCURRENCY", "2"),
            ("BSC_RPC_MAX_ATTEMPTS", "2"),
        ])
        .unwrap();
        let store = ClickHouseIngestionStore::for_test(
            admin.clone().with_database(&database),
            database.clone(),
        );
        store
            .import_protocol_contract(ProtocolRegistryInput {
                contract_address: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
                protocol: "reviewed_fixture_scam".to_string(),
                protocol_type: "scam".to_string(),
                contract_role: "reviewed_address".to_string(),
                decoder: "reviewed_interaction".to_string(),
                remote_network_id: String::new(),
                remote_contract_address: String::new(),
                method_ids: Vec::new(),
                method_event_types: Vec::new(),
                event_topics: Vec::new(),
                event_types: Vec::new(),
                remote_receiver_topic_index: -1,
                message_topic_index: -1,
                source_id: "integration_fixture".to_string(),
                source_reference: "fixture://reviewed-scam".to_string(),
                review_status: "approved".to_string(),
                evidence_confidence: 1.0,
                enabled: true,
                reviewed_by: "integration-test".to_string(),
                review_note: String::new(),
            })
            .await
            .unwrap();
        let ingestor = CanonicalIngestor::new(
            &app_config,
            ingestion_config,
            store,
            "mock-bsc/v1".to_string(),
        )
        .unwrap();

        let first = ingestor.ingest_range(100, 100).await.unwrap();
        let skipped = ingestor.ingest_range(100, 100).await.unwrap();
        let replay = ingestor.replay_range(100, 100).await.unwrap();
        assert_eq!(first.completed_blocks, 1);
        assert_eq!(first.transaction_count, 3);
        assert_eq!(first.log_count, 1);
        assert_eq!(first.relationship_count, 3);
        assert_eq!(first.feature_count, 1);
        assert_eq!(first.semantic_event_count, 1);
        assert!(first.trace_data_complete);
        assert_eq!(skipped.completed_blocks, 0);
        assert_eq!(skipped.skipped_blocks, 1);
        assert_eq!(replay.completed_blocks, 1);
        assert_eq!(replay.skipped_blocks, 0);

        let database_client = admin.clone().with_database(&database);
        let canonical_blocks = database_client
            .query("SELECT count() FROM ingested_blocks_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let canonical_transactions = database_client
            .query("SELECT count() FROM transactions_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let canonical_logs = database_client
            .query("SELECT count() FROM evm_logs_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let canonical_relationships = database_client
            .query("SELECT count() FROM address_relationships_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let canonical_discoveries = database_client
            .query("SELECT count() FROM token_metadata_discoveries_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let canonical_features = database_client
            .query("SELECT count() FROM transaction_features_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let canonical_semantic_events = database_client
            .query("SELECT count() FROM semantic_aml_events_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let failed_transactions = database_client
            .query("SELECT count() FROM transactions_canonical WHERE status = 0")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let created_contracts = database_client
            .query("SELECT count() FROM transactions_canonical WHERE contract_address != ''")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let current_revision = database_client
            .query("SELECT max(current_revision) FROM ingested_blocks_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let fact_revision = database_client
            .query("SELECT max(block_state_revision) FROM transactions_canonical")
            .fetch_one::<u64>()
            .await
            .unwrap();

        assert_eq!(canonical_blocks, 1);
        assert_eq!(canonical_transactions, 3);
        assert_eq!(canonical_logs, 1);
        assert_eq!(canonical_relationships, 3);
        assert_eq!(canonical_discoveries, 1);
        assert_eq!(canonical_features, 1);
        assert_eq!(canonical_semantic_events, 1);
        assert_eq!(failed_transactions, 1);
        assert_eq!(created_contracts, 1);
        assert_eq!(current_revision, 2);
        assert_eq!(fact_revision, current_revision);

        server.abort();
        admin
            .query(&format!("DROP DATABASE IF EXISTS {database}"))
            .execute()
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires disposable ClickHouse configured through BSC_TEST_CLICKHOUSE_* settings"]
    async fn crash_boundaries_never_expose_partial_facts_or_advance_cursor() {
        let (admin, database) = disposable_database("crash_it").await;
        let (rpc_url, _state, server) = start_mock().await;
        let store = ClickHouseIngestionStore::for_test(
            admin.clone().with_database(&database),
            database.clone(),
        );
        let ingestor = CanonicalIngestor::new(
            &test_app_config(&rpc_url),
            test_ingestion_config(),
            store.clone(),
            "mock-bsc/v1".to_string(),
        )
        .unwrap();
        let database_client = admin.clone().with_database(&database);

        let boundaries = [
            TestFailPoint::Transactions,
            TestFailPoint::Logs,
            TestFailPoint::Relationships,
            TestFailPoint::TokenDiscoveries,
            TestFailPoint::SemanticEvidence,
        ];
        for (index, boundary) in boundaries.into_iter().enumerate() {
            let expected_revision = index as u64;
            store.inject_failure_once(boundary);
            assert!(ingestor.replay_range(100, 100).await.is_err());
            let visible_revision = database_client
                .query("SELECT coalesce(max(current_revision), 0) FROM ingested_blocks_canonical WHERE block_number = 100")
                .fetch_one::<u64>()
                .await
                .unwrap();
            assert_eq!(visible_revision, expected_revision);

            ingestor.replay_range(100, 100).await.unwrap();
            let canonical_transactions = database_client
                .query("SELECT count() FROM transactions_canonical WHERE block_number = 100")
                .fetch_one::<u64>()
                .await
                .unwrap();
            assert_eq!(canonical_transactions, 3);
        }

        store.inject_failure_once(TestFailPoint::CompleteMarker);
        assert!(ingestor.sync_once(Some(99), Some(1)).await.is_err());
        let cursor_count = database_client
            .query("SELECT count() FROM sync_state_current")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let block_count = database_client
            .query("SELECT count() FROM ingested_blocks_canonical WHERE block_number = 99")
            .fetch_one::<u64>()
            .await
            .unwrap();
        assert_eq!(cursor_count, 0);
        assert_eq!(block_count, 1);

        let recovered = ingestor.sync_once(Some(99), Some(1)).await.unwrap();
        assert_eq!(recovered.batch.unwrap().skipped_blocks, 1);
        let next_block = database_client
            .query("SELECT next_block FROM sync_state_current")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let revision = database_client
            .query("SELECT current_revision FROM ingested_blocks_canonical WHERE block_number = 99")
            .fetch_one::<u64>()
            .await
            .unwrap();
        assert_eq!(next_block, 100);
        assert_eq!(revision, 1);

        server.abort();
        admin
            .query(&format!("DROP DATABASE IF EXISTS {database}"))
            .execute()
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires disposable ClickHouse configured through BSC_TEST_CLICKHOUSE_* settings"]
    async fn simulated_reorg_rewinds_to_common_ancestor_and_replaces_canonical_facts() {
        let (admin, database) = disposable_database("reorg_it").await;
        let (rpc_url, state, server) = start_mock().await;
        let store = ClickHouseIngestionStore::for_test(
            admin.clone().with_database(&database),
            database.clone(),
        );
        let ingestor = CanonicalIngestor::new(
            &test_app_config(&rpc_url),
            test_ingestion_config(),
            store,
            "mock-bsc/v1".to_string(),
        )
        .unwrap();

        let initial = ingestor.sync_once(Some(99), Some(2)).await.unwrap();
        assert_eq!(initial.batch.unwrap().completed_blocks, 2);
        state.reorged.store(true, Ordering::SeqCst);
        let repaired = ingestor.sync_once(None, Some(1)).await.unwrap();
        assert_eq!(repaired.reorg_repaired_from, Some(100));

        let database_client = admin.clone().with_database(&database);
        let canonical_hash = database_client
            .query("SELECT block_hash FROM ingested_blocks_canonical WHERE block_number = 100")
            .fetch_one::<String>()
            .await
            .unwrap();
        let current_revision = database_client
            .query(
                "SELECT current_revision FROM ingested_blocks_canonical WHERE block_number = 100",
            )
            .fetch_one::<u64>()
            .await
            .unwrap();
        let old_transactions = database_client
            .query("SELECT count() FROM transactions_canonical WHERE block_number = 100")
            .fetch_one::<u64>()
            .await
            .unwrap();
        let next_block = database_client
            .query("SELECT next_block FROM sync_state_current")
            .fetch_one::<u64>()
            .await
            .unwrap();
        assert_eq!(canonical_hash, REORG_BLOCK_HASH);
        assert_eq!(current_revision, 3);
        assert_eq!(old_transactions, 0);
        assert_eq!(next_block, 101);

        server.abort();
        admin
            .query(&format!("DROP DATABASE IF EXISTS {database}"))
            .execute()
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires disposable ClickHouse configured through BSC_TEST_CLICKHOUSE_* settings"]
    async fn dead_letter_requires_explicit_requeue_before_gap_repair() {
        let (admin, database) = disposable_database("dead_letter_it").await;
        let (rpc_url, _state, server) = start_mock().await;
        let store = ClickHouseIngestionStore::for_test(
            admin.clone().with_database(&database),
            database.clone(),
        );
        store
            .record_failure(
                FailureInput {
                    block_number: 100,
                    block_hash: None,
                    stage: "fetch",
                    error_class: "test_failure",
                    error_summary: "bounded retry test",
                    retryable: true,
                },
                1,
            )
            .await
            .unwrap();
        let ingestor = CanonicalIngestor::new(
            &test_app_config(&rpc_url),
            test_ingestion_config(),
            store,
            "mock-bsc/v1".to_string(),
        )
        .unwrap();

        let skipped = ingestor.repair_range(100, 100, false).await.unwrap();
        assert!(skipped.repaired_blocks.is_empty());
        assert_eq!(skipped.skipped_dead_blocks, 1);

        let repaired = ingestor.repair_range(100, 100, true).await.unwrap();
        assert_eq!(repaired.repaired_blocks, vec![100]);
        let database_client = admin.clone().with_database(&database);
        let status = database_client
            .query("SELECT status FROM ingestion_failures_current WHERE block_number = 100")
            .fetch_one::<String>()
            .await
            .unwrap();
        let canonical_blocks = database_client
            .query("SELECT count() FROM ingested_blocks_canonical WHERE block_number = 100")
            .fetch_one::<u64>()
            .await
            .unwrap();
        assert_eq!(status, "resolved");
        assert_eq!(canonical_blocks, 1);

        server.abort();
        admin
            .query(&format!("DROP DATABASE IF EXISTS {database}"))
            .execute()
            .await
            .unwrap();
    }
}
