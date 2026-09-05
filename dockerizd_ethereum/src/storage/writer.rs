use anyhow::{Context, ensure};
use clickhouse::{Client, Row, RowOwned, RowWrite};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    config::AppConfig,
    ethereum::ExtractedBlock,
    storage::{
        IngestionFailureRow, ProtocolContractRow, SyncStateRow, TokenMetadataJobRow,
        TokenMetadataRow,
    },
};

#[derive(Debug, Deserialize, Row)]
struct StoredBlockHash {
    block_hash: String,
    trace_data_complete: u8,
}

#[derive(Debug, Deserialize, Row)]
struct StoredCheckpoint {
    last_synced_block: u64,
    last_synced_block_hash: String,
}

#[derive(Clone)]
pub struct EthereumStore {
    client: Client,
    network_id: String,
    max_batch_rows: usize,
}

impl EthereumStore {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            client: crate::db::database_client(config),
            network_id: config.eth_network_id.clone(),
            max_batch_rows: config.eth_ingestion_batch_max_rows,
        }
    }

    pub async fn checkpoint(&self) -> anyhow::Result<Option<(u64, String)>> {
        let checkpoint = self
            .client
            .query(
                r#"
                SELECT last_synced_block, last_synced_block_hash
                FROM sync_state
                WHERE network_id = ?
                ORDER BY updated_at DESC
                LIMIT 1
                "#,
            )
            .bind(&self.network_id)
            .fetch_optional::<StoredCheckpoint>()
            .await
            .context("failed to read Ethereum ingestion checkpoint")?;

        Ok(checkpoint.map(|row| (row.last_synced_block, row.last_synced_block_hash)))
    }

    pub async fn is_block_complete(
        &self,
        block_number: u64,
        require_traces: bool,
    ) -> anyhow::Result<bool> {
        Ok(self
            .stored_block_hash(block_number)
            .await?
            .is_some_and(|stored| !require_traces || stored.trace_data_complete == 1))
    }

    pub async fn protocol_contracts(&self) -> anyhow::Result<Vec<ProtocolContractRow>> {
        self.client
            .query(
                r#"
                SELECT
                    network_id,
                    contract_address,
                    protocol,
                    protocol_type,
                    contract_role,
                    remote_network_id,
                    remote_contract_address,
                    decoder,
                    source,
                    confidence,
                    enabled,
                    created_at_unix_ms
                FROM protocol_contract_registry_active
                WHERE network_id = ?
                "#,
            )
            .bind(&self.network_id)
            .fetch_all::<ProtocolContractRow>()
            .await
            .context("failed to load active Ethereum protocol registry")
    }

    pub async fn persist_block(&self, extracted: &ExtractedBlock) -> anyhow::Result<()> {
        self.validate_chain_continuity(extracted).await?;

        insert_rows(
            &self.client,
            "transactions",
            &extracted.transactions,
            self.max_batch_rows,
        )
        .await?;
        insert_rows(
            &self.client,
            "evm_logs",
            &extracted.logs,
            self.max_batch_rows,
        )
        .await?;
        insert_rows(
            &self.client,
            "address_relationships",
            &extracted.relationships,
            self.max_batch_rows,
        )
        .await?;
        insert_rows(
            &self.client,
            "transaction_features",
            &extracted.transaction_features,
            self.max_batch_rows,
        )
        .await?;
        insert_rows(
            &self.client,
            "semantic_aml_events",
            &extracted.semantic_events,
            self.max_batch_rows,
        )
        .await?;
        insert_rows(
            &self.client,
            "token_metadata_discoveries",
            &extracted.token_discoveries,
            self.max_batch_rows,
        )
        .await?;
        insert_rows(
            &self.client,
            "token_metadata_jobs",
            &extracted.token_jobs,
            self.max_batch_rows,
        )
        .await?;

        // The completion marker is deliberately last. A failed partial write is replayable.
        insert_rows(
            &self.client,
            "ingested_blocks",
            std::slice::from_ref(&extracted.block),
            1,
        )
        .await?;

        let checkpoint = self.checkpoint().await?;
        if checkpoint
            .as_ref()
            .is_none_or(|(block_number, _)| extracted.block.block_number >= *block_number)
        {
            let row = SyncStateRow {
                network_id: extracted.block.network_id.clone(),
                last_synced_block: extracted.block.block_number,
                last_synced_block_hash: extracted.block.block_hash.clone(),
                updated_at_unix_ms: extracted.block.indexed_at_unix_ms,
            };
            insert_rows(&self.client, "sync_state", std::slice::from_ref(&row), 1).await?;
        }

        Ok(())
    }

    pub async fn pending_token_metadata_jobs(
        &self,
        limit: u64,
        max_attempts: u8,
    ) -> anyhow::Result<Vec<TokenMetadataJobRow>> {
        self.client
            .query(
                r#"
                SELECT
                    any(network_id) AS network_id,
                    token_address,
                    argMax(token_standard, updated_at_unix_ms) AS token_standard,
                    min(discovered_block) AS discovered_block,
                    'pending' AS status,
                    max(attempt_count) AS attempt_count,
                    argMax(last_error, updated_at_unix_ms) AS last_error,
                    max(updated_at_unix_ms) AS updated_at_unix_ms
                FROM token_metadata_jobs
                WHERE network_id = ?
                  AND token_address NOT IN
                  (
                      SELECT token_address
                      FROM token_metadata FINAL
                      WHERE network_id = ?
                  )
                GROUP BY token_address
                HAVING max(attempt_count) < ?
                ORDER BY discovered_block, token_address
                LIMIT ?
                "#,
            )
            .bind(&self.network_id)
            .bind(&self.network_id)
            .bind(max_attempts)
            .bind(limit)
            .fetch_all::<TokenMetadataJobRow>()
            .await
            .context("failed to load pending Ethereum token metadata jobs")
    }

    pub async fn persist_token_metadata(
        &self,
        metadata: &TokenMetadataRow,
        job: &TokenMetadataJobRow,
    ) -> anyhow::Result<()> {
        insert_rows(
            &self.client,
            "token_metadata",
            std::slice::from_ref(metadata),
            1,
        )
        .await?;
        insert_rows(
            &self.client,
            "token_metadata_jobs",
            std::slice::from_ref(job),
            1,
        )
        .await
    }

    pub async fn update_token_metadata_job(&self, job: &TokenMetadataJobRow) -> anyhow::Result<()> {
        insert_rows(
            &self.client,
            "token_metadata_jobs",
            std::slice::from_ref(job),
            1,
        )
        .await
    }
    pub async fn record_failure(
        &self,
        block_number: u64,
        stage: &str,
        message: &str,
        attempt_count: u32,
        timestamp_unix_ms: u64,
    ) -> anyhow::Result<()> {
        let identity = format!("{}|{block_number}|{stage}", self.network_id);
        let row = IngestionFailureRow {
            failure_id: format!("{:x}", Sha256::digest(identity.as_bytes())),
            network_id: self.network_id.clone(),
            block_number,
            block_hash: String::new(),
            tx_hash: String::new(),
            stage: stage.to_string(),
            error_class: "rpc_or_decode".to_string(),
            error_message: message.to_string(),
            retryable: 1,
            attempt_count,
            status: "open".to_string(),
            first_failed_at_unix_ms: timestamp_unix_ms,
            last_failed_at_unix_ms: timestamp_unix_ms,
            resolved_at_unix_ms: 0,
        };
        insert_rows(
            &self.client,
            "ingestion_failures",
            std::slice::from_ref(&row),
            1,
        )
        .await
    }

    async fn validate_chain_continuity(&self, extracted: &ExtractedBlock) -> anyhow::Result<()> {
        let same_height = self.stored_block_hash(extracted.block.block_number).await?;
        if let Some(stored) = same_height {
            ensure!(
                stored.block_hash == extracted.block.block_hash,
                "finalized block hash conflict at height {}: stored {}, received {}",
                extracted.block.block_number,
                stored.block_hash,
                extracted.block.block_hash
            );
        }

        if extracted.block.block_number > 0 {
            if let Some(parent) = self
                .stored_block_hash(extracted.block.block_number - 1)
                .await?
            {
                ensure!(
                    parent.block_hash == extracted.block.parent_hash,
                    "parent hash mismatch before finalized block {}: stored {}, received {}",
                    extracted.block.block_number,
                    parent.block_hash,
                    extracted.block.parent_hash
                );
            }
        }

        Ok(())
    }

    async fn stored_block_hash(
        &self,
        block_number: u64,
    ) -> anyhow::Result<Option<StoredBlockHash>> {
        let row = self
            .client
            .query(
                r#"
                SELECT block_hash, trace_data_complete
                FROM ingested_blocks
                WHERE network_id = ?
                  AND block_number = ?
                  AND ingestion_status = 'complete'
                ORDER BY updated_at DESC
                LIMIT 1
                "#,
            )
            .bind(&self.network_id)
            .bind(block_number)
            .fetch_optional::<StoredBlockHash>()
            .await
            .with_context(|| format!("failed to inspect stored Ethereum block {block_number}"))?;
        Ok(row)
    }
}

async fn insert_rows<R>(
    client: &Client,
    table: &str,
    rows: &[R],
    max_batch_rows: usize,
) -> anyhow::Result<()>
where
    R: RowOwned + RowWrite,
{
    if rows.is_empty() {
        return Ok(());
    }

    for chunk in rows.chunks(max_batch_rows) {
        let mut insert = client
            .insert::<R>(table)
            .await
            .with_context(|| format!("failed to start ClickHouse insert into {table}"))?;
        for row in chunk {
            insert
                .write(row)
                .await
                .with_context(|| format!("failed to serialize row for {table}"))?;
        }
        insert
            .end()
            .await
            .with_context(|| format!("failed to commit ClickHouse insert into {table}"))?;
    }

    Ok(())
}
