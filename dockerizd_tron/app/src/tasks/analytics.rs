use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use chrono::Utc;
use clickhouse::Client;
use serde::{Deserialize, Serialize};

use crate::{
    config::AppConfig,
    db::{sync_state::get_last_synced_block, tron_schema::validate_tron_schema},
    services::tron::address_clustering::discover_address_cluster_claims,
    services::tron::entity_intelligence::{IntelligenceSourceInput, register_intelligence_source},
    tasks::exposure_task::run_all_exposure_scans,
};

#[derive(Debug)]
pub struct Settings {
    pub directory: PathBuf,
    pub interval: u64,
    pub exposure_interval: u64,
    pub batch_blocks: u64,
    pub overlap_blocks: u64,
    pub max_claims: usize,
    pub max_hops: u8,
}

fn setting(name: &str, default: u64, min: u64, max: u64) -> Result<u64> {
    let value = match std::env::var(name) {
        Ok(value) => value.parse().with_context(|| format!("invalid {name}"))?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    ensure!(
        (min..=max).contains(&value),
        "{name} must be in {min}..={max}"
    );
    Ok(value)
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            directory: std::env::var_os("TRON_ANALYTICS_STATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("tron-analytics")),
            interval: setting("TRON_ANALYTICS_INTERVAL_SECONDS", 60, 10, 86400)?,
            exposure_interval: setting(
                "TRON_ANALYTICS_EXPOSURE_INTERVAL_SECONDS",
                3600,
                60,
                86400,
            )?,
            batch_blocks: setting("TRON_ANALYTICS_BATCH_BLOCKS", 100_000, 1, 1_000_000)?,
            overlap_blocks: setting("TRON_ANALYTICS_OVERLAP_BLOCKS", 10_000, 0, 1_000_000)?,
            max_claims: setting("TRON_ANALYTICS_MAX_CLAIMS", 5000, 1, 50_000)? as usize,
            max_hops: setting("TRON_EXPOSURE_MAX_HOPS", 5, 1, 10)? as u8,
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Progress {
    last_cluster_block: Option<u64>,
    last_cluster_hash: Option<String>,
    last_exposure_unix_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct Status {
    phase: String,
    updated_at_unix_ms: u64,
    detail: serde_json::Value,
}

fn now_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)?;
    serde_json::to_writer(&mut file, value)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(temporary, path)?;
    Ok(())
}

fn status(settings: &Settings, phase: &str, detail: serde_json::Value) -> Result<()> {
    atomic_json(
        &settings.directory.join("status.json"),
        &Status {
            phase: phase.into(),
            updated_at_unix_ms: now_ms(),
            detail,
        },
    )
}

pub fn read_status(settings: &Settings, require_healthy: bool) -> Result<String> {
    let raw = std::fs::read_to_string(settings.directory.join("status.json"))?;
    let value: Status = serde_json::from_str(&raw)?;
    if require_healthy {
        ensure!(
            value.phase != "FAILED",
            "analytics stage failed; inspect worker logs"
        );
        ensure!(
            now_ms().saturating_sub(value.updated_at_unix_ms) <= 120_000,
            "analytics heartbeat is stale"
        );
    }
    Ok(raw)
}

// Serial stages avoid overlapping runs. A heartbeat distinguishes a long query from a stuck worker.
async fn monitored<T>(
    settings: &Settings,
    phase: &str,
    detail: serde_json::Value,
    task: impl Future<Output = Result<T>>,
) -> Result<T> {
    let task = tokio::time::timeout(Duration::from_secs(1800), task);
    tokio::pin!(task);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    status(settings, phase, detail)?;
    loop {
        tokio::select! {
            result = &mut task => return result.context("analytics stage exceeded 30 minutes")?,
            _ = heartbeat.tick() => {
                let mut current: Status = serde_json::from_str(&read_status(settings, false)?)?;
                current.updated_at_unix_ms = now_ms();
                atomic_json(&settings.directory.join("status.json"), &current)?;
            },
        }
    }
}

fn cluster_range(last: Option<u64>, tip: u64, batch: u64, overlap: u64) -> (u64, u64) {
    let next = last.map_or(0, |n| n.saturating_add(1));
    if next > tip {
        (tip.saturating_sub(batch.saturating_sub(1)), tip)
    } else {
        (
            next.saturating_sub(overlap),
            next.saturating_add(batch.saturating_sub(1)).min(tip),
        )
    }
}

async fn block_hash(client: &Client, block: u64) -> Result<Option<String>> {
    Ok(client.query("SELECT block_hash FROM ingested_blocks FINAL WHERE chain='tron' AND block_number=? AND ingestion_status='COMPLETE'")
        .bind(block).fetch_optional::<String>().await?)
}

async fn ensure_internal_source(client: &Client) -> Result<()> {
    const SOURCE: &str = "tron_structural_heuristics_v1";
    let existing = client
        .query(
            "SELECT is_active FROM intelligence_sources FINAL WHERE chain='tron' AND source_id=?",
        )
        .bind(SOURCE)
        .fetch_optional::<u8>()
        .await?;
    match existing {
        Some(0) => anyhow::bail!(
            "internal discovery source is disabled by the analyst; not reactivating it"
        ),
        Some(_) => Ok(()),
        None => {
            register_intelligence_source(
                client,
                IntelligenceSourceInput {
                    source_id: SOURCE.into(),
                    source_name: "TRON structural candidate discovery".into(),
                    source_type: "HEURISTIC".into(),
                    trust_tier: "UNVERIFIED".into(),
                    reference_url: String::new(),
                    license: "internal".into(),
                    is_active: true,
                    created_by: "tron_analytics_worker".into(),
                },
            )
            .await?;
            Ok(())
        }
    }
}

async fn cycle(settings: &Settings, client: Arc<Client>, progress: &mut Progress) -> Result<()> {
    ensure_internal_source(&client).await?;
    let Some(tip) = get_last_synced_block(&client).await? else {
        status(
            settings,
            "WAITING_FOR_DATA",
            serde_json::json!({"reason":"no ingestion checkpoint"}),
        )?;
        println!("[TRON ANALYTICS] waiting for the first committed ingestion checkpoint");
        return Ok(());
    };
    if let Some(last) = progress.last_cluster_block {
        if last > tip || block_hash(&client, last).await? != progress.last_cluster_hash {
            // A replaced checkpoint invalidates the discovery cursor, not the underlying evidence.
            *progress = Progress::default();
            println!("[TRON ANALYTICS] canonical checkpoint changed; restarting discovery scan");
        }
    }
    let (start, end) = cluster_range(
        progress.last_cluster_block,
        tip,
        settings.batch_blocks,
        settings.overlap_blocks,
    );
    let hash = block_hash(&client, end)
        .await?
        .context("analysis boundary is not a complete block")?;
    let detail = serde_json::json!({"start_block":start,"end_block":end,"ingestion_tip":tip,
        "scope":"bounded_candidate_discovery","max_claims":settings.max_claims});
    let report = monitored(
        settings,
        "CLUSTERING",
        detail,
        discover_address_cluster_claims(&client, start, end, settings.max_claims),
    )
    .await?;
    ensure!(
        block_hash(&client, end).await?.as_ref() == Some(&hash),
        "analysis boundary changed during discovery; cursor not advanced"
    );
    progress.last_cluster_block = Some(end);
    progress.last_cluster_hash = Some(hash);
    atomic_json(&settings.directory.join("progress.json"), progress)?;
    println!(
        "[TRON ANALYTICS] clustering {}",
        serde_json::to_string(&report)?
    );

    if now_ms().saturating_sub(progress.last_exposure_unix_ms) >= settings.exposure_interval * 1000
    {
        let (seeds, rows) = monitored(
            settings,
            "EXPOSURE",
            serde_json::json!({"max_hops":settings.max_hops}),
            run_all_exposure_scans(client, settings.max_hops),
        )
        .await?;
        progress.last_exposure_unix_ms = now_ms();
        atomic_json(&settings.directory.join("progress.json"), progress)?;
        println!(
            "[TRON ANALYTICS] exposure completed seeds={seeds} rows={rows}; zero seeds means unavailable label coverage, not clean wallets"
        );
    }
    status(
        settings,
        "IDLE",
        serde_json::json!({"progress":progress,"clustering":report,
        "ingestion_tip":tip,"pending_review_required":true}),
    )?;
    Ok(())
}

pub async fn run(settings: Settings, once: bool) -> Result<()> {
    std::fs::create_dir_all(&settings.directory)?;
    let path = settings.directory.join("progress.json");
    let mut progress = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .context("invalid analytics progress; refusing to silently reset")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Progress::default(),
        Err(error) => return Err(error.into()),
    };
    let config = AppConfig::from_env();
    let client = Arc::new(
        Client::default()
            .with_url(&config.clickhouse_url)
            .with_user(&config.clickhouse_user)
            .with_password(&config.clickhouse_pass)
            .with_database(&config.clickhouse_db_tron)
            .with_option("join_algorithm", "auto")
            .with_option("max_execution_time", "120")
            .with_option("max_memory_usage", "536870912"),
    );
    monitored(
        &settings,
        "VALIDATING_SCHEMA",
        serde_json::json!({}),
        validate_tron_schema(&client),
    )
    .await?;
    loop {
        let outcome = monitored(
            &settings,
            "RUNNING",
            serde_json::json!({}),
            cycle(&settings, client.clone(), &mut progress),
        )
        .await;
        if let Err(error) = outcome {
            status(
                &settings,
                "FAILED",
                serde_json::json!({"error":format!("{error:#}")}),
            )?;
            // Docker restarts a failed worker; the last successful stage remains checkpointed.
            return Err(error);
        }
        if once {
            return Ok(());
        }
        for _ in 0..settings.interval.div_ceil(10) {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let mut current: Status = serde_json::from_str(&read_status(&settings, false)?)?;
            current.updated_at_unix_ms = now_ms();
            atomic_json(&settings.directory.join("status.json"), &current)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_advances_from_genesis_in_overlapping_batches() {
        assert_eq!(cluster_range(None, 250, 100, 10), (0, 99));
        assert_eq!(cluster_range(Some(99), 250, 100, 10), (90, 199));
        assert_eq!(cluster_range(Some(199), 250, 100, 10), (190, 250));
    }
    #[test]
    fn caught_up_worker_refreshes_recent_history_instead_of_advancing_past_tip() {
        assert_eq!(cluster_range(Some(250), 250, 100, 10), (151, 250));
        assert_eq!(cluster_range(None, 0, 100, 10), (0, 0));
        assert_eq!(cluster_range(Some(0), 0, 100, 10), (0, 0));
    }
    #[test]
    fn progress_round_trip_preserves_resume_and_exposure_time() {
        let value = Progress {
            last_cluster_block: Some(0),
            last_cluster_hash: Some("hash".into()),
            last_exposure_unix_ms: 42,
        };
        let restored: Progress =
            serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(restored.last_cluster_block, Some(0));
        assert_eq!(restored.last_exposure_unix_ms, 42);
        assert!(serde_json::from_str::<Progress>("broken").is_err());
    }
}
