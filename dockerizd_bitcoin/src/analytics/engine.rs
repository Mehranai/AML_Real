use super::*;
use crate::{db, writer_lock::WriterLock};
use anyhow::{Context, Result};
use std::time::Duration;
pub struct Worker {
    _owner: WriterLock,
    pub config: Config,
    pub(crate) state: State,
}
impl Worker {
    pub fn open(database: &str, config: Config) -> Result<Self> {
        config.validate()?;
        let owner = WriterLock::for_database(&format!("{database}_analytics"))?;
        let mut state = checkpoint::load(&config.directory, database)?
            .unwrap_or_else(|| State::new(database, &config));
        if state.definition_hash != config.hash() {
            state.abandoned = state.abandoned.saturating_add(state.queue.len() as u64);
            state.queue.clear();
            state.scan = None;
            state.next_sweep_unix_ms = 0;
            state.definition = config.definition();
            state.definition_hash = config.hash();
        }
        for j in &mut state.queue {
            if j.status == "running" {
                j.status = if j.attempts >= config.max_attempts {
                    "failed"
                } else {
                    "retry"
                }
                .into();
                j.next_attempt_unix_ms = 0;
                j.last_error = "interrupted attempt; publication must be revalidated".into();
            }
        }
        state.phase = "waiting".into();
        state.reason = "started_or_resumed".into();
        checkpoint::save(&config.directory, &mut state, db::timestamp())?;
        Ok(Self {
            _owner: owner,
            config,
            state,
        })
    }
    fn save(&mut self) -> Result<()> {
        checkpoint::save(&self.config.directory, &mut self.state, db::timestamp())
    }
    pub fn pause(&mut self) -> Result<()> {
        for j in &mut self.state.queue {
            if j.status == "running" {
                j.status = if j.attempts >= self.config.max_attempts {
                    "failed"
                } else {
                    "retry"
                }
                .into();
                j.next_attempt_unix_ms = 0;
                j.last_error = "interrupted by shutdown".into();
            }
        }
        self.state.phase = "waiting".into();
        self.state.reason = "stopped".into();
        self.save()
    }
    pub fn retry(&mut self, id: &str) -> Result<()> {
        let j = self
            .state
            .queue
            .iter_mut()
            .find(|j| j.id == id && j.status == "failed")
            .context("failed analytics job not found; pause worker before retry")?;
        j.attempts = 0;
        j.status = "pending".into();
        j.next_attempt_unix_ms = 0;
        self.state.reason = "manual_retry_queued".into();
        self.save()
    }
    pub fn report(&self) -> Value {
        status_at(
            &self.config.directory,
            &self.state.database,
            db::timestamp(),
        )
    }
    fn begin_pass(&mut self, source: &db::SyncState) -> Result<()> {
        self.state.pass = self
            .state
            .pass
            .checked_add(1)
            .context("analytics pass exhausted")?;
        self.state.scan = Some(Scan {
            height: source.tip_height,
            hash: source.tip_hash.clone(),
            start: source.start_height,
            after_address: String::new(),
            exhausted: false,
        });
        self.state.next_sweep_unix_ms = 0;
        self.state.reason = "sweeping_canonical_addresses".into();
        Ok(())
    }
    async fn discover(&mut self, db: &Database, source: &db::SyncState) -> Result<()> {
        let scan = self.state.scan.as_ref().context("analytics scan missing")?;
        if scan.exhausted || self.state.queue.len() >= MAX_QUEUE {
            return Ok(());
        }
        let count = self
            .config
            .page_size
            .min(MAX_QUEUE - self.state.queue.len());
        let rows:Vec<Value>=db.rows("SELECT address FROM (SELECT address FROM aml_bitcoin_inputs_canonical WHERE network_id={network:String} AND block_number<={height:UInt64} AND isNotNull(address) UNION ALL SELECT address FROM aml_bitcoin_outputs_canonical WHERE network_id={network:String} AND block_number<={height:UInt64} AND isNotNull(address)) WHERE address>{after:String} GROUP BY address ORDER BY address LIMIT {limit:UInt64} SETTINGS max_threads=2,max_memory_usage=268435456,max_execution_time=15",&[("network",NETWORK_ID.into()),("height",scan.height.to_string()),("after",scan.after_address.clone()),("limit",(count+1).to_string())]).await?;
        ensure!(
            db.state().await?.revision == source.revision,
            "source changed during analytics discovery; cursor retained"
        );
        for row in rows.iter().take(count) {
            let address = crate::model::address(
                row["address"]
                    .as_str()
                    .context("canonical address missing")?,
            )?;
            if !self.state.queue.iter().any(|j| j.address == address) {
                let id = format!(
                    "btc-job-{}",
                    aml_risk_policy::digest(&json!([
                        self.state.database,
                        self.state.definition_hash,
                        self.state.pass,
                        address
                    ]))
                );
                self.state.queue.push(Job {
                    id,
                    address: address.clone(),
                    status: "pending".into(),
                    attempts: 0,
                    next_attempt_unix_ms: 0,
                    last_error: String::new(),
                });
            }
            self.state.scan.as_mut().unwrap().after_address = address;
        }
        self.state.scan.as_mut().unwrap().exhausted = rows.len() <= count;
        self.save()
    }
    /// One durable job attempt, or one discovery/wait step. True means an attempt ran.
    pub async fn cycle(&mut self, db: &Database) -> Result<bool> {
        ensure!(
            db.database() == self.state.database,
            "analytics database mismatch"
        );
        let result = self.cycle_inner(db).await;
        if result.is_err() {
            self.state.phase = "failed".into();
            self.state.reason = "source_or_discovery_unavailable_cursor_retained".into();
            self.save()?;
        }
        result
    }
    async fn cycle_inner(&mut self, db: &Database) -> Result<bool> {
        let source = db.state().await?;
        if source.initialized == 0 {
            self.state.phase = "waiting".into();
            self.state.reason = "waiting_for_blocks".into();
            self.save()?;
            return Ok(false);
        }
        let now = db::timestamp();
        let reset = if let Some(scan) = &self.state.scan {
            source.start_height != scan.start
                || source.tip_height < scan.height
                || db.block_hash(scan.height).await?.as_deref() != Some(scan.hash.as_str())
        } else {
            false
        };
        if reset {
            self.state.abandoned = self
                .state
                .abandoned
                .saturating_add(self.state.queue.len() as u64);
            self.state.queue.clear();
            self.begin_pass(&source)?;
            self.state.reason = "source_branch_changed_resweep".into();
            self.save()?;
        }
        let active = self.state.queue.iter().any(|j| j.status != "failed");
        if self.state.scan.is_none()
            || (self.state.scan.as_ref().is_some_and(|s| s.exhausted)
                && !active
                && self.state.next_sweep_unix_ms > 0
                && now >= self.state.next_sweep_unix_ms)
        {
            self.begin_pass(&source)?;
            self.save()?;
        }
        if self.state.due(now).is_none() {
            self.discover(db, &source).await?;
        }
        let Some(index) = self.state.due(db::timestamp()) else {
            let exhausted = self.state.scan.as_ref().is_some_and(|s| s.exhausted);
            let active = self.state.queue.iter().any(|j| j.status != "failed");
            if exhausted && !active && self.state.next_sweep_unix_ms == 0 {
                self.state.next_sweep_unix_ms =
                    now.saturating_add(self.config.refresh_seconds * 1000);
            }
            self.state.phase = if self.state.queue.iter().any(|j| j.status == "failed") {
                "failed"
            } else if active {
                "waiting"
            } else {
                "idle"
            }
            .into();
            self.state.reason = if self.state.queue.len() >= MAX_QUEUE {
                "queue_full_operator_attention_required"
            } else if active {
                "retry_backoff"
            } else if exhausted {
                "awaiting_next_sweep"
            } else {
                "discovery_pending"
            }
            .into();
            self.save()?;
            return Ok(false);
        };
        self.state.queue[index].status = "running".into();
        self.state.queue[index].attempts += 1;
        self.state.phase = "running".into();
        self.state.reason = "refreshing_address_analysis".into();
        self.save()?;
        let address = self.state.queue[index].address.clone();
        let future = tokio::time::timeout(
            Duration::from_secs(self.config.job_timeout_seconds),
            analyze(db, &address),
        );
        tokio::pin!(future);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECONDS));
        heartbeat.tick().await;
        let result = loop {
            tokio::select! {
                result=&mut future=>break match result {Ok(r)=>r,Err(_)=>Err(anyhow::anyhow!("analytics job deadline exceeded; attempt may leave audit-only rows"))},
                _=heartbeat.tick()=>self.save()?,
            }
        };
        self.state
            .finish(index, result, &self.config, db::timestamp());
        self.save()?;
        Ok(true)
    }
}
async fn analyze(db: &Database, address: &str) -> Result<Value> {
    let data = crate::api::investigation(db, address, 25).await?;
    let source = aml_warehouse::visibility::Snapshot::from_state(crate::warehouse::to_state(
        &db.state().await?,
    ))?
    .value();
    ensure!(
        source == data["source_snapshot"] && data["risk_persistence"]["persisted"] == true,
        "analytics source changed or assessment not published"
    );
    Ok(
        json!({"source_snapshot":source,"fingerprint_hash":aml_risk_policy::digest(&data["fingerprint"]),
        "semantic_snapshot":data["semantic_snapshot"],"semantic_coverage":data["semantic_coverage"],
        "cluster_run_id":data["cluster_snapshot"]["run_id"],"cluster_coverage":data["cluster_coverage"],
        "exposure_run_id":data["exposure_snapshot"]["run_id"],"exposure_coverage":data["exposure_coverage"],
        "assessment_id":data["risk_snapshot"]["assessment_id"],"risk_level":data["risk_engine"]["risk_level"],
        "fingerprint_and_semantic_events_in_assessment":true,"automatic_approval":false}),
    )
}
