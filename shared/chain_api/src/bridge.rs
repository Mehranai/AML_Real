use crate::{ApiError, Network};
use aml_warehouse::{Connection, publication::literal, values, visibility};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[path = "bridge_allbridge.rs"]
pub mod allbridge;
#[path = "bridge_tbtc.rs"]
pub mod tbtc;

pub const TRANSACTION_ROUTE: &str = "/internal/bridge/transactions/{hash}";
pub const OPERATION_ROUTE: &str = "/internal/bridge/operations/{hash}";
pub const ALLBRIDGE_OPERATION_ROUTE: &str = "/internal/bridge/allbridge/operations/{hash}";
pub const SNAPSHOT_ROUTE: &str = "/internal/bridge/snapshot";
pub const WINDOW_MS: u64 = 48 * 60 * 60 * 1000;
pub const LOG_LIMIT: usize = 4;
const BUDGET: &str = "SETTINGS max_threads=2,max_memory_usage=268435456,max_execution_time=8";

// Celer's deployed liquidity pool emits these events without indexed fields.
const SEND: &str = "89d8051e597ab4178a863a5190407b98abfeff406aa8db90c59af76612e58f01";
const RELAY: &str = "79fa08de5149d912dce8e5e8da7a7c17ccdf23dd5d3bfe196802e6eb86347c7c";

pub fn pool(network: Network) -> Option<&'static str> {
    match network {
        Network::Ethereum => Some("0x5427fefa711eff984124bfbb1ab6fbf5e3da1820"),
        Network::Bsc => Some("0xdd90e5e87a2081dcf0391920868ebc2ffb81a1af"),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Send,
    Relay,
}

