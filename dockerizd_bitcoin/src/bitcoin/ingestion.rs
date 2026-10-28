use crate::{
    config::{IngestConfig, PipelineConfig},
    db::{Database, SyncState, timestamp},
    failures::{self, Policy, Stage},
    fetch,
    model::{self, ScriptOutput},
    resolver,
    rpc::{Rpc, RpcPool},
    warehouse,
    writer_lock::WriterLock,
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{collections::HashMap, time::Duration};
use tokio::time::Instant;

// Extract first, write all evidence, then publish the visibility fence. A failure
// at any preceding step leaves the old checkpoint and API snapshot intact.
pub async fn commit(
    db: &Database,
    old: &SyncState,
    block: &Value,
    resolved: &HashMap<String, ScriptOutput>,
    observed: u64,
    start: u64,
) -> Result<SyncState> {
    let height = block["height"].as_u64().context("missing block height")?;
    ensure!(
        height
            == if old.initialized == 1 {
                old.tip_height + 1
            } else {
                start
            },
        "non-contiguous block ingestion"
    );
    if old.initialized == 1 {
        ensure!(
            block["previousblockhash"].as_str() == Some(&old.tip_hash),
            "block parent changed"
        );
    }
    let commit_id = format!("rpc-{}", block["hash"].as_str().context("missing hash")?);
    let evidence = model::extract(block, &commit_id, resolved)?;
    warehouse::write_block(
        db,
        old,
        &warehouse::LegacyBlock {
            block_height: height,
            block_hash: block["hash"].as_str().unwrap().into(),
            parent_hash: block["previousblockhash"].as_str().unwrap_or("").into(),
            block_time: block["time"].as_u64().context("missing block timestamp")?,
            commit_id,
            transaction_count: u32::try_from(evidence.transactions.len())?,
        },
        &evidence,
        warehouse::WriteContext {
            observed,
            start,
            source_id: "bitcoin_core_rpc",
            legacy_fence: None,
        },
    )
    .await
}

pub async fn reconcile(
    db: &Database,
    rpc: &Rpc,
    state: &SyncState,
    config: &IngestConfig,
    head: u64,
) -> Result<SyncState> {
    if state.initialized == 0 {
        return Ok(state.clone());
    }
    if state.tip_height <= head && rpc.block_hash(state.tip_height).await? == state.tip_hash {
        return Ok(state.clone());
    }
    let floor = state
        .tip_height
        .saturating_sub(config.reorg_depth)
        .max(state.start_height);
    let mut ancestor = None;
    for height in (floor..=state.tip_height.min(head)).rev() {
        if db.block_hash(height).await?.as_deref() == Some(&rpc.block_hash(height).await?) {
            ancestor = Some(height);
            break;
        }
    }
    // If the first ingested block itself was orphaned, the whole observed window
    // can be reset only when it fits inside the configured rollback bound.
    ensure!(
        ancestor.is_some() || state.tip_height - state.start_height < config.reorg_depth,
        "reorg exceeds BITCOIN_REORG_MAX_DEPTH; operator recovery required"
    );
    let mut next = state.clone();
    next.revision = db.next_revision(state.revision).await?;
    next.initialized = u8::from(ancestor.is_some());
    next.tip_height = ancestor.unwrap_or(0);
    next.tip_hash = if let Some(height) = ancestor {
        rpc.block_hash(height).await?
    } else {
        String::new()
    };
    next.observed_rpc_height = head;
    next.published_at = timestamp();
    db.publish_after(state, &next).await?;
    eprintln!("Bitcoin reorg: visible tip reset to {:?}", ancestor);
    Ok(next)
}

pub async fn run(db: Database, rpc: Rpc, config: IngestConfig, once: bool) -> Result<()> {
    run_pool(
        db,
        RpcPool::single(rpc),
        config,
        PipelineConfig::default(),
        once,
    )
    .await
}

pub async fn run_pool(
    db: Database,
    pool: RpcPool,
    config: IngestConfig,
    tuning: PipelineConfig,
    once: bool,
) -> Result<()> {
    run_with_policy(db, pool, config, tuning, Policy::default(), once).await
}

pub async fn run_with_policy(
    db: Database,
    pool: RpcPool,
    config: IngestConfig,
    tuning: PipelineConfig,
    policy: Policy,
    once: bool,
) -> Result<()> {
    let owner = WriterLock::for_database(db.database())?;
    run_owned(
        db,
        pool,
        config,
        tuning,
        RunOptions {
            once,
            repair_height: None,
            policy,
        },
        &owner,
    )
    .await
}

pub(crate) struct RunOptions {
    pub once: bool,
    pub repair_height: Option<u64>,
    pub policy: Policy,
}

pub(crate) async fn run_owned(
    db: Database,
    pool: RpcPool,
    config: IngestConfig,
    tuning: PipelineConfig,
    options: RunOptions,
    _owner: &WriterLock,
) -> Result<()> {
    let once = options.once;
    let forced = options.repair_height;
    let policy = &options.policy;
    policy.validate()?;
    tuning.validate()?;
    ensure!(
        (1..=1000).contains(&config.confirmations)
            && matches!(config.verbosity, 2 | 3)
            && config.start <= u32::MAX as u64
            && config
                .end
                .is_none_or(|end| end >= config.start && end <= u32::MAX as u64),
        "invalid ingestion range"
    );
    db.ready().await?;
    warehouse::migration_ready(&db).await?;
    if forced.is_none() {
        failures::recover_interrupted(&db, policy).await?;
        failures::settle_published(&db, &db.state().await?).await?;
    }
    let mut active = 0;
    let mut retry_primary_at = Instant::now();
    'work: loop {
        if forced.is_none() {
            let state = db.state().await?;
            if config
                .end
                .is_some_and(|end| state.initialized == 1 && state.tip_height >= end)
            {
                return Ok(());
            }
            let next = if state.initialized == 1 {
                state.tip_height + 1
            } else {
                config.start
            };
            if next <= u32::MAX as u64
                && let Some(reason) = failures::gate(&db, next, policy, once).await?
            {
                if once {
                    anyhow::bail!(
                        "ingestion blocked by {reason}; inspect failures and explicitly repair the next block"
                    );
                }
                eprintln!("Bitcoin ingestion waiting on persisted {reason} at height={next}");
                tokio::time::sleep(Duration::from_secs(config.poll_seconds)).await;
                continue 'work;
            }
        }
        let mut order: Vec<usize> = (0..pool.endpoints.len()).collect();
        if active > 0 && Instant::now() < retry_primary_at {
            order.retain(|i| *i != active && *i != 0);
            order.insert(0, active);
            order.push(0);
        }
        let mut last_error = None;
        for index in order {
            // Reread after every failed source attempt: an earlier block may have committed.
            let mut state = db.state().await?;
            ensure!(
                state.revision == 0 || state.start_height == config.start,
                "BITCOIN_START_BLOCK differs from persisted history"
            );
            if config
                .end
                .is_some_and(|end| state.initialized == 1 && state.tip_height >= end)
            {
                return Ok(());
            }
            let rpc = &pool.endpoints[index];
            let source = async {
                rpc.validate_chain().await?;
                let head = rpc.height().await?;
                ensure!(head <= u32::MAX as u64, "RPC height out of range");
                if state.initialized == 1 {
                    ensure!(
                        head >= state.tip_height,
                        "RPC is behind the published checkpoint; refusing rollback"
                    );
                    if index > 0 || forced.is_some() {
                        ensure!(
                            rpc.block_hash(state.tip_height).await? == state.tip_hash,
                            "fallback disagrees with the published checkpoint; refusing rollback"
                        );
                    }
                }
                Ok::<_, anyhow::Error>(head)
            }
            .await;
            let head = match source {
                Ok(head) => head,
                Err(error) => {
                    let next = if state.initialized == 1 {
                        state.tip_height + 1
                    } else {
                        config.start
                    };
                    failures::record(&db, next, None, Stage::Source, policy, forced.is_none())
                        .await?;
                    last_error = Some(error);
                    continue;
                }
            };
            if index == 0 && forced.is_none() {
                state = match reconcile(&db, rpc, &state, &config, head).await {
                    Ok(state) => state,
                    Err(error) => {
                        let next = if state.initialized == 1 {
                            state.tip_height + 1
                        } else {
                            config.start
                        };
                        let _ =
                            failures::record(&db, next, None, Stage::Reconcile, policy, true).await;
                        return Err(error);
                    }
                };
            }
            let next = if state.initialized == 1 {
                state.tip_height + 1
            } else {
                config.start
            };
            ensure!(
                forced.is_none_or(|height| height == next),
                "repair cannot rewind or skip the published checkpoint"
            );
            let end = aml_warehouse::visibility::confirmed_tip(head, config.confirmations)
                .map(|h| config.end.map_or(h, |end| end.min(h)));
            let Some(end) = end.filter(|end| next <= *end) else {
                if once {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_secs(config.poll_seconds)).await;
                continue 'work;
            };
            let blocks = match fetch::blocks(
                rpc,
                next,
                end,
                tuning.block_batch,
                tuning.block_concurrency,
                config.verbosity,
            )
            .await
            {
                Ok(blocks) => blocks,
                Err(error) => {
                    failures::record(&db, next, None, Stage::Fetch, policy, forced.is_none())
                        .await?;
                    last_error = Some(error);
                    continue;
                }
            };
            let mut source_failed = false;
            for block in blocks {
                // A window can contain a later persisted failure. Never let
                // batching bypass the same gate used at the current frontier.
                if forced.is_none() {
                    let height = block["height"].as_u64().context("missing block height")?;
                    if let Some(reason) = failures::gate(&db, height, policy, once).await? {
                        if once {
                            anyhow::bail!(
                                "ingestion blocked by {reason} at height={height}; explicit repair required"
                            );
                        }
                        tokio::time::sleep(Duration::from_secs(config.poll_seconds)).await;
                        continue 'work;
                    }
                }
                let preparation = async {
                    if state.initialized == 1 {
                        ensure!(
                            block["previousblockhash"].as_str() == Some(&state.tip_hash),
                            "block parent changed"
                        );
                    }
                    let resolved = resolver::resolve(rpc, &db, &block, &tuning).await?;
                    let height = block["height"].as_u64().context("missing block height")?;
                    ensure!(
                        rpc.block_hash(height).await?.as_str() == block["hash"].as_str().unwrap(),
                        "chain changed while resolving block"
                    );
                    model::extract(&block, "rpc-validation", &resolved)?;
                    Ok::<_, anyhow::Error>(resolved)
                }
                .await;
                let resolved = match preparation {
                    Ok(v) => v,
                    Err(error) => {
                        failures::record(
                            &db,
                            block["height"].as_u64().context("missing failure height")?,
                            block["hash"].as_str(),
                            Stage::Resolve,
                            policy,
                            forced.is_none(),
                        )
                        .await?;
                        last_error = Some(error);
                        source_failed = true;
                        break;
                    }
                };
                // Database write errors exit; an uncertain publication is never retried blindly.
                state = match commit(&db, &state, &block, &resolved, head, config.start).await {
                    Ok(state) => state,
                    Err(error) => {
                        if failures::record(
                            &db,
                            block["height"].as_u64().unwrap(),
                            block["hash"].as_str(),
                            Stage::Commit,
                            policy,
                            forced.is_none(),
                        )
                        .await
                        .is_err()
                        {
                            eprintln!(
                                "Bitcoin failure persistence unavailable after a database write error; inspect canonical state before retrying"
                            );
                        }
                        return Err(error);
                    }
                };
                failures::resolved(&db, state.tip_height, &state.tip_hash, forced.is_none())
                    .await?;
                eprintln!(
                    "Bitcoin committed height={} transactions={} rpc_source={}",
                    state.tip_height,
                    block["tx"].as_array().unwrap().len(),
                    index
                );
            }
            if source_failed {
                continue;
            }
            if index > 0 && (active != index || Instant::now() >= retry_primary_at) {
                retry_primary_at =
                    Instant::now() + Duration::from_secs(tuning.fallback_cooldown_seconds);
            }
            active = index;
            continue 'work;
        }
        if once {
            return Err(
                last_error.unwrap_or_else(|| anyhow::anyhow!("no usable Bitcoin RPC source"))
            );
        }
        eprintln!("Bitcoin ingestion waiting: no usable RPC source; published checkpoint retained");
        tokio::time::sleep(Duration::from_secs(config.poll_seconds)).await;
    }
}
