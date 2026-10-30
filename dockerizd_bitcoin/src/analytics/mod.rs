//! Continuous bounded address sweeps over canonical facts. No attribution imports.
mod checkpoint;
pub mod cli;
mod engine;
#[cfg(test)]
mod tests;
use crate::{NETWORK_ID, config, db::Database};
use anyhow::{Result, ensure};
pub use engine::Worker;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

pub const VERSION: &str = "bitcoin_analytics_worker_v1";
pub const USAGE: &str = "Usage: bitcoin_aml analytics\n       bitcoin_aml analytics once --max-jobs N\n       bitcoin_aml analytics status\n       bitcoin_aml analytics healthcheck\n       bitcoin_aml analytics retry JOB_ID\nUses canonical ClickHouse data only, no RPC. Never imports or approves labels/leads. Requires a persistent BITCOIN_ANALYTICS_STATE_DIR and the shared writer lock directory.";
pub(crate) const MAX_QUEUE: usize = 128;
pub(crate) const MAX_RECENT: usize = 16;
pub(crate) const HEARTBEAT_SECONDS: u64 = 5;

#[derive(Clone, Debug)]
pub struct Config {
    pub directory: PathBuf,
    pub page_size: usize,
    pub poll_seconds: u64,
    pub refresh_seconds: u64,
    pub job_timeout_seconds: u64,
    pub retry_seconds: u64,
    pub max_attempts: u32,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        let c = Self {
            directory: config::required("BITCOIN_ANALYTICS_STATE_DIR")?.into(),
            page_size: config::bounded("BITCOIN_ANALYTICS_PAGE_SIZE", 10, 1, 100)? as usize,
            poll_seconds: config::bounded("BITCOIN_ANALYTICS_POLL_SECONDS", 5, 1, 60)?,
            refresh_seconds: config::bounded("BITCOIN_ANALYTICS_REFRESH_SECONDS", 300, 10, 86400)?,
            job_timeout_seconds: config::bounded(
                "BITCOIN_ANALYTICS_JOB_TIMEOUT_SECONDS",
                120,
                1,
                600,
            )?,
            retry_seconds: config::bounded("BITCOIN_ANALYTICS_RETRY_SECONDS", 30, 1, 3600)?,
            max_attempts: config::bounded("BITCOIN_ANALYTICS_MAX_ATTEMPTS", 3, 1, 20)? as u32,
        };
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.directory.is_absolute(),
            "analytics state directory must be absolute"
        );
        ensure!(
            (1..=100).contains(&self.page_size)
                && (1..=60).contains(&self.poll_seconds)
                && (10..=86400).contains(&self.refresh_seconds)
                && (1..=600).contains(&self.job_timeout_seconds)
                && (1..=3600).contains(&self.retry_seconds)
                && (1..=20).contains(&self.max_attempts),
            "invalid analytics resource limits"
        );
        Ok(())
    }
    pub fn definition(&self) -> Value {
        json!({"worker_version":VERSION,"network_id":NETWORK_ID,"page_size":self.page_size,
            "poll_seconds":self.poll_seconds,"refresh_seconds":self.refresh_seconds,
            "job_timeout_seconds":self.job_timeout_seconds,"retry_seconds":self.retry_seconds,"max_attempts":self.max_attempts,
            "queue_limit":MAX_QUEUE,"recent_receipts_limit":MAX_RECENT,"concurrency":1,"activity_roots":25,
            "heartbeat_seconds":HEARTBEAT_SECONDS,"heartbeat_max_age_seconds":(self.poll_seconds*3+15).max(30),
            "semantic_config":crate::semantics::config(),"cluster_config":crate::clustering::config(),
            "exposure_policy":crate::exposure::POLICY,"risk_policy":aml_risk_policy::definition(),
            "automatic_approval":false,"whole_chain_analysis_claimed":false})
    }
    pub(crate) fn hash(&self) -> String {
        aml_risk_policy::digest(&self.definition())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Job {
    pub id: String,
    pub address: String,
    pub status: String,
    pub attempts: u32,
    pub next_attempt_unix_ms: u64,
    pub last_error: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scan {
    pub height: u64,
    pub hash: String,
    pub start: u64,
    pub after_address: String,
    pub exhausted: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    pub contract: String,
    pub network_id: String,
    pub database: String,
    pub definition: Value,
    pub definition_hash: String,
    pub generation: u64,
    pub pass: u64,
    pub phase: String,
    pub reason: String,
    pub heartbeat_unix_ms: u64,
    pub next_sweep_unix_ms: u64,
    pub scan: Option<Scan>,
    pub queue: Vec<Job>,
    pub recent: Vec<Value>,
    pub succeeded: u64,
    pub failures: u64,
    pub abandoned: u64,
}
impl State {
    pub(crate) fn new(database: &str, config: &Config) -> Self {
        Self {
            contract: VERSION.into(),
            network_id: NETWORK_ID.into(),
            database: database.into(),
            definition: config.definition(),
            definition_hash: config.hash(),
            generation: 0,
            pass: 0,
            phase: "waiting".into(),
            reason: "starting".into(),
            heartbeat_unix_ms: 0,
            next_sweep_unix_ms: 0,
            scan: None,
            queue: vec![],
            recent: vec![],
            succeeded: 0,
            failures: 0,
            abandoned: 0,
        }
    }
    pub(crate) fn due(&self, now: u64) -> Option<usize> {
        self.queue
            .iter()
            .enumerate()
            .filter(|(_, j)| {
                matches!(j.status.as_str(), "pending" | "retry") && j.next_attempt_unix_ms <= now
            })
            .min_by_key(|(i, j)| (j.attempts > 0, j.next_attempt_unix_ms, *i))
            .map(|(i, _)| i)
    }
    pub(crate) fn finish(
        &mut self,
        index: usize,
        result: Result<Value>,
        config: &Config,
        now: u64,
    ) {
        match result {
            Ok(value) => {
                let job = self.queue.remove(index);
                self.succeeded = self.succeeded.saturating_add(1);
                self.recent.push(json!({"job_id":job.id,"address":job.address,"attempts":job.attempts,"completed_at_unix_ms":now,"result":value}));
                if self.recent.len() > MAX_RECENT {
                    self.recent.remove(0);
                }
                self.phase = "idle".into();
                self.reason = "job_completed".into();
            }
            Err(error) => {
                let j = &mut self.queue[index];
                j.status = if j.attempts >= config.max_attempts {
                    "failed"
                } else {
                    "retry"
                }
                .into();
                j.last_error = error
                    .to_string()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(256)
                    .collect();
                let delay = config
                    .retry_seconds
                    .saturating_mul(1u64 << j.attempts.min(10))
                    .min(3600);
                j.next_attempt_unix_ms = now.saturating_add(delay * 1000);
                self.failures = self.failures.saturating_add(1);
                self.phase = "failed".into();
                self.reason = "job_failed_other_jobs_continue".into();
            }
        }
    }
}

pub fn status_at(directory: &std::path::Path, database: &str, now: u64) -> Value {
    let state = match checkpoint::load(directory, database) {
        Ok(Some(state)) => state,
        Ok(None) => {
            return json!({"contract":VERSION,"network_id":NETWORK_ID,"phase":"waiting","reason":"not_started","heartbeat_fresh":false,"automatic_approval":false});
        }
        Err(_) => {
            return json!({"contract":VERSION,"network_id":NETWORK_ID,"phase":"failed","reason":"checkpoint_invalid_or_unreadable","heartbeat_fresh":false,"automatic_approval":false});
        }
    };
    let max_age = state.definition["heartbeat_max_age_seconds"]
        .as_u64()
        .unwrap_or(30)
        * 1000;
    let fresh = state.heartbeat_unix_ms <= now && now - state.heartbeat_unix_ms <= max_age;
    let failed = state.queue.iter().filter(|j| j.status == "failed").count();
    json!({"contract":VERSION,"network_id":NETWORK_ID,"database":database,"worker_version":VERSION,
        "phase":if fresh{state.phase.as_str()}else{"failed"},"stored_phase":state.phase,
        "reason":if fresh{state.reason.as_str()}else{"heartbeat_stale_or_clock_changed"},"heartbeat_fresh":fresh,
        "heartbeat_unix_ms":state.heartbeat_unix_ms,"heartbeat_max_age_ms":max_age,"definition_hash":state.definition_hash,
        "pass":state.pass,"scan":state.scan,"next_sweep_unix_ms":state.next_sweep_unix_ms,
        "queue":state.queue,"failed_jobs":failed,"degraded":failed>0||state.queue.iter().any(|j|j.status=="retry")||state.phase=="failed",
        "succeeded":state.succeeded,"failed_attempts":state.failures,"abandoned_jobs":state.abandoned,
        "recent":state.recent,"automatic_approval":false,"all_addresses_current":false,
        "coverage":"bounded_repeated_sweeps_of_canonical_ingested_addresses","concurrency":1})
}
pub fn local_status(database: &str) -> Value {
    match std::env::var_os("BITCOIN_ANALYTICS_STATE_DIR") {
        Some(path) => status_at(
            std::path::Path::new(&path),
            database,
            crate::db::timestamp(),
        ),
        None => {
            json!({"contract":VERSION,"network_id":NETWORK_ID,"phase":"waiting","reason":"not_configured","heartbeat_fresh":false,"automatic_approval":false})
        }
    }
}
pub async fn status(db: &Database) -> Result<Value> {
    let mut value = local_status(db.database());
    let source = aml_warehouse::visibility::Snapshot::from_state(crate::warehouse::to_state(
        &db.state().await?,
    ))?
    .value();
    value["source_snapshot"] = source.clone();
    if let Some(recent) = value["recent"].as_array_mut() {
        for receipt in recent {
            // This is only a source comparison, not a claim that reviews/seeds remain valid.
            receipt["source_matches_now"] = json!(receipt["result"]["source_snapshot"] == source);
            receipt["requires_dependency_revalidation"] = json!(true);
        }
    }
    Ok(value)
}
