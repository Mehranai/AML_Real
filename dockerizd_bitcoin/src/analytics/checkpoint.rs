use super::*;
use anyhow::Context;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 1024 * 1024;
fn path(directory: &Path, database: &str) -> Result<PathBuf> {
    aml_warehouse::contract::identifier(database)?;
    Ok(directory.join(format!("bitcoin-analytics-{database}.json")))
}
pub(super) fn load(directory: &Path, database: &str) -> Result<Option<State>> {
    let file = match File::open(path(directory, database)?) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = vec![];
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_BYTES as usize,
        "analytics checkpoint exceeds size cap"
    );
    let frame: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        frame["checksum"] == aml_risk_policy::digest(&frame["state"]),
        "analytics checkpoint checksum mismatch"
    );
    let state: State = serde_json::from_value(frame["state"].clone())?;
    ensure!(
        state.contract == VERSION && state.network_id == NETWORK_ID && state.database == database,
        "analytics checkpoint identity mismatch"
    );
    ensure!(
        state.queue.len() <= MAX_QUEUE
            && state.recent.len() <= MAX_RECENT
            && state.definition_hash == aml_risk_policy::digest(&state.definition),
        "invalid analytics checkpoint limits/configuration"
    );
    ensure!(
        ["waiting", "running", "idle", "failed"].contains(&state.phase.as_str()),
        "invalid analytics phase"
    );
    let mut ids = std::collections::HashSet::new();
    for job in &state.queue {
        crate::model::address(&job.address)?;
        ensure!(
            ids.insert(&job.id)
                && ["pending", "running", "retry", "failed"].contains(&job.status.as_str())
                && job.attempts <= 20,
            "invalid analytics job"
        );
    }
    Ok(Some(state))
}
pub(super) fn save(directory: &Path, state: &mut State, now: u64) -> Result<()> {
    state.generation = state
        .generation
        .checked_add(1)
        .context("analytics generation exhausted")?;
    state.heartbeat_unix_ms = now;
    let body = serde_json::to_value(&*state)?;
    let bytes =
        serde_json::to_vec(&json!({"checksum":aml_risk_policy::digest(&body),"state":body}))?;
    ensure!(
        bytes.len() <= MAX_BYTES as usize,
        "analytics checkpoint exceeds size cap"
    );
    fs::create_dir_all(directory)?;
    let target = path(directory, &state.database)?;
    let temporary = target.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, &target).context("cannot commit analytics checkpoint")?;
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;
    Ok(())
}
