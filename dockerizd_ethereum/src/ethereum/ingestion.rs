use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, ensure};
use serde::Serialize;
use tokio::{
    sync::Mutex,
    time::{Instant, sleep},
};

use crate::{config::AppConfig, storage::EthereumStore};

use super::{EthereumRpc, SemanticDecoderRegistry, extract_block};

struct RegistryCache {
    loaded_at_block: Option<u64>,
    registry: SemanticDecoderRegistry,
}

#[derive(Debug, Default, Serialize)]
pub struct IngestionReport {
    pub from_block: u64,
    pub to_block: u64,
    pub completed_blocks: u64,
    pub skipped_blocks: u64,
    pub transactions: u64,
    pub logs: u64,
    pub relationships: u64,
    pub discovered_tokens: u64,
    pub transaction_features: u64,
    pub semantic_events: u64,
    pub rpc_attempts: u64,
    pub elapsed_ms: u64,
    pub trace_data_complete: bool,
}

pub struct IngestionService {
    config: AppConfig,
    rpc: EthereumRpc,
    store: EthereumStore,
    registry_cache: Mutex<RegistryCache>,
}

impl IngestionService {
    pub async fn connect(config: AppConfig) -> anyhow::Result<Self> {
        let rpc = EthereumRpc::connect(&config).await?;
        let store = EthereumStore::new(&config);
        let registry = SemanticDecoderRegistry::from_contracts(store.protocol_contracts().await?);
        Ok(Self {
            config,
            rpc,
            store,
            registry_cache: Mutex::new(RegistryCache {
                loaded_at_block: None,
                registry,
            }),
        })
    }

