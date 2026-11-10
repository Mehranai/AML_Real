use super::{BUDGET, Log, Lookup, Observation, invalid, lookup, snapshot, unavailable};
use crate::{ApiError, Network};
use aml_warehouse::{Connection, publication::literal, values, visibility};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FUNDING_ROUTE: &str = "/internal/bridge/tbtc/funding/{hash}";
pub const REVEALS_ROUTE: &str = "/internal/bridge/tbtc/reveals/{hash}";
pub const SPENDS_ROUTE: &str = "/internal/bridge/tbtc/spends/{hash}";
pub const SWEEPS_ROUTE: &str = "/internal/bridge/tbtc/sweeps/{hash}";
pub const MINTS_ROUTE: &str = "/internal/bridge/tbtc/mints/{key}";
// Threshold's mainnet Bridge proxy and its deposit events.
pub const EMITTER: &str = "0x5e4861a80b55f035d899f66772117f00fa0e8e7b";
pub const REVEAL_TOPIC: &str = "a7382159a693ed317a024daf0fd1ba30805cdf9928ee09550af517c516e2ef05";
pub const SWEEP_TOPIC: &str = "e50ffdcc0a5f2c1ede5c122b9414ffd7b2c6bc870d2d775194049dc30da95e6a";
pub const VAULT: &str = "0x9c070027cdc9dc8f82416b2e5314e11dfb4fe3cd";
pub const TOKEN: &str = "0x18084fba666a33d37592fa2633fd49a74dd93a88";
pub const MINT_FINALIZED_TOPIC: &str =
    "2cffebf26d639426e79514d100febae8b2c63e700e5dc0fa6c88a12963350636";
