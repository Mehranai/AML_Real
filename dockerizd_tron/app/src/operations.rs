//! Operator commands executed in the existing TRON image and writer boundary.
use crate::{
    config::AppConfig,
    db::Client,
    helper::tron::TronClient,
    services::{
        loader::LoaderTron,
        tron::{fetcher, publication_store, source_evidence::VerifiedTransaction},
    },
};
use aml_warehouse::{
    Network,
    maintenance::{self, Command},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::Arc;

pub async fn probe(config: &AppConfig) -> Result<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(120), probe_inner(config))
        .await
        .context("TRON probe exceeded 120 seconds")?
}
async fn probe_inner(config: &AppConfig) -> Result<Value> {
    let rpc = TronClient::new(
        config
            .tron_rpc_url
            .as_deref()
            .context("TRON_RPC_URL required")?,
        config.tron_api_key.clone(),
        config.rpc_timeout_seconds,
    )?;
    rpc.validate_mainnet().await?;
    let latest = rpc.get_block_number().await?;
    let solid = rpc.get_solid_block_number().await?;
    ensure!(
        solid <= latest && config.tron_start_block <= solid,
        "TRON solid/start height is inconsistent with the RPC head"
    );
    let mut samples = vec![];
    let mut proven = false;
    for height in [config.tron_start_block, solid]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let block = rpc.get_block(height).await?;
        let header = publication_store::block_header(&block, height, 1)?;
        let txs = block["transactions"].as_array();
        let mut count = 0;
        let mut genesis_count = 0;
        for tx in txs.into_iter().flatten().take(8) {
            if height == 0 {
                VerifiedTransaction::genesis(tx, header.block_timestamp_unix_ms)?;
                genesis_count += 1;
                continue;
            }
            let id = tx["txID"]
                .as_str()
                .context("missing probe transaction ID")?;
            let receipt = rpc.get_tx_receipt(id).await?;
            VerifiedTransaction::verify(tx, &receipt, height, header.block_timestamp_unix_ms)?;
            count += 1;
        }
        let checked = publication_store::block_header(&rpc.get_block(height).await?, height, 1)?;
        ensure!(
            checked.block_hash == header.block_hash,
            "TRON sample changed during probe"
        );
        proven |= count > 0;
        samples.push(json!({"height":height,"hash":header.block_hash,"transactions":header.transaction_count,"receipts_verified":count,"genesis_allocations_verified":genesis_count,"receipt_sample_limit":8}));
    }
    Ok(
        json!({"contract":"chain_rpc_probe_v1","network_id":Network::Tron.id(),"status":if proven{"ready"}else{"degraded"},"mainnet_genesis_verified":true,
        "latest_block":latest,"solid_block":solid,"configured_start":config.tron_start_block,"samples":samples,"full_history_verified":false,
        "receipt_coverage":if proven{"bounded_source_samples"}else{"unproven_empty_samples"},"logs":"receipt_records","execution_traces":"not_applicable","local_database_checked":false}),
    )
}
pub async fn run(config: AppConfig, command: &Command) -> Result<Value> {
    if command.action == "probe" {
        return probe(&config).await;
    }
    let db = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass)
        .with_database(&config.clickhouse_db_tron);
    if !command.mutates() {
        return maintenance::read(&db, db.database(), Network::Tron, command, false).await;
    }
    let owner = publication_store::writer_lock(db.database())?;
    let loader = Arc::new(LoaderTron::new(&config).await?);
    maintenance::run(
        &db,
        db.database(),
        Network::Tron,
        command,
        false,
        config.tron_start_block,
        async |height, historical| {
            fetcher::maintenance_block(loader.clone(), height, historical, &owner).await
        },
    )
    .await
}
