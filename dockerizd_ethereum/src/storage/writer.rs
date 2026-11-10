use crate::{
    config::AppConfig,
    db::{Client, database_client, now_ms},
    ethereum::ExtractedBlock,
    storage::{ProtocolContractRow, TokenMetadataJobRow, TokenMetadataRow},
};
use aml_warehouse::{
    Network,
    publication::{self, Block},
};
use anyhow::{Context, Result, ensure};
use clickhouse::Row;
use serde::Deserialize;
use serde_json::json;
#[derive(Clone)]
pub struct EthereumStore {
    client: Client,
}
#[derive(Deserialize, Row)]
struct StoredBlock {
    block_hash: String,
    trace_data_complete: u8,
}
impl EthereumStore {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            client: database_client(config),
        }
    }
    pub async fn checkpoint(&self) -> Result<Option<(u64, String)>> {
        let state =
            publication::state(&self.client, self.client.database(), Network::Ethereum).await?;
        Ok((state.initialized == 1).then_some((state.last_block_number, state.last_block_hash)))
    }
    pub async fn complete_block_hash(
        &self,
        height: u64,
        require_traces: bool,
    ) -> Result<Option<String>> {
        Ok(self.client.query("SELECT block_hash,ifNull(trace_coverage,toUInt8(0)) AS trace_data_complete FROM aml_ingested_blocks_canonical WHERE network_id='eip155:1' AND block_number=?").bind(height).fetch_optional::<StoredBlock>().await?.filter(|b|!require_traces||b.trace_data_complete==1).map(|b|b.block_hash))
    }
    pub async fn protocol_contracts(&self) -> Result<Vec<ProtocolContractRow>> {
        Ok(self.client.query("SELECT network_id,contract_address,protocol,protocol_type,contract_role,remote_network_id,remote_contract_address,decoder,source,confidence,enabled,created_at_unix_ms FROM eth_protocols").fetch_all().await?)
    }
    pub async fn persist_block(&self, source: &ExtractedBlock, observed: u64) -> Result<()> {
        self.persist_block_force(source, observed, false).await
    }
    pub async fn persist_block_force(
        &self,
        source: &ExtractedBlock,
        observed: u64,
        force: bool,
    ) -> Result<()> {
        let previous =
            publication::state(&self.client, self.client.database(), Network::Ethereum).await?;
        let mut refresh = false;
        if previous.initialized == 1 && source.block.block_number <= previous.last_block_number {
            let old=self.client.query("SELECT block_hash,ifNull(trace_coverage,toUInt8(0)) AS trace_data_complete FROM aml_ingested_blocks_canonical WHERE network_id='eip155:1' AND block_number=?").bind(source.block.block_number).fetch_optional::<StoredBlock>().await?.context("requested replay lies outside the observed range")?;
            ensure!(
                old.block_hash == source.block.block_hash,
                "finalized Ethereum hash conflict; refusing an implicit branch replacement"
            );
            if !force && old.trace_data_complete >= source.block.trace_data_complete {
                return Ok(());
            }
            refresh = true;
        }
        let revision = publication::next_revision(
            &self.client,
            self.client.database(),
            Network::Ethereum,
            previous.revision,
        )
        .await?;
        let block = Block {
            network_id: Network::Ethereum.id().into(),
            block_number: source.block.block_number,
            block_hash: source.block.block_hash.clone(),
            block_timestamp_unix_ms: source.block.block_timestamp_unix_ms,
            commit_id: format!("{revision}-{}", source.block.block_hash),
            revision,
            parent_hash: source.block.parent_hash.clone(),
            canonical: 1,
            ingestion_status: "complete".into(),
            transaction_count: source.block.transaction_count,
            receipt_coverage: Some(source.block.receipt_data_complete),
            trace_coverage: Some(source.block.trace_data_complete),
            prevout_coverage: None,
            source_id: source.block.rpc_provider.clone(),
            created_at_unix_ms: now_ms(),
        };
        let facts = super::mapping::evidence(&block, source)?;
        let original =
            json!({"ethereum_block":source.source_block,"withdrawals":source.block_credits});
        if refresh {
            aml_warehouse::core::refresh_observed_batch(
                &self.client,
                self.client.database(),
                &previous,
                &block,
                &facts,
                &original.to_string(),
                observed,
            )
            .await?;
        } else {
            aml_warehouse::core::publish_observed_batch(
                &self.client,
                self.client.database(),
                &previous,
                &block,
                &facts,
                &original.to_string(),
                observed,
            )
            .await?;
        }

        Ok(())
    }
    pub async fn pending_token_metadata_jobs(
        &self,
        limit: u64,
        max_attempts: u8,
    ) -> Result<Vec<TokenMetadataJobRow>> {
        Ok(self.client.query(r#"
            SELECT j.network_id AS network_id,j.token_address AS token_address,
                argMax(j.token_standard,j.updated_at_unix_ms) AS token_standard,
                min(j.discovered_block) AS discovered_block,'pending' AS status,
                max(j.attempt_count) AS attempt_count,
                argMax(j.last_error,j.updated_at_unix_ms) AS last_error,
                max(j.updated_at_unix_ms) AS updated_at_unix_ms
            FROM eth_metadata_jobs AS j
            WHERE (j.network_id,j.token_address) NOT IN (SELECT network_id,token_address FROM eth_metadata)
            GROUP BY j.network_id,j.token_address HAVING max(j.attempt_count)<?
            ORDER BY discovered_block,token_address LIMIT ?
        "#).bind(max_attempts).bind(limit).fetch_all().await?)
    }
    pub async fn persist_token_metadata(
        &self,
        m: &TokenMetadataRow,
        j: &TokenMetadataJobRow,
    ) -> Result<()> {
        crate::db::records::metadata(&self.client, m, j).await
    }
    pub async fn update_token_metadata_job(&self, j: &TokenMetadataJobRow) -> Result<()> {
        crate::db::records::metadata_job(&self.client, j).await
    }
    pub async fn record_failure(
        &self,
        height: u64,
        stage: &str,
        message: &str,
        _attempts: u32,
        time: u64,
    ) -> Result<()> {
        use sha2::{Digest, Sha256};
        let failure_id = format!("{:x}", Sha256::digest(format!("eip155:1|{height}|{stage}")));
        let previous = self
            .client
            .rows(
                "aml_ingestion_failures",
                &format!("failure_id={}", publication::literal(&failure_id)),
            )
            .await?
            .pop();
        let attempts = previous
            .as_ref()
            .and_then(|r| r["attempts"].as_u64())
            .unwrap_or(0)
            .saturating_add(1)
            .min(u32::MAX as u64);
        let retryable =
            attempts < aml_warehouse::maintenance::failure_limit("ETHEREUM_FAILURE_MAX_ATTEMPTS")?;
        let data = crate::db::row(
            "aml_ingestion_failures",
            json!({"failure_id":failure_id,"block_number":height,"block_hash":null,"operation":stage,"status":if retryable{"retry"}else{"dead_letter"},"attempts":attempts,"error_code":"rpc_or_decode","error_message":message.chars().take(4096).collect::<String>(),"next_attempt_unix_ms":0,"tx_hash":null,"retryable":u8::from(retryable),"first_failed_at_unix_ms":previous.as_ref().and_then(|r|r["first_failed_at_unix_ms"].as_u64()).unwrap_or(time),"last_failed_at_unix_ms":time,"resolved_at_unix_ms":null,"raw_evidence_json":json!({"stage":stage}).to_string()}),
            self.client.revision("aml_ingestion_failures").await?,
            time,
        )?;
        self.client
            .insert_rows("aml_ingestion_failures", &[data])
            .await
    }
    pub async fn resolve_failure(&self, height: u64, stage: &str) -> Result<()> {
        let failures = self
            .client
            .rows(
                "aml_ingestion_failures",
                &format!(
                    "block_number={height} AND operation={} AND status!='resolved'",
                    publication::literal(stage)
                ),
            )
            .await?;
        for mut row in failures {
            row["status"] = json!("resolved");
            row["resolved_at_unix_ms"] = json!(now_ms());
            row["next_attempt_unix_ms"] = json!(0);
            row["revision"] = json!(self.client.revision("aml_ingestion_failures").await?);
            self.client
                .insert_rows("aml_ingestion_failures", &[row])
                .await?;
        }
        Ok(())
    }
}
pub fn ingestion_lock(database: &str) -> Result<std::fs::File> {
    aml_warehouse::contract::identifier(database)?;
    let directory = std::env::var_os("ETHEREUM_WRITER_LOCK_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("aml-ethereum-writer"));
    std::fs::create_dir_all(&directory)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(format!("{database}.lock")))?;
    file.try_lock()
        .context("another Ethereum ingest/range process owns this database")?;
    Ok(file)
}