pub const MINTED_TOPIC: &str = "30385c845b448a36257a6a1716e6ad2e1bc2cbe333cde1e69fe849ad6511adfe";
pub const TRANSFER_TOPIC: &str = "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
pub const OUTPUT_LIMIT: usize = 8;
pub const SPEND_BLOCKS: u64 = 4_320;
pub const PROOF_DELAY_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const CLOCK_SKEW_MS: u64 = 2 * 60 * 60 * 1000;
pub const MINT_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevealWindow {
    pub from_ms: u64,
    pub to_ms: u64,
    pub output_index: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FundingQuery {
    pub block_number: u64,
    pub output_index: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepWindow {
    pub from_ms: u64,
    pub to_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MintWindow {
    pub from_ms: u64,
    pub to_ms: u64,
}

#[derive(Deserialize, Serialize)]
pub struct MintObservation {
    pub finalizations: Observation,
    pub preceding_logs: Vec<Log>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpendQuery {
    pub block_number: u64,
    pub output_index: u32,
}

#[derive(Deserialize, Serialize)]
pub struct FundingObservation {
    pub network_id: String,
    pub source_snapshot: Value,
    pub outputs: Vec<Output>,
    pub truncated: bool,
}

#[derive(Deserialize, Serialize)]
pub struct Output {
    pub tx_hash: String,
    pub block_hash: String,
    pub block_number: u64,
    pub timestamp_ms: u64,
    pub output_index: u32,
    pub value_sats: u64,
    pub script_hex: String,
}

#[derive(Deserialize, Serialize)]
pub struct SpendObservation {
    pub network_id: String,
    pub source_snapshot: Value,
    pub inputs: Vec<Spend>,
}

#[derive(Deserialize, Serialize)]
pub struct Spend {
    pub tx_hash: String,
    pub block_hash: String,
    pub block_number: u64,
    pub timestamp_ms: u64,
    pub input_index: u32,
    pub prev_tx_hash: String,
    pub prev_output_index: u32,
}

pub fn reverse_hash(hash: &str) -> Option<String> {
    values::hex(hash, 64).then(|| {
        (0..64)
            .step_by(2)
            .rev()
            .map(|i| &hash[i..i + 2])
            .collect::<String>()
            .to_ascii_lowercase()
    })
}

pub async fn mints(
    db: &impl Connection,
    database: &str,
    key: &str,
    window: MintWindow,
) -> Result<MintObservation, ApiError> {
    let network = Network::Ethereum;
    let finalizations = lookup(db, database, network, key, Lookup::TbtcMint(window)).await?;
    let preceding_logs = match finalizations.logs.as_slice() {
        [log] if !finalizations.truncated && log.log_index > 0 => mint_neighbors(db, database, log)
            .await
            .map_err(unavailable)?,
        _ => Vec::new(),
    };
    if snapshot(db, database, network).await? != finalizations.source_snapshot {
        return Err(unavailable(anyhow::anyhow!("mint source snapshot changed")));
    }
    Ok(MintObservation {
        finalizations,
        preceding_logs,
    })
}

async fn mint_neighbors(
    db: &impl Connection,
    database: &str,
    log: &Log,
) -> anyhow::Result<Vec<Log>> {
    let network = literal(Network::Ethereum.id());
    let block = log.block_number;
    let from = log.log_index.saturating_sub(4);
    let to = log.log_index - 1;
    let rows = db
        .query_bounded(&format!(
            "SELECT toJSONString(tuple(log_index,address,topics,data))
         FROM {database}.aml_evm_logs FINAL
         WHERE network_id={network} AND block_number={block} AND tx_hash={}
           AND block_hash={} AND block_timestamp_unix_ms={}
           AND log_index BETWEEN {from} AND {to}
           AND (block_number,block_hash,block_timestamp_unix_ms,commit_id,revision) IN (
             SELECT block_number,block_hash,block_timestamp_unix_ms,commit_id,revision
             FROM {database}.aml_ingested_blocks_canonical
             WHERE network_id={network} AND block_number={block}
           )
           AND ifNull(removed,0)=0 AND address IN ('{VAULT}','{TOKEN}')
           AND length(topics)<=4 AND length(data)<=66
         LIMIT 5 {BUDGET}",
            literal(&log.tx_hash),
            literal(&log.block_hash),
            log.timestamp_ms
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            let (log_index, address, topics, data) = serde_json::from_str(&row)?;
            Ok(Log {
                log_index,
                address,
                topics,
                data,
                ..log.clone()
            })
        })
        .collect()
}

pub async fn funding(
    db: &impl Connection,
    database: &str,
    hash: &str,
    query: FundingQuery,
) -> Result<FundingObservation, ApiError> {
    let hash = values::normalize_hash(Network::Bitcoin, hash)
        .map_err(|_| invalid("Invalid Bitcoin funding hash"))?;
    observe_funding(db, database, &hash, query)
        .await
        .map_err(unavailable)
}

async fn observe_funding(
    db: &impl Connection,
    database: &str,
    hash: &str,
    query: FundingQuery,
) -> anyhow::Result<FundingObservation> {
    let network = Network::Bitcoin;
    let snapshot = visibility::capture(db, database, network).await?;
    let mut result = FundingObservation {
        network_id: network.id().into(),
        source_snapshot: snapshot.value(),
        outputs: Vec::new(),
        truncated: false,
    };
    let state = snapshot.state();
    let block = query.block_number;
    if state.initialized == 0 || !(state.start_block..=state.last_block_number).contains(&block) {
        return Ok(result);
    }
    let network = literal(network.id());
    let index = query
        .output_index
        .map_or_else(String::new, |index| format!("AND output_index={index}"));
    let rows = db.query_bounded(&format!(
        "SELECT toJSONString(tuple(block_hash,toString(block_timestamp_unix_ms),output_index,toString(value_sats),script_hex))
         FROM {database}.aml_bitcoin_outputs FINAL
         WHERE network_id={network} AND block_number={block} AND tx_hash={}
           AND (block_number,block_hash,block_timestamp_unix_ms,commit_id,revision) IN (
              SELECT block_number,block_hash,block_timestamp_unix_ms,commit_id,revision
              FROM {database}.aml_ingested_blocks_canonical
              WHERE network_id={network} AND block_number={block}
           )
           AND (match(script_hex,'^a914[0-9a-f]{{40}}87$') OR match(script_hex,'^0020[0-9a-f]{{64}}$'))
           {index} LIMIT {} {BUDGET}",
        literal(hash), OUTPUT_LIMIT + 1
    )).await?;
    result.truncated = rows.len() > OUTPUT_LIMIT;
    for row in rows.into_iter().take(OUTPUT_LIMIT) {
        let (block_hash, timestamp, output_index, amount, script_hex): (
            String,
            String,
            u32,
            String,
            String,
        ) = serde_json::from_str(&row)?;
        result.outputs.push(Output {
            tx_hash: hash.into(),
            block_hash,
            block_number: block,
            timestamp_ms: timestamp.parse()?,
            output_index,
            value_sats: amount.parse()?,
            script_hex,
        });
    }
    visibility::unchanged(db, database, &snapshot).await?;
    Ok(result)
}

pub async fn spends(
    db: &impl Connection,
    database: &str,
    hash: &str,
    query: SpendQuery,
) -> Result<SpendObservation, ApiError> {
    let hash = values::normalize_hash(Network::Bitcoin, hash)
        .map_err(|_| invalid("Invalid Bitcoin funding hash"))?;
    observe_spends(db, database, &hash, query)
        .await
        .map_err(unavailable)
}

async fn observe_spends(
    db: &impl Connection,
    database: &str,
    hash: &str,
    query: SpendQuery,
) -> anyhow::Result<SpendObservation> {
    let network = Network::Bitcoin;
    let snapshot = visibility::capture(db, database, network).await?;
    let state = snapshot.state();
    let mut result = SpendObservation {
        network_id: network.id().into(),
        source_snapshot: snapshot.value(),
        inputs: Vec::new(),
    };
    let from = query.block_number;
    if state.initialized == 0 || !(state.start_block..=state.last_block_number).contains(&from) {
        return Ok(result);
    }
    let to = state
        .last_block_number
        .min(from.saturating_add(SPEND_BLOCKS));
    let network = literal(network.id());
    let index = query.output_index;
    let rows = db.query_bounded(&format!(
        "SELECT toJSONString(tuple(tx_hash,block_hash,toString(block_number),toString(block_timestamp_unix_ms),input_index))
         FROM {database}.aml_bitcoin_inputs FINAL
         WHERE network_id={network} AND block_number BETWEEN {from} AND {to}
           AND prev_tx_hash={} AND prev_output_index={index}
           AND (block_number,block_hash,block_timestamp_unix_ms,commit_id,revision) IN (
             SELECT block_number,block_hash,block_timestamp_unix_ms,commit_id,revision
             FROM {database}.aml_ingested_blocks_canonical
             WHERE network_id={network} AND block_number BETWEEN {from} AND {to}
           )
         LIMIT 2 {BUDGET}",
        literal(hash)
    )).await?;
    for row in rows {
        let (tx_hash, block_hash, height, timestamp, input_index): (
            String,
            String,
            String,
            String,
            u32,
        ) = serde_json::from_str(&row)?;
        result.inputs.push(Spend {
            tx_hash,
            block_hash,
            block_number: height.parse()?,
            timestamp_ms: timestamp.parse()?,
            input_index,
            prev_tx_hash: hash.into(),
            prev_output_index: index,
        });
    }
    visibility::unchanged(db, database, &snapshot).await?;
    Ok(result)
}
