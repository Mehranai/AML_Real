use crate::{
    config::{IngestConfig, PipelineConfig},
    db::Database,
    failures::{self, Policy, Stage},
    ingestion::{self, RunOptions},
    rpc::RpcPool,
    warehouse,
    writer_lock::WriterLock,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub async fn scan(db: &Database, from: u64, to: u64, max_blocks: u64) -> Result<Value> {
    let _owner = WriterLock::for_database(db.database())?;
    db.ready().await?;
    warehouse::migration_ready(db).await?;
    failures::scan_gaps(db, from, to, max_blocks).await
}

pub struct Options {
    pub ids: Vec<String>,
    pub max_blocks: u64,
    pub policy: Policy,
}
pub async fn run(
    db: Database,
    pool: RpcPool,
    config: IngestConfig,
    tuning: PipelineConfig,
    options: Options,
) -> Result<Value> {
    let heights = options
        .ids
        .iter()
        .map(|id| failures::height_from_id(id))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !heights.is_empty()
            && heights.len() == options.ids.len()
            && (1..=1000).contains(&options.max_blocks)
            && heights.len() as u64 <= options.max_blocks,
        "select unique failure ids within max-blocks (1..1000)"
    );
    options.policy.validate()?;
    tuning.validate()?;
    let owner = WriterLock::for_database(db.database())?;
    db.ready().await?;
    warehouse::migration_ready(&db).await?;
    failures::recover_interrupted(&db, &options.policy).await?;
    let before = db.state().await?;
    ensure!(
        before.revision == 0 || before.start_height == config.start,
        "BITCOIN_START_BLOCK differs from persisted history"
    );
    // Validate the selection before starting any new attempt.
    for height in &heights {
        ensure!(
            failures::current(&db, *height).await?.is_some(),
            "selected failure does not exist"
        );
    }
    let mut results = Vec::new();
    let mut status = "complete";
    for height in heights {
        let state = db.state().await?;
        if state.initialized == 1 && height <= state.tip_height {
            if failures::healthy(&db, height, &state).await? {
                let hash = db
                    .block_hash(height)
                    .await?
                    .context("missing canonical hash")?;
                failures::resolved(&db, height, &hash, false).await?;
                results
                    .push(json!({"failure_id":failures::id(height),"status":"already_canonical"}));
                continue;
            }
            failures::mark_gap(&db, height, db.block_hash(height).await?).await?;
            results.push(json!({"failure_id":failures::id(height),"status":"blocked","reason":"historical_gap_requires_replay"}));
            status = "blocked";
            break;
        }
        let next = if state.initialized == 1 {
            state.tip_height + 1
        } else {
            config.start
        };
        if height != next {
            results.push(json!({"failure_id":failures::id(height),"status":"blocked","reason":"repair_must_start_at_next_unpublished_block","next_block":next}));
            status = "blocked";
            break;
        }
        failures::begin_repair(&db, height).await?;
        let attempt = ingestion::run_owned(
            db.clone(),
            pool.clone(),
            IngestConfig {
                end: Some(height),
                ..config.clone()
            },
            tuning.clone(),
            RunOptions {
                once: true,
                repair_height: Some(height),
                policy: options.policy.clone(),
            },
            &owner,
        )
        .await;
        let after = db.state().await?;
        if failures::healthy(&db, height, &after).await? {
            failures::resolved(&db, height, &after.tip_hash, false).await?;
            results.push(json!({"failure_id":failures::id(height),"status":"resolved"}));
        } else {
            let row = failures::current(&db, height)
                .await?
                .context("repair failure record disappeared")?;
            if row.status == "repairing" {
                failures::record(&db, height, None, Stage::Source, &options.policy, false).await?;
            }
            results.push(json!({"failure_id":failures::id(height),"status":"failed","reason":if attempt.is_ok() {"no_confirmed_block_available"} else {"attempt_failed"}}));
            status = "failed";
            break;
        }
    }
    let after = db.state().await?;
    Ok(
        json!({"network_id":crate::NETWORK_ID,"status":status,"results":results,"selected":options.ids.len(),
        "checkpoint_changed":before.revision!=after.revision,"last_synced_block":if after.initialized==1 {Some(after.tip_height)} else {None},
        "revision":after.revision.to_string(),"historical_replay_performed":false}),
    )
}

#[derive(Default)]
pub struct Arguments {
    pub query: failures::Query,
    pub ids: Vec<String>,
    pub max_blocks: Option<u64>,
}
impl Arguments {
    pub fn parse(mode: &str, args: &[String]) -> Result<Self> {
        ensure!(args.len().is_multiple_of(2), "tool options require a value");
        let mut parsed = Self::default();
        let mut seen = BTreeSet::new();
        for pair in args.chunks(2) {
            let flag = pair[0].as_str();
            let value = &pair[1];
            ensure!(
                flag == "--failure-id" || seen.insert(flag),
                "duplicate tool option"
            );
            match (mode, flag) {
                ("failures", "--status") => parsed.query.status = Some(value.clone()),
                ("failures" | "scan-gaps", "--from-block") => {
                    parsed.query.from_block = Some(value.parse()?)
                }
                ("failures" | "scan-gaps", "--to-block") => {
                    parsed.query.to_block = Some(value.parse()?)
                }
                ("failures", "--after-block") => parsed.query.after_block = Some(value.parse()?),
                ("failures", "--limit") => parsed.query.limit = Some(value.parse()?),
                ("repair", "--failure-id") => {
                    failures::height_from_id(value)?;
                    parsed.ids.push(value.clone());
                }
                ("repair" | "scan-gaps", "--max-blocks") => {
                    parsed.max_blocks = Some(value.parse()?)
                }
                _ => anyhow::bail!("unsupported tool option"),
            }
        }
        parsed.query.validate()?;
        match mode {
            "repair" => ensure!(
                !parsed.ids.is_empty()
                    && parsed.ids.len() <= 1000
                    && parsed.ids.len() as u64
                        <= parsed.max_blocks.context("repair requires --max-blocks")?
                    && (1..=1000).contains(&parsed.max_blocks.unwrap())
                    && parsed.ids.iter().collect::<BTreeSet<_>>().len() == parsed.ids.len(),
                "repair requires unique --failure-id values and --max-blocks 1..1000"
            ),
            "scan-gaps" => {
                let from = parsed
                    .query
                    .from_block
                    .context("scan-gaps requires --from-block")?;
                let to = parsed
                    .query
                    .to_block
                    .context("scan-gaps requires --to-block")?;
                let max = parsed
                    .max_blocks
                    .context("scan-gaps requires --max-blocks")?;
                ensure!(
                    (1..=10000).contains(&max) && to - from < max,
                    "gap scan exceeds max-blocks (1..10000)"
                );
            }
            "failures" => {}
            _ => anyhow::bail!("unknown failure tool"),
        }
        Ok(parsed)
    }
    pub fn usage(mode: &str) -> &'static str {
        match mode {
            "failures" => {
                "Usage: bitcoin_aml failures [--status retry|dead_letter|repairing|gap|resolved] [--from-block N] [--to-block N] [--after-block N] [--limit 1..200]"
            }
            "repair" => {
                "Usage: bitcoin_aml repair --failure-id bitcoin:block:N [--failure-id bitcoin:block:M] --max-blocks 1..1000 (pause ingestion first)"
            }
            _ => {
                "Usage: bitcoin_aml scan-gaps --from-block N --to-block M --max-blocks 1..10000 (pause ingestion first)"
            }
        }
    }
}