    pub async fn ingest_range(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<IngestionReport> {
        ensure!(
            from_block <= to_block,
            "from-block must not exceed to-block"
        );
        let finalized = self.rpc.finalized_block_number().await?;
        ensure!(
            to_block <= finalized,
            "requested block {to_block} is above finalized head {finalized}"
        );

        self.ingest_bounded_range(from_block, to_block).await
    }

    pub async fn follow_finalized(
        &self,
        requested_start: Option<u64>,
        max_blocks: Option<u64>,
    ) -> anyhow::Result<IngestionReport> {
        ensure!(max_blocks != Some(0), "max-blocks must be positive");
        let checkpoint = self.store.checkpoint().await?.map(|(block, _)| block);
        let mut next_block = follow_start(requested_start, checkpoint, self.config.eth_start_block);

        let started = Instant::now();
        let mut report = IngestionReport {
            from_block: next_block,
            to_block: next_block,
            trace_data_complete: false,
            ..Default::default()
        };

        loop {
            let finalized = self.rpc.finalized_block_number().await?;
            while next_block <= finalized {
                let block_report = self.ingest_one(next_block).await?;
                report.absorb(block_report);
                report.to_block = next_block;
                next_block = next_block.saturating_add(1);

                if max_blocks.is_some_and(|limit| report.completed_blocks >= limit) {
                    report.elapsed_ms = elapsed_millis(started);
                    return Ok(report);
                }
                self.rate_limit_delay().await;
            }

            sleep(Duration::from_secs(self.config.eth_poll_interval_seconds)).await;
        }
    }

    async fn ingest_bounded_range(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<IngestionReport> {
        let started = Instant::now();
        let mut report = IngestionReport {
            from_block,
            to_block: from_block,
            trace_data_complete: false,
            ..Default::default()
        };

        for block_number in from_block..=to_block {
            let block_report = self.ingest_one(block_number).await?;
            report.absorb(block_report);
            report.to_block = block_number;
            if block_number < to_block {
                self.rate_limit_delay().await;
            }
        }
        report.elapsed_ms = elapsed_millis(started);
        Ok(report)
    }

    async fn ingest_one(&self, block_number: u64) -> anyhow::Result<IngestionReport> {
        let started = Instant::now();
        if self
            .store
            .is_block_complete(block_number, self.config.eth_trace_mode.required())
            .await?
        {
            tracing::info!(
                block_number,
                "finalized Ethereum block already complete; skipping replay"
            );
            return Ok(IngestionReport {
                from_block: block_number,
                to_block: block_number,
                skipped_blocks: 1,
                elapsed_ms: elapsed_millis(started),
                trace_data_complete: false,
                ..Default::default()
            });
        }

        let result = async {
            let fetched = self.rpc.fetch_block(block_number).await?;
            let registry = self.semantic_registry(block_number).await?;
            let extracted = extract_block(
                &self.config,
                &fetched.block,
                &fetched.receipts,
                fetched.traces.as_deref(),
                &registry,
                unix_time_millis()?,
            )?;
            let report = IngestionReport {
                from_block: block_number,
                to_block: block_number,
                completed_blocks: 1,
                skipped_blocks: 0,
                transactions: extracted.transactions.len() as u64,
                logs: extracted.logs.len() as u64,
                relationships: extracted.relationships.len() as u64,
                discovered_tokens: extracted.token_discoveries.len() as u64,
                transaction_features: extracted.transaction_features.len() as u64,
                semantic_events: extracted.semantic_events.len() as u64,
                rpc_attempts: fetched.attempts as u64,
                elapsed_ms: 0,
                trace_data_complete: fetched.traces.is_some(),
            };
            self.store.persist_block(&extracted).await?;
            Ok::<_, anyhow::Error>(report)
        }
        .await;

        match result {
            Ok(mut report) => {
                report.elapsed_ms = elapsed_millis(started);
                tracing::info!(
                    block_number,
                    transactions = report.transactions,
                    logs = report.logs,
                    relationships = report.relationships,
                    semantic_events = report.semantic_events,
                    trace_data_complete = report.trace_data_complete,
                    elapsed_ms = report.elapsed_ms,
                    "finalized Ethereum block ingested"
                );
                Ok(report)
            }
            Err(error) => {
                let timestamp = unix_time_millis().unwrap_or_default();
                if let Err(store_error) = self
                    .store
                    .record_failure(
                        block_number,
                        "fetch_extract_persist",
                        &format!("{error:#}"),
                        self.config.eth_rpc_max_retries,
                        timestamp,
                    )
                    .await
                {
                    tracing::error!(
                        block_number,
                        error = %store_error,
                        "failed to persist Ethereum ingestion failure"
                    );
                }
                Err(error)
                    .with_context(|| format!("Ethereum block {block_number} ingestion failed"))
            }
        }
    }

    async fn semantic_registry(
        &self,
        block_number: u64,
    ) -> anyhow::Result<SemanticDecoderRegistry> {
        let mut cache = self.registry_cache.lock().await;
        let should_refresh = cache.loaded_at_block.is_none_or(|loaded_at| {
            block_number.saturating_sub(loaded_at)
                >= self.config.eth_protocol_registry_refresh_blocks
        });
        if should_refresh {
            cache.registry =
                SemanticDecoderRegistry::from_contracts(self.store.protocol_contracts().await?);
            cache.loaded_at_block = Some(block_number);
        }
        Ok(cache.registry.clone())
    }

    async fn rate_limit_delay(&self) {
        if self.config.eth_request_delay_ms > 0 {
            sleep(Duration::from_millis(self.config.eth_request_delay_ms)).await;
        }
    }
}

impl IngestionReport {
    fn absorb(&mut self, other: Self) {
        let had_completed_blocks = self.completed_blocks > 0;
        if other.completed_blocks > 0 {
            self.trace_data_complete = if had_completed_blocks {
                self.trace_data_complete && other.trace_data_complete
            } else {
                other.trace_data_complete
            };
        }
        self.completed_blocks += other.completed_blocks;
        self.transactions += other.transactions;
        self.skipped_blocks += other.skipped_blocks;
        self.logs += other.logs;
        self.relationships += other.relationships;
        self.discovered_tokens += other.discovered_tokens;
        self.transaction_features += other.transaction_features;
        self.semantic_events += other.semantic_events;
        self.rpc_attempts += other.rpc_attempts;
    }
}

fn follow_start(requested_start: Option<u64>, checkpoint: Option<u64>, configured_start: u64) -> u64 {
    requested_start.unwrap_or_else(|| {
        checkpoint.map_or(configured_start, |block| block.saturating_add(1))
    })
}

fn unix_time_millis() -> anyhow::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis()
        .try_into()
        .context("Unix timestamp does not fit UInt64")?)
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{IngestionReport, follow_start};

    #[test]
    fn follow_honors_initial_history_and_resumes_checkpoint() {
        assert_eq!(follow_start(None, None, 0), 0);
        assert_eq!(follow_start(None, None, 1_000), 1_000);
        assert_eq!(follow_start(None, Some(2_094), 0), 2_095);
        assert_eq!(follow_start(None, Some(2_094), 5_000), 2_095);
        assert_eq!(follow_start(Some(10), Some(2_094), 0), 10);
        assert_eq!(follow_start(Some(10), None, 0), 10);
    }

    #[test]
    fn aggregates_completed_and_skipped_blocks() {
        let mut report = IngestionReport {
            from_block: 10,
            to_block: 10,
            completed_blocks: 1,
            transactions: 3,
            ..Default::default()
        };
        report.absorb(IngestionReport {
            from_block: 11,
            to_block: 11,
            skipped_blocks: 1,
            ..Default::default()
        });

        assert_eq!(report.completed_blocks, 1);
        assert_eq!(report.skipped_blocks, 1);
        assert_eq!(report.transactions, 3);
    }
}