impl Side {
    fn topic(self) -> &'static str {
        match self {
            Self::Send => SEND,
            Self::Relay => RELAY,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub side: Side,
    pub from_ms: u64,
    pub to_ms: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub block_number: u64,
}

pub enum Lookup {
    Transaction(Anchor),
    Operation(Window),
    AllbridgeOperation(Window),
    TbtcReveal(tbtc::RevealWindow),
    TbtcSweep(tbtc::SweepWindow),
    TbtcMint(tbtc::MintWindow),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Observation {
    pub network_id: String,
    pub source_snapshot: Value,
    pub logs: Vec<Log>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Log {
    pub tx_hash: String,
    pub block_hash: String,
    pub block_number: u64,
    pub timestamp_ms: u64,
    pub log_index: u32,
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
}

#[derive(Debug)]
pub struct Leg {
    pub side: Side,
    pub remote_chain_id: u64,
    pub operation_id: String,
    pub event_id: String,
    pub sender: String,
    pub receiver: String,
    pub token: String,
    pub amount_word: String,
}

fn digits(hex: &str) -> &str {
    hex.strip_prefix("0x").unwrap_or(hex)
}

impl Log {
    pub fn decode(&self, network: Network) -> Option<Leg> {
        if Some(self.address.as_str()) != pool(network)
            || values::normalize_hash(network, &self.tx_hash).ok()? != self.tx_hash
            || values::normalize_hash(network, &self.block_hash).ok()? != self.block_hash
        {
            return None;
        }
        let [topic] = self.topics.as_slice() else {
            return None;
        };
        let side = match digits(topic) {
            SEND => Side::Send,
            RELAY => Side::Relay,
            _ => return None,
        };
        let data = digits(&self.data);
        if !values::hex(data, if side == Side::Send { 512 } else { 448 }) {
            return None;
        }
        let word = |i: usize| &data[i * 64..(i + 1) * 64];
        let nonzero = |s: &str| s.bytes().any(|byte| byte != b'0');
        let address = |i| {
            let (padding, value) = word(i).split_at(24);
            (!nonzero(padding) && nonzero(value))
                .then(|| format!("0x{}", value.to_ascii_lowercase()))
        };
        let integer = |i| u64::from_str_radix(word(i), 16).ok();
        let event_id = word(0);
        let operation_id = match side {
            Side::Send => {
                integer(6)?;
                u32::try_from(integer(7)?).ok()?;
                event_id
            }
            Side::Relay => word(6),
        };
        if !nonzero(event_id) || !nonzero(operation_id) || !nonzero(word(4)) {
            return None;
        }
        Some(Leg {
            side,
            remote_chain_id: integer(5)?,
            operation_id: format!("0x{}", operation_id.to_ascii_lowercase()),
            event_id: format!("0x{}", event_id.to_ascii_lowercase()),
            sender: address(1)?,
            receiver: address(2)?,
            token: address(3)?,
            amount_word: word(4).into(),
        })
    }
}

pub async fn lookup(
    db: &impl Connection,
    database: &str,
    network: Network,
    hash: &str,
    lookup: Lookup,
) -> Result<Observation, ApiError> {
    let (celer, allbridge) = match &lookup {
        Lookup::Transaction(_) => (pool(network), allbridge::emitter(network)),
        Lookup::Operation(_) => (pool(network), None),
        Lookup::AllbridgeOperation(_) => (None, allbridge::emitter(network)),
        Lookup::TbtcReveal(_) | Lookup::TbtcSweep(_) | Lookup::TbtcMint(_) => (None, None),
    };
    let bitcoin_hash = matches!(lookup, Lookup::TbtcReveal(_) | Lookup::TbtcSweep(_));
    let tbtc = bitcoin_hash || matches!(lookup, Lookup::TbtcMint(_));
    if (tbtc && network != Network::Ethereum) || (!tbtc && celer.is_none() && allbridge.is_none()) {
        return Err(invalid("Unsupported bridge network"));
    }
    let hash_network = if matches!(lookup, Lookup::Transaction(_)) {
        network
    } else if bitcoin_hash {
        Network::Bitcoin
    } else {
        Network::Ethereum
    };
    let hash = values::normalize_hash(hash_network, hash)
        .map_err(|_| invalid("Invalid bridge lookup hash"))?;
    if lookup
        .window()
        .is_some_and(|(from, to, max, _, _)| !to.checked_sub(from).is_some_and(|span| span <= max))
    {
        return Err(invalid("Invalid bridge lookup window"));
    }
    observe(db, database, network, (celer, allbridge), &hash, lookup)
        .await
        .map_err(unavailable)
}

impl Lookup {
    fn window(&self) -> Option<(u64, u64, u64, &'static str, u32)> {
        let (window, topic, offset) = match self {
            Self::Transaction(_) => return None,
            Self::TbtcReveal(window) => {
                return Some((
                    window.from_ms,
                    window.to_ms,
                    2 * WINDOW_MS,
                    tbtc::REVEAL_TOPIC,
                    1,
                ));
            }
            Self::TbtcSweep(window) => {
                return Some((
                    window.from_ms,
                    window.to_ms,
                    tbtc::PROOF_DELAY_MS + tbtc::CLOCK_SKEW_MS,
                    tbtc::SWEEP_TOPIC,
                    65,
                ));
            }
            Self::TbtcMint(window) => {
                return Some((
                    window.from_ms,
                    window.to_ms,
                    tbtc::MINT_WINDOW_MS,
                    tbtc::MINT_FINALIZED_TOPIC,
                    0,
                ));
            }
            Self::Operation(window) => {
                let offset = if window.side == Side::Send { 1 } else { 385 };
                (window, window.side.topic(), offset)
            }
            Self::AllbridgeOperation(window) => {
                let topic = if window.side == Side::Send {
                    allbridge::SEND
                } else {
                    allbridge::RECEIVE
                };
                (window, topic, 257)
            }
        };
        Some((window.from_ms, window.to_ms, WINDOW_MS, topic, offset))
    }
}

pub async fn snapshot(
    db: &impl Connection,
    database: &str,
    network: Network,
) -> Result<Value, ApiError> {
    Ok(visibility::capture(db, database, network)
        .await
        .map_err(unavailable)?
        .value())
}

fn invalid(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, message)
}

fn unavailable(error: anyhow::Error) -> ApiError {
    eprintln!("Bridge canonical evidence lookup failed: {error:#}");
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "Canonical bridge evidence unavailable",
    )
}

async fn observe(
    db: &impl Connection,
    database: &str,
    network: Network,
    emitters: (Option<&str>, Option<&str>),
    hash: &str,
    lookup: Lookup,
) -> anyhow::Result<Observation> {
    let snapshot = visibility::capture(db, database, network).await?;
    let mut result = Observation {
        network_id: network.id().into(),
        source_snapshot: snapshot.value(),
        logs: Vec::new(),
        truncated: false,
    };
    let state = snapshot.state();
    if state.initialized == 0 {
        return Ok(result);
    }
    let tron = network == Network::Tron;
    let (log_table, data_column, live_filter) = if tron {
        ("aml_tron_logs", "data_hex", "1")
    } else {
        ("aml_evm_logs", "data", "ifNull(removed,0)=0")
    };
    let mut emitters_sql = Vec::new();
    for (address, send, relay) in [
        (emitters.0, SEND, RELAY),
        (emitters.1, allbridge::SEND, allbridge::RECEIVE),
    ] {
        if let Some(address) = address {
            emitters_sql.push(format!(
                "(address={} AND topics[1] IN ('{send}','0x{send}','{relay}','0x{relay}'))",
                literal(address)
            ));
        }
    }
    let bitcoin_hash = matches!(lookup, Lookup::TbtcReveal(_) | Lookup::TbtcSweep(_));
    let mint = matches!(lookup, Lookup::TbtcMint(_));
    let (emitters_sql, topic_count) = match &lookup {
        Lookup::TbtcReveal(_) => (format!("address={}", literal(tbtc::EMITTER)), 3),
        Lookup::TbtcSweep(_) => (format!("address={}", literal(tbtc::EMITTER)), 1),
        Lookup::TbtcMint(_) => (format!("address={}", literal(tbtc::VAULT)), 4),
        _ => (emitters_sql.join(" OR "), 1),
    };
    let network = literal(network.id());
    let (from, to, filter) = match &lookup {
        Lookup::Transaction(anchor) => (
            anchor.block_number,
            anchor.block_number,
            format!("tx_hash={}", literal(hash)),
        ),
        _ => {
            let (from_ms, to_ms, _, topic, offset) = lookup.window().expect("operation window");
            // Bounds may include displaced block versions. The canonical join below
            // selects the published occurrence at each height.
            let rows = db.query_bounded(&format!(
                "SELECT toJSONString(tuple(toString(minOrNull(block_number)),toString(maxOrNull(block_number))))
                FROM {database}.aml_ingested_blocks WHERE network_id={network}
                  AND block_number BETWEEN {} AND {} AND revision<={}
                  AND block_timestamp_unix_ms BETWEEN {} AND {} {BUDGET}",
                state.start_block, state.last_block_number, state.revision, from_ms, to_ms,
            )).await?;
            let row = rows
                .first()
                .ok_or_else(|| anyhow::anyhow!("missing bridge block bounds"))?;
            let (from, to): (Option<String>, Option<String>) = serde_json::from_str(row)?;
            let (Some(from), Some(to)) = (from, to) else {
                visibility::unchanged(db, database, &snapshot).await?;
                return Ok(result);
            };
            let hash_word = if bitcoin_hash {
                tbtc::reverse_hash(hash)
                    .ok_or_else(|| anyhow::anyhow!("invalid Bitcoin transaction hash"))?
            } else {
                digits(hash).into()
            };
            let hash_filter = if mint {
                format!("topics[3] IN ('{hash_word}','0x{hash_word}')")
            } else {
                format!(
                    "lower(substring(if(startsWith({data_column},'0x'),substring({data_column},3),{data_column}),{offset},64))={}",
                    literal(&hash_word),
                )
            };
            let filter = format!(
                "block_timestamp_unix_ms BETWEEN {from_ms} AND {to_ms} AND topics[1] IN ('{topic}','0x{topic}')
                 AND {hash_filter}",
            );
            let filter = match &lookup {
                Lookup::TbtcReveal(tbtc::RevealWindow {
                    output_index: Some(index),
                    ..
                }) => format!(
                    "{filter} AND lower(substring(if(startsWith(data,'0x'),substring(data,3),data),65,64))='{index:064x}'"
                ),
                _ => filter,
            };
            (from.parse()?, to.parse()?, filter)
        }
    };
    if from < state.start_block || to > state.last_block_number {
        visibility::unchanged(db, database, &snapshot).await?;
        return Ok(result);
    }
    anyhow::ensure!(
        to.checked_sub(from).is_some_and(|n| n <= 1_000_000),
        "bridge block range exceeds bound"
    );
    let rows = db.query_bounded(&format!(
        "SELECT toJSONString(tuple(tx_hash,block_hash,toString(block_number),toString(block_timestamp_unix_ms),log_index,address,topics,{data_column},commit_id,toString(revision)))
        FROM {database}.{log_table} FINAL
        WHERE network_id={network} AND block_number BETWEEN {from} AND {to}
          AND (block_number,block_hash,block_timestamp_unix_ms,commit_id,revision) IN (
            SELECT block_number,block_hash,block_timestamp_unix_ms,commit_id,revision
            FROM {database}.aml_ingested_blocks_canonical
            WHERE network_id={network} AND block_number BETWEEN {from} AND {to}
          )
          AND ({emitters_sql}) AND {live_filter} AND length(topics)={topic_count}
          AND length({data_column})<=514 AND ({filter})
        LIMIT {} {BUDGET}",
        LOG_LIMIT + 1
    )).await?;
    result.truncated = rows.len() > LOG_LIMIT;
    for row in rows.into_iter().take(LOG_LIMIT) {
        let (
            tx_hash,
            block_hash,
            block,
            timestamp,
            log_index,
            address,
            topics,
            data,
            commit,
            revision,
        ): (
            String,
            String,
            String,
            String,
            u32,
            String,
            Vec<String>,
            String,
            String,
            String,
        ) = serde_json::from_str(&row)?;
        let block: u64 = block.parse()?;
        if tron || bitcoin_hash || mint {
            // The raw log tables can retain failure receipts. Require a success
            // in the same published occurrence for these bridge claims.
            let revision: u64 = revision.parse()?;
            let statuses = db.query_bounded(&format!(
                "SELECT toString(status) FROM {database}.aml_transactions FINAL WHERE network_id={network}
                   AND block_number={block} AND block_hash={} AND tx_hash={} AND commit_id={}
                   AND revision={revision} LIMIT 2 {BUDGET}",
                literal(&block_hash), literal(&tx_hash), literal(&commit)
            )).await?;
            if statuses != ["success"] {
                continue;
            }
        }
        result.logs.push(Log {
            tx_hash,
            block_hash,
            block_number: block,
            timestamp_ms: timestamp.parse()?,
            log_index,
            address,
            topics,
            data,
        });
    }
    visibility::unchanged(db, database, &snapshot).await?;
    Ok(result)
}
