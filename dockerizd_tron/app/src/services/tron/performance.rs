use std::sync::Arc;
use std::time::Instant;

use crate::db::Client;
use anyhow::{Context, Result};
use chrono::Utc;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::models::tron::modules::IngestionBenchmarkRow;
use crate::services::loader::LoaderTron;
use crate::services::tron::fetcher::ingest_tron_range;
use crate::services::tron::wallet_investigation::{
    WalletInvestigationOptions, build_wallet_investigation,
};

const ZERO_ADDRESS: &str = "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb";

#[derive(Debug, Clone, Deserialize, Serialize, clickhouse::Row)]
pub struct StorageTableMetric {
    pub table: String,
    pub row_count: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub active_parts: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageSnapshot {
    pub row_count: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub active_parts: u64,
    pub tables: Vec<StorageTableMetric>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoricalBenchmarkReport {
    pub run_id: String,
    pub source_kind: String,
    pub start_block: u64,
    pub end_block: u64,
    pub requested_blocks: u32,
    pub completed_blocks: u32,
    pub transaction_count: u64,
    pub elapsed_ms: u64,
    pub blocks_per_second: f64,
    pub transactions_per_second: f64,
    pub storage_before: StorageSnapshot,
    pub storage_after: StorageSnapshot,
    pub storage_row_delta: u64,
    pub compressed_byte_delta: u64,
    pub investigation_address: String,
    pub investigation_latency_ms: u64,
    pub investigation_error: String,
    pub status: String,
    pub error_message: String,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct CompletedRangeSummary {
    completed_blocks: u64,
    transaction_count: u64,
}

pub async fn run_historical_benchmark(
    config: &AppConfig,
    loader: Arc<LoaderTron>,
    start_block: u64,
    end_block: u64,
    investigation_address: Option<String>,
) -> Result<HistoricalBenchmarkReport> {
    let requested_blocks = end_block
        .checked_sub(start_block)
        .and_then(|difference| difference.checked_add(1))
        .and_then(|count| u32::try_from(count).ok())
        .context("invalid historical benchmark range")?;
    let run_id = format!("tron-benchmark-{}-{}", now_unix_ms(), nanoid!(8));
    let source_kind = source_kind(config.tron_rpc_url.as_deref().unwrap_or_default());
    let started_at_unix_ms = now_unix_ms();
    let storage_before = load_storage_snapshot(&loader.clickhouse, &config.clickhouse_db_tron)
        .await
        .context("failed to load pre-benchmark storage metrics")?;

    persist_benchmark(
        &loader.clickhouse,
        &IngestionBenchmarkRow {
            run_id: run_id.clone(),
            chain: "tron".to_string(),
            source_kind: source_kind.clone(),
            start_block,
            end_block,
            requested_blocks,
            completed_blocks: 0,
            transaction_count: 0,
            elapsed_ms: 0,
            blocks_per_second: 0.0,
            transactions_per_second: 0.0,
            rows_before: storage_before.row_count,
            rows_after: storage_before.row_count,
            compressed_bytes_before: storage_before.compressed_bytes,
            compressed_bytes_after: storage_before.compressed_bytes,
            investigation_address: investigation_address.clone().unwrap_or_default(),
            investigation_latency_ms: 0,
            status: "RUNNING".to_string(),
            error_message: String::new(),
            metrics_json: "{}".to_string(),
            started_at_unix_ms,
            completed_at_unix_ms: 0,
        },
    )
    .await?;

    let ingestion_started = Instant::now();
    let ingestion_result = ingest_tron_range(loader.clone(), start_block, end_block).await;
    let elapsed_ms = ingestion_started.elapsed().as_millis() as u64;
    let range_summary = load_completed_range_summary(
        &loader.clickhouse,
        start_block,
        end_block,
        started_at_unix_ms,
    )
    .await?;
    let storage_after = load_storage_snapshot(&loader.clickhouse, &config.clickhouse_db_tron)
        .await
        .context("failed to load post-benchmark storage metrics")?;

    let (investigation_address, investigation_latency_ms, investigation_error) =
        if ingestion_result.is_ok() {
            benchmark_investigation(
                loader.clickhouse.clone(),
                start_block,
                end_block,
                investigation_address,
            )
            .await
        } else {
            (String::new(), 0, "ingestion_failed".to_string())
        };

    let elapsed_seconds = (elapsed_ms.max(1) as f64) / 1_000.0;
    let completed_blocks = u32::try_from(range_summary.completed_blocks).unwrap_or(u32::MAX);
    let error_message = ingestion_result
        .as_ref()
        .err()
        .map(|error| truncate_error(&format!("{error:#}")))
        .unwrap_or_default();
    let status = if ingestion_result.is_err() {
        "FAILED"
    } else if !investigation_error.is_empty() {
        "COMPLETE_WITH_INVESTIGATION_ERROR"
    } else if completed_blocks == 0 {
        "NOOP"
    } else {
        "COMPLETE"
    }
    .to_string();
    let completed_at_unix_ms = now_unix_ms();
    let storage_row_delta = storage_after
        .row_count
        .saturating_sub(storage_before.row_count);
    let compressed_byte_delta = storage_after
        .compressed_bytes
        .saturating_sub(storage_before.compressed_bytes);

    let mut report = HistoricalBenchmarkReport {
        run_id: run_id.clone(),
        source_kind: source_kind.clone(),
        start_block,
        end_block,
        requested_blocks,
        completed_blocks,
        transaction_count: range_summary.transaction_count,
        elapsed_ms,
        blocks_per_second: f64::from(completed_blocks) / elapsed_seconds,
        transactions_per_second: range_summary.transaction_count as f64 / elapsed_seconds,
        storage_before,
        storage_after,
        storage_row_delta,
        compressed_byte_delta,
        investigation_address,
        investigation_latency_ms,
        investigation_error,
        status,
        error_message,
        started_at_unix_ms,
        completed_at_unix_ms,
    };

    let metrics_json = serde_json::to_string(&report)?;
    persist_benchmark(
        &loader.clickhouse,
        &IngestionBenchmarkRow {
            run_id,
            chain: "tron".to_string(),
            source_kind,
            start_block,
            end_block,
            requested_blocks,
            completed_blocks,
            transaction_count: report.transaction_count,
            elapsed_ms,
            blocks_per_second: report.blocks_per_second,
            transactions_per_second: report.transactions_per_second,
            rows_before: report.storage_before.row_count,
            rows_after: report.storage_after.row_count,
            compressed_bytes_before: report.storage_before.compressed_bytes,
            compressed_bytes_after: report.storage_after.compressed_bytes,
            investigation_address: report.investigation_address.clone(),
            investigation_latency_ms,
            status: report.status.clone(),
            error_message: report.error_message.clone(),
            metrics_json,
            started_at_unix_ms,
            completed_at_unix_ms,
        },
    )
    .await?;

    if let Err(error) = ingestion_result {
        report.error_message = truncate_error(&format!("{error:#}"));
    }

    Ok(report)
}

pub async fn load_storage_snapshot(clickhouse: &Client, database: &str) -> Result<StorageSnapshot> {
    let tables = clickhouse
        .query(
            r#"
            SELECT
                table,
                sum(rows) AS row_count,
                sum(data_compressed_bytes) AS compressed_bytes,
                sum(data_uncompressed_bytes) AS uncompressed_bytes,
                count() AS active_parts
            FROM system.parts
            WHERE database = ?
              AND active
              AND table IN (
                  'aml_transactions',
                  'aml_address_relationships',
                  'aml_transaction_features',
                  'aml_semantic_aml_events',
                  'aml_tron_contracts','aml_tron_logs','aml_tron_internals','aml_tron_resources',
                  'aml_ingested_blocks',
                  'aml_ingestion_failures'
              )
            GROUP BY table
            ORDER BY table
            "#,
        )
        .bind(database)
        .fetch_all::<StorageTableMetric>()
        .await?;

    Ok(StorageSnapshot {
        row_count: tables.iter().map(|metric| metric.row_count).sum(),
        compressed_bytes: tables.iter().map(|metric| metric.compressed_bytes).sum(),
        uncompressed_bytes: tables.iter().map(|metric| metric.uncompressed_bytes).sum(),
        active_parts: tables.iter().map(|metric| metric.active_parts).sum(),
        tables,
    })
}

async fn load_completed_range_summary(
    clickhouse: &Client,
    start_block: u64,
    end_block: u64,
    started_at_unix_ms: u64,
) -> Result<CompletedRangeSummary> {
    clickhouse
        .query(
            r#"
            SELECT
                countIf(
                    ingestion_status = 'COMPLETE'
                    AND indexed_at_unix_ms >= ?
                ) AS completed_blocks,
                sumIf(
                    toUInt64(transaction_count),
                    ingestion_status = 'COMPLETE'
                    AND indexed_at_unix_ms >= ?
                ) AS transaction_count
            FROM ingested_blocks FINAL
            WHERE chain = 'tron'
              AND block_number BETWEEN ? AND ?
            "#,
        )
        .bind(started_at_unix_ms)
        .bind(started_at_unix_ms)
        .bind(start_block)
        .bind(end_block)
        .fetch_one::<CompletedRangeSummary>()
        .await
        .context("failed to summarize benchmark block range")
}

async fn benchmark_investigation(
    clickhouse: Arc<Client>,
    start_block: u64,
    end_block: u64,
    requested_address: Option<String>,
) -> (String, u64, String) {
    let address = match requested_address {
        Some(address) => address,
        None => match load_busiest_wallet(&clickhouse, start_block, end_block).await {
            Ok(Some(address)) => address,
            Ok(None) => return (String::new(), 0, String::new()),
            Err(error) => return (String::new(), 0, truncate_error(&format!("{error:#}"))),
        },
    };
    let started = Instant::now();
    let result = build_wallet_investigation(
        clickhouse,
        &address,
        WalletInvestigationOptions::new(
            Some(2),
            Some(200),
            Some(90),
            Some(25),
            Some(1_000),
            Some(50),
        ),
    )
    .await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    match result {
        Ok(_) => (address, elapsed_ms, String::new()),
        Err(error) => (address, elapsed_ms, truncate_error(&format!("{error:#}"))),
    }
}

async fn load_busiest_wallet(
    clickhouse: &Client,
    start_block: u64,
    end_block: u64,
) -> Result<Option<String>> {
    clickhouse
        .query(
            r#"
            SELECT address
            FROM
            (
                SELECT from_address AS address
                FROM address_relationships
                WHERE block_number BETWEEN ? AND ?
                UNION ALL
                SELECT to_address AS address
                FROM address_relationships
                WHERE block_number BETWEEN ? AND ?
            )
            WHERE address != '' AND address != ?
            GROUP BY address
            ORDER BY count() DESC, address
            LIMIT 1
            "#,
        )
        .bind(start_block)
        .bind(end_block)
        .bind(start_block)
        .bind(end_block)
        .bind(ZERO_ADDRESS)
        .fetch_optional::<String>()
        .await
        .context("failed to select benchmark investigation wallet")
}

async fn persist_benchmark(clickhouse: &Client, row: &IngestionBenchmarkRow) -> Result<()> {
    let now = crate::db::now_ms();
    let data = serde_json::json!({"benchmark_id":row.run_id,"start_block":row.start_block,"end_block":row.end_block,"blocks":row.completed_blocks,"transactions":row.transaction_count,"rows_written":row.rows_after.saturating_sub(row.rows_before),"elapsed_ms":row.elapsed_ms,"bytes_written":row.compressed_bytes_after.saturating_sub(row.compressed_bytes_before),"settings_json":"{}","source_kind":row.source_kind,"status":match row.status.as_str(){"RUNNING"=>"running","COMPLETE"|"NOOP"=>"complete",_=>"failed"},"started_at_unix_ms":row.started_at_unix_ms,"completed_at_unix_ms":if row.completed_at_unix_ms==0{None}else{Some(row.completed_at_unix_ms)},"metrics_json":row.metrics_json,"error_message":row.error_message,"raw_evidence_json":serde_json::to_string(row)?,"blocks_per_second":row.blocks_per_second,"transactions_per_second":row.transactions_per_second,"rows_per_second":if row.elapsed_ms==0{0.0}else{row.rows_after.saturating_sub(row.rows_before) as f64*1000.0/row.elapsed_ms as f64}});
    clickhouse
        .insert_rows(
            "aml_ingestion_benchmarks",
            &[crate::db::row(
                "aml_ingestion_benchmarks",
                data,
                clickhouse.revision("aml_ingestion_benchmarks").await?,
                now,
            )?],
        )
        .await
}

fn source_kind(rpc_url: &str) -> String {
    let rpc_url = rpc_url.to_ascii_lowercase();
    if rpc_url.contains("localhost")
        || rpc_url.contains("127.0.0.1")
        || rpc_url.contains("host.docker.internal")
    {
        "local_node".to_string()
    } else {
        "remote_api".to_string()
    }
}

fn truncate_error(error: &str) -> String {
    error.chars().take(4_096).collect()
}

fn now_unix_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_local_and_remote_sources_without_exposing_credentials() {
        assert_eq!(source_kind("http://127.0.0.1:8090"), "local_node");
        assert_eq!(source_kind("https://api.trongrid.io"), "remote_api");
    }
}
