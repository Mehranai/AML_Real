//! Shared live block validation, persistence, publication and canonical reads.
//! Each adapter keeps its own connection/database and source decoder.
use crate::{
    Connection, Network,
    contract::{Column, identifier},
    publication::{self, Block, State, literal},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const FACTS: [&str; 15] = [
    "aml_transactions",
    "aml_address_relationships",
    "aml_transaction_features",
    "aml_semantic_aml_events",
    "aml_bitcoin_inputs",
    "aml_bitcoin_outputs",
    "aml_evm_logs",
    "aml_tron_contracts",
    "aml_tron_logs",
    "aml_tron_internals",
    "aml_tron_resources",
    "aml_metadata_discoveries",
    "aml_bitcoin_scripts",
    "aml_evm_traces",
    "aml_raw_transactions",
];
#[derive(Clone, Copy, Debug)]
pub enum View {
    Blocks,
    Transactions,
    Relationships,
    Features,
    Events,
    State,
    Failures,
    Benchmarks,
}
impl View {
    pub const ALL: [Self; 8] = [
        Self::Blocks,
        Self::Transactions,
        Self::Relationships,
        Self::Features,
        Self::Events,
        Self::State,
        Self::Failures,
        Self::Benchmarks,
    ];
    pub fn table(self) -> &'static str {
        match self {
            Self::Blocks => "aml_ingested_blocks",
            Self::Transactions => "aml_transactions",
            Self::Relationships => "aml_address_relationships",
            Self::Features => "aml_transaction_features",
            Self::Events => "aml_semantic_aml_events",
            Self::State => "aml_sync_state",
            Self::Failures => "aml_ingestion_failures",
            Self::Benchmarks => "aml_ingestion_benchmarks",
        }
    }
    pub fn name(self) -> String {
        format!(
            "{}_{}",
            self.table(),
            if matches!(self, Self::State | Self::Failures | Self::Benchmarks) {
                "current"
            } else {
                "canonical"
            }
        )
    }
    fn has_height(self) -> bool {
        !matches!(self, Self::State | Self::Benchmarks)
    }
}
#[derive(Clone, Debug)]
pub struct Query {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: usize,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            limit: 100,
        }
    }
}
pub fn projection(columns: &[Column]) -> String {
    columns
        .iter()
        .map(|c| {
            let scalar = c
                .data_type
                .strip_prefix("Nullable(")
                .and_then(|v| v.strip_suffix(')'))
                .unwrap_or(&c.data_type);
            if matches!(scalar, "UInt64" | "UInt256") {
                format!("toString(`{}`)", c.name)
            } else {
                format!("`{}`", c.name)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
pub fn select(database: &str, network: Network, view: View, query: &Query) -> Result<String> {
    identifier(database)?;
    ensure!(
        (1..=500).contains(&query.limit),
        "core read limit must be 1..500"
    );
    ensure!(
        query.from.unwrap_or(0) <= query.to.unwrap_or(u64::MAX),
        "invalid core block range"
    );
    ensure!(
        view.has_height() || (query.from.is_none() && query.to.is_none()),
        "this core view has no block range"
    );
    let contract = crate::current_contract()?;
    let table = contract
        .tables
        .iter()
        .find(|t| t.name == view.table())
        .context("core table missing")?;
    let mut filter = format!("network_id={}", literal(network.id()));
    if let Some(from) = query.from {
        filter.push_str(&format!(" AND block_number>={from}"));
    }
    if let Some(to) = query.to {
        filter.push_str(&format!(" AND block_number<={to}"));
    }
    Ok(format!(
        "SELECT toJSONString(tuple({})) FROM {database}.{} WHERE {filter} ORDER BY {} LIMIT {}",
        projection(&table.columns),
        view.name(),
        table.order_by.join(","),
        query.limit
    ))
}
fn decode_value(value: &mut Value, data_type: &str) -> Result<()> {
    if value.is_null() {
        return Ok(());
    }
    let scalar = data_type
        .strip_prefix("Nullable(")
        .and_then(|v| v.strip_suffix(')'))
        .unwrap_or(data_type);
    if scalar == "UInt64" {
        *value = json!(
            value
                .as_str()
                .context("UInt64 must be returned as exact string")?
                .parse::<u64>()?
        );
    }
    if scalar == "UInt256" {
        ensure!(value.is_string(), "raw amount must remain an exact string");
    }
    if scalar == "Float64" {
        *value = json!(
            value
                .as_f64()
                .context("Float64 must be returned as a number")?
        );
    }
    Ok(())
}
pub async fn read(
    db: &impl Connection,
    database: &str,
    network: Network,
    view: View,
    query: &Query,
) -> Result<Vec<Value>> {
    let sql = select(database, network, view, query)?;
    let before = publication::state(db, database, network).await?;
    let rows = decode_rows(view.table(), network, db.query(&sql).await?)?;
    ensure!(
        publication::state(db, database, network).await? == before,
        "checkpoint changed during core read; retry"
    );
    Ok(rows)
}
pub fn decode_rows(table_name: &str, network: Network, rows: Vec<String>) -> Result<Vec<Value>> {
    let contract = crate::current_contract()?;
    let table = contract
        .tables
        .iter()
        .find(|t| t.name == table_name)
        .context("reader table missing")?;
    rows.into_iter()
        .map(|line| {
            let values: Vec<Value> = serde_json::from_str(&line)?;
            ensure!(
                values.len() == table.columns.len(),
                "core reader column count mismatch"
            );
            let mut row = serde_json::Map::new();
            for (mut value, column) in values.into_iter().zip(&table.columns) {
                decode_value(&mut value, &column.data_type)?;
                row.insert(column.name.clone(), value);
            }
            let row = Value::Object(row);
            ensure!(
                row["network_id"] == network.id(),
                "foreign network row in core response"
            );
            crate::semantics::validate_row(&contract, table_name, &row)?;
            Ok(row)
        })
        .collect()
}

pub fn block_row(
    block: &Block,
    counts: &BTreeMap<String, u64>,
    raw_evidence: &str,
) -> Result<Value> {
    ensure!(
        serde_json::from_str::<Value>(raw_evidence).is_ok(),
        "block source evidence must be JSON"
    );
    ensure!(
        counts.keys().all(|t| FACTS.contains(&t.as_str())),
        "unknown fact family in block counts"
    );
    let mut row = crate::core_schema::extend("aml_ingested_blocks", serde_json::to_value(block)?)?;
    row["expected_counts_json"] = json!(serde_json::to_string(counts)?);
    row["raw_evidence_json"] = json!(raw_evidence);
    Ok(row)
}
pub async fn complete(
    db: &impl Connection,
    database: &str,
    block: &Block,
    counts: &BTreeMap<String, u64>,
    raw_evidence: &str,
) -> Result<()> {
    identifier(database)?;
    ensure!(
        counts.get("aml_transactions") == Some(&u64::from(block.transaction_count)),
        "complete marker requires exact transaction-family coverage"
    );
    ensure!(
        block.canonical == 1 && block.ingestion_status == "complete",
        "invalid complete core marker"
    );
    let row = block_row(block, counts, raw_evidence)?;
    crate::semantics::validate_row(&crate::current_contract()?, "aml_ingested_blocks", &row)?;
    for (table, expected) in counts {
        let actual=db.query(&format!("SELECT toString(count()) FROM {database}.{table} FINAL WHERE network_id={} AND block_number={} AND block_hash={} AND commit_id={} AND revision={}",literal(&block.network_id),block.block_number,literal(&block.block_hash),literal(&block.commit_id),block.revision)).await?;
        ensure!(
            actual
                .first()
                .context("missing fact count")?
                .parse::<u64>()?
                == *expected,
            "incomplete core commit: {table}"
        );
    }
    publication::insert(db, database, "aml_ingested_blocks", &[row]).await
}
/// Validate the entire source batch before any database access. An empty family
/// is allowed on every network; unrelated specialized families cannot hold data.
pub fn validate_batch(
    previous: &State,
    block: &Block,
    facts: &BTreeMap<String, Vec<Value>>,
    raw_evidence: &str,
) -> Result<BTreeMap<String, u64>> {
    crate::visibility::Snapshot::from_state(previous.clone())?;
    ensure!(
        block.revision >= 2 && block.revision > previous.revision,
        "invalid core publication fence"
    );
    ensure!(
        block.canonical == 1 && block.ingestion_status == "complete",
        "a core batch must describe a complete block"
    );
    let network = Network::parse(&block.network_id)?;
    ensure!(
        previous.network_id == network.id(),
        "core batch network mismatch"
    );
    if previous.initialized == 1 {
        ensure!(
            previous.last_block_number.checked_add(1) == Some(block.block_number),
            "noncontiguous core batch"
        );
        ensure!(
            block.parent_hash == previous.last_block_hash,
            "core parent mismatch"
        );
    }
    validate_content(block, facts, raw_evidence)
}

fn validate_content(
    block: &Block,
    facts: &BTreeMap<String, Vec<Value>>,
    raw_evidence: &str,
) -> Result<BTreeMap<String, u64>> {
    let network = Network::parse(&block.network_id)?;
    ensure!(
        block.canonical == 1 && block.ingestion_status == "complete",
        "incomplete source batch"
    );
    let transactions = facts
        .get("aml_transactions")
        .context("transaction family coverage is required")?;
    ensure!(
        transactions.len() == block.transaction_count as usize,
        "block transaction count mismatch"
    );
    let contract = crate::current_contract()?;
    let header = serde_json::to_value(block)?;
    let mut transactions_by_hash = BTreeMap::new();
    let mut positions = BTreeSet::new();
    for row in transactions {
        let hash = row["tx_hash"]
            .as_str()
            .context("transaction hash missing")?;
        let position = row["transaction_index"]
            .as_u64()
            .context("transaction position missing")?;
        ensure!(
            transactions_by_hash.insert(hash, position).is_none(),
            "duplicate transaction hash in block"
        );
        ensure!(
            position < u64::from(block.transaction_count) && positions.insert(position),
            "duplicate or noncontiguous transaction position"
        );
    }
    for (table, rows) in facts {
        ensure!(
            FACTS.contains(&table.as_str()),
            "unsupported core fact family"
        );
        let applicable = match table.as_str() {
            "aml_bitcoin_inputs" | "aml_bitcoin_outputs" | "aml_bitcoin_scripts" => {
                network == Network::Bitcoin
            }
            "aml_evm_logs" | "aml_evm_traces" => {
                matches!(network, Network::Ethereum | Network::Bsc)
            }
            "aml_tron_contracts" | "aml_tron_logs" | "aml_tron_internals"
            | "aml_tron_resources" => network == Network::Tron,
            _ => true,
        };
        ensure!(
            applicable || rows.is_empty(),
            "inapplicable specialized fact family: {table}"
        );
        let schema = contract
            .tables
            .iter()
            .find(|t| &t.name == table)
            .context("fact schema missing")?;
        let mut keys = BTreeSet::new();
        for row in rows {
            if table == "aml_address_relationships" {
                crate::relationships::validate_with_contract(&contract, row)?;
            } else {
                crate::semantics::validate_row(&contract, table, row)?;
            }
            for key in [
                "network_id",
                "block_number",
                "block_hash",
                "block_timestamp_unix_ms",
                "commit_id",
                "revision",
            ] {
                ensure!(
                    row.get(key) == header.get(key),
                    "core row header mismatch: {key}"
                );
            }
            let key = serde_json::to_string(
                &schema
                    .order_by
                    .iter()
                    .map(|column| &row[column])
                    .collect::<Vec<_>>(),
            )?;
            ensure!(keys.insert(key), "duplicate stored fact identity: {table}");
            let transaction = row["tx_hash"]
                .as_str()
                .context("fact transaction hash missing")?;
            let position = transactions_by_hash
                .get(transaction)
                .context("fact references a transaction outside its block")?;
            if let Some(index) = row.get("transaction_index").filter(|v| !v.is_null()) {
                ensure!(
                    index.as_u64() == Some(*position),
                    "fact transaction position mismatch: {table}"
                );
            }
        }
    }
    let counts = facts
        .iter()
        .map(|(t, r)| (t.clone(), r.len() as u64))
        .collect();
    let marker = block_row(block, &counts, raw_evidence)?;
    crate::semantics::validate_row(&contract, "aml_ingested_blocks", &marker)?;
    Ok(counts)
}

/// Compare every persisted column with the source batch. Counts alone cannot
/// detect a changed amount, raw payload, token identity or source position.
pub async fn verify_facts<K: AsRef<str> + Ord + Sync>(
    db: &impl Connection,
    database: &str,
    block: &Block,
    facts: &BTreeMap<K, Vec<Value>>,
) -> Result<()> {
    identifier(database)?;
    let network = Network::parse(&block.network_id)?;
    let contract = crate::current_contract()?;
    for (name, expected) in facts {
        let table = name.as_ref();
        ensure!(FACTS.contains(&table), "unsupported core fact family");
        let schema = contract
            .tables
            .iter()
            .find(|t| t.name == table)
            .context("fact schema missing")?;
        let sql = format!(
            "SELECT toJSONString(tuple({})) FROM {database}.{table} FINAL WHERE network_id={} AND block_number={} AND block_hash={} AND commit_id={} AND revision={}",
            projection(&schema.columns),
            literal(network.id()),
            block.block_number,
            literal(&block.block_hash),
            literal(&block.commit_id),
            block.revision
        );
        // A real Bitcoin block can exceed a connection's response-byte bound.
        // Read in storage-key order while retaining exact comparison of every
        // column and one extra row to detect unexpected persisted evidence.
        let mut actual = Vec::new();
        let maximum = expected
            .len()
            .checked_add(1)
            .context("fact count overflow")?;
        while actual.len() < maximum {
            let limit = 256.min(maximum - actual.len());
            let page_sql = format!(
                "{sql} ORDER BY {} LIMIT {limit} OFFSET {}",
                schema.order_by.join(","),
                actual.len()
            );
            let page = decode_rows(table, network, db.query(&page_sql).await?)?;
            let count = page.len();
            ensure!(count <= limit, "fact read exceeded its page bound");
            actual.extend(page);
            if count < limit {
                break;
            }
        }
        let normalize = |mut row: Value| -> Result<String> {
            for column in &schema.columns {
                if column.data_type == "Float64" {
                    row[&column.name] =
                        json!(row[&column.name].as_f64().context("invalid stored float")?);
                }
            }
            Ok(serde_json::to_string(
                &schema
                    .columns
                    .iter()
                    .map(|column| &row[&column.name])
                    .collect::<Vec<_>>(),
            )?)
        };
        let mut expected = expected
            .iter()
            .cloned()
            .map(&normalize)
            .collect::<Result<Vec<_>>>()?;
        let mut actual = actual
            .into_iter()
            .map(normalize)
            .collect::<Result<Vec<_>>>()?;
        expected.sort();
        actual.sort();
        ensure!(
            actual == expected,
            "persisted core evidence differs from source batch: {table}"
        );
    }
    Ok(())
}

/// Requires the same exclusive cursor ownership as publication::reserve.
/// Callers without a separate RPC observation retain the previous observed tip.
pub async fn publish_batch(
    db: &impl Connection,
    database: &str,
    previous: &State,
    block: &Block,
    facts: &BTreeMap<String, Vec<Value>>,
    raw_evidence: &str,
) -> Result<State> {
    publish_observed_batch(
        db,
        database,
        previous,
        block,
        facts,
        raw_evidence,
        previous.observed_rpc_height.max(block.block_number),
    )
    .await
}

/// The common live writer, including exact read-back before the visibility fence.
pub async fn publish_observed_batch(
    db: &impl Connection,
    database: &str,
    previous: &State,
    block: &Block,
    facts: &BTreeMap<String, Vec<Value>>,
    raw_evidence: &str,
    observed_rpc_height: u64,
) -> Result<State> {
    identifier(database)?;
    let counts = validate_batch(previous, block, facts, raw_evidence)?;
    ensure!(
        observed_rpc_height >= block.block_number,
        "RPC observation precedes the block being published"
    );
    let network = Network::parse(&block.network_id)?;
    let next = State {
        network_id: network.id().into(),
        revision: block.revision,
        initialized: 1,
        start_block: if previous.initialized == 1 {
            previous.start_block
        } else {
            block.block_number
        },
        last_block_number: block.block_number,
        last_block_hash: block.block_hash.clone(),
        observed_rpc_height,
        published_at_unix_ms: block.created_at_unix_ms,
    };
    persist_verified(
        db,
        database,
        previous,
        block,
        facts,
        raw_evidence,
        &counts,
        &next,
    )
    .await?;
    Ok(next)
}

/// Same-branch evidence refresh, e.g. adding trace coverage to an older block.
/// It cannot replace a block hash, change header/transaction count or lower
/// known coverage. The cursor remains at the existing tip; its revision changes.
pub async fn refresh_observed_batch(
    db: &impl Connection,
    database: &str,
    previous: &State,
    block: &Block,
    facts: &BTreeMap<String, Vec<Value>>,
    raw_evidence: &str,
    observed_rpc_height: u64,
) -> Result<State> {
    identifier(database)?;
    crate::visibility::Snapshot::from_state(previous.clone())?;
    ensure!(
        previous.initialized == 1
            && previous.network_id == block.network_id
            && block.block_number >= previous.start_block
            && block.block_number <= previous.last_block_number
            && block.revision > previous.revision
            && observed_rpc_height >= block.block_number,
        "invalid same-branch refresh"
    );
    let network = Network::parse(&block.network_id)?;
    let counts = validate_content(block, facts, raw_evidence)?;
    let old = read(
        db,
        database,
        network,
        View::Blocks,
        &Query {
            from: Some(block.block_number),
            to: Some(block.block_number),
            limit: 1,
        },
    )
    .await?;
    let old = old
        .first()
        .context("refresh requires an existing canonical block")?;
    let new = serde_json::to_value(block)?;
    for key in [
        "block_hash",
        "parent_hash",
        "block_timestamp_unix_ms",
        "transaction_count",
    ] {
        ensure!(
            old[key] == new[key],
            "refresh cannot change canonical source header: {key}"
        );
    }
    for key in ["receipt_coverage", "trace_coverage", "prevout_coverage"] {
        if let Some(known) = old[key].as_u64() {
            ensure!(
                new[key].as_u64().is_some_and(|value| value >= known),
                "refresh cannot lower known coverage: {key}"
            );
        }
    }
    let mut next = previous.clone();
    next.revision = block.revision;
    next.observed_rpc_height = observed_rpc_height;
    next.published_at_unix_ms = block.created_at_unix_ms;
    persist_verified(
        db,
        database,
        previous,
        block,
        facts,
        raw_evidence,
        &counts,
        &next,
    )
    .await?;
    Ok(next)
}

#[allow(clippy::too_many_arguments)]
async fn persist_verified(
    db: &impl Connection,
    database: &str,
    previous: &State,
    block: &Block,
    facts: &BTreeMap<String, Vec<Value>>,
    raw_evidence: &str,
    counts: &BTreeMap<String, u64>,
    next: &State,
) -> Result<()> {
    let network = Network::parse(&block.network_id)?;
    ensure!(
        publication::state(db, database, network).await? == *previous,
        "checkpoint changed before core write; exclusive writer required"
    );
    publication::reserve(db, database, block).await?;
    for (table, rows) in facts {
        publication::facts(db, database, block, table, rows).await?;
    }
    verify_facts(db, database, block, facts).await?;
    complete(db, database, block, &counts, raw_evidence).await?;
    publication::publish(db, database, network, previous, next).await
}
