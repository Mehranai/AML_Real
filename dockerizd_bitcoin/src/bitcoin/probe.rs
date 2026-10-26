//! Bounded, read-only RPC diagnostics. No database, wallet or checkpoint access.
use crate::{GENESIS, NETWORK_ID, config::IngestConfig, model, rpc::Rpc};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};

const INPUTS_PER_BLOCK: usize = 8;
const DEADLINE_SECONDS: u64 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    Degraded,
    Unready,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Warning,
    Fail,
    Info,
}
#[derive(Serialize)]
pub struct Check {
    pub code: &'static str,
    pub outcome: Outcome,
    pub message: &'static str,
}
#[derive(Serialize)]
pub struct Scope {
    pub configured_start_block: u64,
    pub configured_end_block: Option<u64>,
    pub confirmations: u64,
    pub configured_verbosity: u64,
    pub local_database_checked: bool,
    pub full_history_verified: bool,
}
#[derive(Deserialize, Serialize)]
pub struct Node {
    pub chain: String,
    pub blocks: u64,
    pub headers: u64,
    pub bestblockhash: String,
    pub time: u64,
    pub verificationprogress: f64,
    pub initialblockdownload: bool,
    pub pruned: bool,
    pub pruneheight: Option<u64>,
}
#[derive(Default, Serialize)]
pub struct TxIndex {
    pub metadata_available: bool,
    pub enabled: Option<bool>,
    pub synced: Option<bool>,
    pub best_block_height: Option<u64>,
}
#[derive(Serialize)]
pub struct Sample {
    pub height: u64,
    pub hash: String,
    pub verbosity: u64,
    pub transaction_count: usize,
    pub non_coinbase_inputs: usize,
    pub inputs_sampled: usize,
    pub inline_prevouts: usize,
    pub in_block_prevouts: usize,
    pub raw_transaction_lookups: usize,
    pub raw_fallback_verified: usize,
    pub unresolved_inputs: usize,
}
#[derive(Serialize)]
pub struct Report {
    pub report_version: u32,
    pub network_id: &'static str,
    pub observed_at_unix_ms: u64,
    pub status: Readiness,
    pub scope: Option<Scope>,
    pub node: Option<Node>,
    pub txindex: TxIndex,
    pub samples: Vec<Sample>,
    pub checks: Vec<Check>,
    pub suggested_block_verbosity: Option<u64>,
    pub max_sample_blocks: usize,
    pub max_inputs_per_block: usize,
    pub deadline_seconds: u64,
}
impl Report {
    fn new(config: Option<&IngestConfig>) -> Self {
        Self {
            report_version: 1,
            network_id: NETWORK_ID,
            observed_at_unix_ms: crate::db::timestamp(),
            status: Readiness::Ready,
            scope: config.map(|c| Scope {
                configured_start_block: c.start,
                configured_end_block: c.end,
                confirmations: c.confirmations,
                configured_verbosity: c.verbosity,
                local_database_checked: false,
                full_history_verified: false,
            }),
            node: None,
            txindex: TxIndex::default(),
            samples: Vec::new(),
            checks: Vec::new(),
            suggested_block_verbosity: None,
            max_sample_blocks: 2,
            max_inputs_per_block: INPUTS_PER_BLOCK,
            deadline_seconds: DEADLINE_SECONDS,
        }
    }
    fn check(&mut self, outcome: Outcome, code: &'static str, message: &'static str) {
        match outcome {
            Outcome::Fail => self.status = Readiness::Unready,
            Outcome::Warning if self.status == Readiness::Ready => {
                self.status = Readiness::Degraded
            }
            _ => {}
        }
        self.checks.push(Check {
            code,
            outcome,
            message,
        });
    }
    pub fn configuration_error() -> Self {
        let mut report = Self::new(None);
        report.check(Outcome::Fail, "configuration_invalid", "Check BITCOIN_RPC_URL/USER/PASSWORD/TIMEOUT_SECONDS and Bitcoin ingestion range settings; credentials must be separate from the URL.");
        report
    }
    pub fn exit_code(&self) -> u8 {
        match self.status {
            Readiness::Ready => 0,
            Readiness::Unready => 1,
            Readiness::Degraded => 2,
        }
    }
}

