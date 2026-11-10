//! Bounded operator access using the existing Bitcoin resolver and publication writer.
use crate::{
    config::{DatabaseConfig, IngestConfig, PipelineConfig},
    db::{Database, warehouse::BlockHeader},
    failures, repair, resolver,
    rpc::RpcPool,
    writer_lock::WriterLock,
};
use aml_warehouse::{
    Network,
    maintenance::{self, Command},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
pub async fn run(command: &Command) -> Result<Value> {
    if command.action == "probe" {
        let rpc = crate::rpc::Rpc::probe_from_env()?;
        let config = IngestConfig::from_env()?;
        return Ok(serde_json::to_value(
            crate::probe::inspect(&rpc, &config).await,
        )?);
    }
    let db = Database::new(DatabaseConfig::from_env()?)?;
    if !command.mutates() {
        return maintenance::read(
            &db.warehouse_connection(),
            db.database(),
            Network::Bitcoin,
            command,
            false,
        )
        .await;
    }
    let owner = WriterLock::for_database(db.database())?;
    let config = IngestConfig::from_env()?;
    let tuning = PipelineConfig::from_env()?;
    let pool = RpcPool::from_env()?;
    let policy = failures::Policy::from_env()?;
    maintenance::run(
        &db.warehouse_connection(),
        db.database(),
        Network::Bitcoin,
        command,
        false,
        config.start,
        async |height, historical| {
            if !historical {
                repair::run_owned(
                    db.clone(),
                    pool.clone(),
                    config.clone(),
                    tuning.clone(),
                    repair::Options {
                        ids: vec![failures::id(height)],
                        max_blocks: 1,
                        policy: policy.clone(),
                    },
                    &owner,
                )
                .await?;
                return Ok(());
            }
            let rpc = pool.endpoints.first().context("missing Bitcoin RPC")?;
            rpc.validate_chain().await?;
            let head = rpc.height().await?;
            ensure!(
                aml_warehouse::visibility::confirmed_tip(head, config.confirmations)
                    .is_some_and(|h| height <= h),
                "replay height lacks required confirmation depth"
            );
            let hash = rpc.block_hash(height).await?;
            let block = rpc.block(&hash, config.verbosity).await?;
            ensure!(
                block["height"] == height && block["hash"] == hash,
                "Bitcoin replay identity mismatch"
            );
            let resolved = resolver::resolve(rpc, &db, &block, &tuning).await?;
            ensure!(
                rpc.block_hash(height).await? == hash,
                "Bitcoin chain changed during replay"
            );
            let evidence = crate::model::extract(&block, &format!("rpc-{hash}"), &resolved)?;
            let old = db.state().await?;
            ensure!(
                rpc.block_hash(old.tip_height).await? == old.tip_hash,
                "Bitcoin published checkpoint changed; reconcile before replay"
            );
            crate::storage::writer::write_block_mode(
                &db,
                &old,
                &BlockHeader {
                    block_height: height,
                    block_hash: hash.clone(),
                    parent_hash: block["previousblockhash"]
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    block_time: block["time"].as_u64().context("missing replay timestamp")?,
                    commit_id: format!("rpc-{hash}"),
                    transaction_count: u32::try_from(evidence.transactions.len())?,
                },
                &evidence,
                crate::storage::writer::WriteContext {
                    observed: head,
                    start: config.start,
                    source_id: "bitcoin_core_rpc",
                },
                true,
            )
            .await?;
            Ok(())
        },
    )
    .await
}