pub async fn inspect(rpc: &Rpc, config: &IngestConfig) -> Report {
    let mut report = Report::new(Some(config));
    if !(1..=1000).contains(&config.confirmations)
        || !matches!(config.verbosity, 2 | 3)
        || config.start > u32::MAX as u64
        || config
            .end
            .is_some_and(|end| end < config.start || end > u32::MAX as u64)
    {
        report.check(
            Outcome::Fail,
            "configuration_invalid",
            "Invalid block range, confirmations or getblock verbosity.",
        );
        return report;
    }
    match tokio::time::timeout(Duration::from_secs(DEADLINE_SECONDS), inspect_node(rpc, config, &mut report)).await {
        Err(_) => report.check(Outcome::Fail, "probe_deadline_exceeded", "The bounded RPC probe exceeded 120 seconds; check endpoint latency and RPC timeout settings."),
        Ok(Err(_)) => report.check(Outcome::Fail, "invalid_rpc_evidence", "RPC returned malformed or inconsistent block, transaction or prevout evidence."),
        Ok(Ok(())) => {}
    }
    report
}

async fn inspect_node(rpc: &Rpc, config: &IngestConfig, report: &mut Report) -> Result<()> {
    let info = match rpc.call("getblockchaininfo", json!([])).await {
        Ok(value) => value,
        Err(_) => {
            report.check(
                Outcome::Fail,
                "rpc_unavailable",
                "getblockchaininfo failed; check connectivity, authentication and RPC permissions.",
            );
            return Ok(());
        }
    };
    let node: Node = serde_json::from_value(info).context("invalid chain info")?;
    ensure!(
        node.headers >= node.blocks
            && node.blocks <= u32::MAX as u64
            && node.verificationprogress.is_finite()
            && (0.0..=1.0).contains(&node.verificationprogress),
        "invalid sync metadata"
    );
    model::hash(&node.bestblockhash)?;
    ensure!(
        !node.pruned || node.pruneheight.is_some_and(|h| h <= node.blocks),
        "missing prune boundary"
    );
    report.node = Some(node);
    if report.node.as_ref().unwrap().chain != "main" {
        report.check(
            Outcome::Fail,
            "wrong_network",
            "This adapter requires Bitcoin mainnet.",
        );
        return Ok(());
    }
    match rpc.block_hash(0).await {
        Ok(hash) if hash == GENESIS => report.check(
            Outcome::Pass,
            "mainnet_identity",
            "Mainnet chain name and genesis hash match.",
        ),
        _ => {
            report.check(
                Outcome::Fail,
                "genesis_mismatch",
                "Cannot verify the Bitcoin mainnet genesis hash.",
            );
            return Ok(());
        }
    }
    let node = report.node.as_ref().unwrap();
    let (height, best_hash, pruneheight, pruned) = (
        node.blocks,
        node.bestblockhash.clone(),
        node.pruneheight,
        node.pruned,
    );
    let syncing =
        node.initialblockdownload || node.headers > height || node.verificationprogress < 0.999;
    let tip_age = report.observed_at_unix_ms / 1000;
    let stale =
        tip_age.saturating_sub(node.time) > 7200 || node.time.saturating_sub(tip_age) > 7200;
    if syncing {
        report.check(
            Outcome::Warning,
            "node_syncing",
            "Node is downloading/validating blocks or has not caught up with its known headers.",
        );
    } else {
        report.check(
            Outcome::Pass,
            "node_sync",
            "Node reports no initial download or known header lag.",
        );
    }
    if stale {
        report.check(Outcome::Warning, "node_tip_time", "Tip timestamp differs from local time by over two hours; check node freshness and clocks.");
    }
    let safe = height.checked_sub(config.confirmations - 1);
    let end = safe.map(|h| config.end.map_or(h, |end| end.min(h)));
    let Some(end) = end.filter(|end| *end >= config.start) else {
        report.check(Outcome::Fail, "no_confirmed_range", "Configured start is above the available confirmed range; wait for sync/confirmations or correct the range.");
        return Ok(());
    };
    if pruned && pruneheight.is_some_and(|floor| config.start < floor) {
        report.check(Outcome::Fail, "requested_history_pruned", "The configured start precedes retained blocks; use an unpruned node or explicitly choose an available partial-history range.");
        return Ok(());
    }
    if pruned {
        report.check(Outcome::Warning, "pruned_history", "Only retained history can be sampled; pruning can remove blocks needed for a later restart or reorg.");
    } else {
        report.check(
            Outcome::Pass,
            "unpruned_history",
            "Node reports pruning disabled; boundary block access is checked below.",
        );
    }
    inspect_index(rpc, report).await;
    let mut heights = vec![config.start];
    if end != config.start {
        heights.push(end);
    }
    for sample_height in heights {
        let hash = match rpc.block_hash(sample_height).await {
            Ok(hash) => hash,
            Err(_) => {
                report.check(
                    Outcome::Fail,
                    "sample_hash_unavailable",
                    "Cannot read a configured range boundary block hash.",
                );
                continue;
            }
        };
        let (block, verbosity) = match rpc.block(&hash, config.verbosity).await {
            Ok(block) => (block, config.verbosity),
            Err(_) if config.verbosity == 3 => match rpc.block(&hash, 2).await {
                Ok(block) => {
                    report.suggested_block_verbosity = Some(2);
                    report.check(Outcome::Fail, "configured_verbosity_unavailable", "Configured getblock verbosity 3 failed but verbosity 2 worked; explicitly select verbosity 2 and verify its prevout fallback before ingestion.");
                    (block, 2)
                }
                Err(_) => {
                    report.check(Outcome::Fail, "sample_block_unavailable", "Cannot read a decoded boundary block; check block retention and getblock permissions.");
                    continue;
                }
            },
            Err(_) => {
                report.check(Outcome::Fail, "sample_block_unavailable", "Cannot read a decoded boundary block; check block retention and getblock permissions.");
                continue;
            }
        };
        let (mut sample, inputs) = scan_block(&block, sample_height, &hash, verbosity)?;
        let mut raw_cache = HashMap::<String, Option<Value>>::new();
        for input in &inputs {
            if input.in_block {
                continue;
            }
            // Test at least one historical lookup even when undo data is present.
            if input.inline.is_none() || raw_cache.is_empty() {
                if !raw_cache.contains_key(&input.txid) {
                    let value = rpc
                        .call("getrawtransaction", json!([input.txid, true]))
                        .await
                        .ok();
                    raw_cache.insert(input.txid.clone(), value);
                }
                if let Some(raw) = &raw_cache[&input.txid] {
                    ensure!(
                        raw["txid"].as_str() == Some(&input.txid),
                        "wrong fallback transaction"
                    );
                    let output = raw["vout"]
                        .as_array()
                        .and_then(|v| v.get(input.index as usize))
                        .context("missing fallback output")?;
                    ensure!(
                        output["n"].as_u64() == Some(input.index),
                        "wrong fallback output index"
                    );
                    let output = model::ScriptOutput::from_rpc(output)?;
                    if let Some(inline) = &input.inline {
                        ensure!(
                            inline.value_sats == output.value_sats
                                && inline.script_hex == output.script_hex,
                            "contradictory prevout evidence"
                        );
                    }
                    sample.raw_fallback_verified += 1;
                } else if input.inline.is_none() {
                    sample.unresolved_inputs += 1;
                }
            }
        }
        sample.raw_transaction_lookups = raw_cache.len();
        if sample.unresolved_inputs > 0 {
            if config.start == 0 {
                report.check(Outcome::Warning, "local_history_required", "Sampled inputs lack RPC prevouts and raw lookup; sequential ingestion from genesis must resolve them from previously committed local outputs. Local history was not checked.");
            } else {
                report.check(Outcome::Fail, "cold_start_prevouts_unavailable", "A fresh start at the configured height cannot resolve sampled inputs; provide undo prevouts or working historical getrawtransaction/txindex. Existing local outputs were not checked.");
            }
        } else if sample.inputs_sampled > 0 {
            report.check(Outcome::Pass, "sample_prevouts_resolved", "Every sampled input was resolved from inline undo data, an earlier transaction in that block, or historical raw transaction lookup.");
        }
        if inputs
            .iter()
            .any(|input| !input.in_block && input.inline.is_none())
            && sample.unresolved_inputs == 0
            && !(report.txindex.synced == Some(true)
                && report
                    .txindex
                    .best_block_height
                    .is_some_and(|h| h >= sample_height))
        {
            report.check(Outcome::Warning, "fallback_index_unconfirmed", "Sampled raw lookups worked, but a synced txindex covering this height was not verified; this does not prove arbitrary historical lookup.");
        }
        if !raw_cache.is_empty()
            && sample.raw_fallback_verified == 0
            && sample.unresolved_inputs == 0
        {
            report.check(Outcome::Info, "raw_fallback_unavailable", "Historical raw lookup did not work; sampled inputs were resolved by another route.");
        }
        if rpc.block_hash(sample_height).await.ok().as_deref() != Some(hash.as_str()) {
            report.check(Outcome::Warning, "chain_changed_during_probe", "A sampled block changed or could not be rechecked; repeat the probe before ingestion.");
        }
        report.samples.push(sample);
    }
    if !report
        .samples
        .iter()
        .any(|s| s.inputs_sampled > s.in_block_prevouts)
    {
        report.check(Outcome::Warning, "external_prevouts_not_tested", "Sampled blocks contained no external inputs; undo data and historical prevout resolution remain untested.");
    }
    if rpc.block_hash(height).await.ok().as_deref() != Some(best_hash.as_str()) {
        report.check(
            Outcome::Warning,
            "tip_changed_during_probe",
            "The originally observed tip changed or could not be rechecked; repeat the probe.",
        );
    }
    Ok(())
}

async fn inspect_index(rpc: &Rpc, report: &mut Report) {
    let Ok(value) = rpc.call("getindexinfo", json!(["txindex"])).await else {
        report.check(Outcome::Info, "txindex_metadata_unavailable", "getindexinfo is unavailable; txindex state is unknown and raw lookup is tested separately when possible.");
        return;
    };
    let Some(object) = value.as_object() else {
        report.check(
            Outcome::Warning,
            "invalid_txindex_metadata",
            "getindexinfo returned invalid metadata; txindex state is unknown.",
        );
        return;
    };
    if let Some(index) = object.get("txindex") {
        match (
            index["synced"].as_bool(),
            index["best_block_height"].as_u64(),
        ) {
            (Some(synced), Some(height)) => {
                report.txindex = TxIndex {
                    metadata_available: true,
                    enabled: Some(true),
                    synced: Some(synced),
                    best_block_height: Some(height),
                }
            }
            _ => report.check(
                Outcome::Warning,
                "invalid_txindex_metadata",
                "txindex sync/height metadata is invalid; index readiness is unknown.",
            ),
        }
    } else {
        report.txindex = TxIndex {
            metadata_available: true,
            enabled: Some(false),
            ..TxIndex::default()
        };
    }
}

struct Input {
    txid: String,
    index: u64,
    inline: Option<model::ScriptOutput>,
    in_block: bool,
}
fn scan_block(
    block: &Value,
    height: u64,
    hash: &str,
    verbosity: u64,
) -> Result<(Sample, Vec<Input>)> {
    ensure!(
        block["hash"].as_str() == Some(hash) && block["height"].as_u64() == Some(height),
        "block identity mismatch"
    );
    ensure!(
        block["confirmations"].as_i64().is_some_and(|n| n > 0),
        "block is not canonical"
    );
    let txs = block["tx"]
        .as_array()
        .context("missing decoded transactions")?;
    ensure!(!txs.is_empty(), "empty block");
    let mut sample = Sample {
        height,
        hash: hash.into(),
        verbosity,
        transaction_count: txs.len(),
        non_coinbase_inputs: 0,
        inputs_sampled: 0,
        inline_prevouts: 0,
        in_block_prevouts: 0,
        raw_transaction_lookups: 0,
        raw_fallback_verified: 0,
        unresolved_inputs: 0,
    };
    let mut inputs = Vec::new();
    let mut earlier = HashMap::<String, &Value>::new();
    for (position, tx) in txs.iter().enumerate() {
        let txid = model::hash(tx["txid"].as_str().context("missing txid")?)?;
        let vin = tx["vin"].as_array().context("missing vin")?;
        ensure!(!vin.is_empty(), "empty vin");
        let coinbase = vin.len() == 1 && vin[0].get("coinbase").is_some();
        ensure!((position == 0) == coinbase, "invalid coinbase position");
        ensure!(
            tx["vout"].as_array().is_some_and(|v| !v.is_empty()),
            "missing outputs"
        );
        if !coinbase {
            sample.non_coinbase_inputs += vin.len();
            for input in vin.iter().take(INPUTS_PER_BLOCK - inputs.len()) {
                let previous =
                    model::hash(input["txid"].as_str().context("missing previous txid")?)?;
                let index = input["vout"]
                    .as_u64()
                    .filter(|n| *n <= u32::MAX as u64)
                    .context("invalid previous output index")?;
                let inline = input
                    .get("prevout")
                    .map(model::ScriptOutput::from_rpc)
                    .transpose()?;
                let in_block = if let Some(tx) = earlier.get(&previous) {
                    let output = tx["vout"]
                        .as_array()
                        .and_then(|v| v.get(index as usize))
                        .context("missing in-block output")?;
                    ensure!(
                        output["n"].as_u64() == Some(index),
                        "in-block index mismatch"
                    );
                    let output = model::ScriptOutput::from_rpc(output)?;
                    if let Some(inline) = &inline {
                        ensure!(
                            inline.value_sats == output.value_sats
                                && inline.script_hex == output.script_hex,
                            "contradictory in-block prevout"
                        );
                    }
                    true
                } else {
                    false
                };
                sample.inline_prevouts += usize::from(inline.is_some());
                sample.in_block_prevouts += usize::from(in_block);
                inputs.push(Input {
                    txid: previous,
                    index,
                    inline,
                    in_block,
                });
            }
        }
        ensure!(
            earlier.insert(txid, tx).is_none(),
            "duplicate transaction in sample"
        );
    }
    sample.inputs_sampled = inputs.len();
    Ok((sample, inputs))
}
