//! One read/write contract for the eight core families. Adapters retain their
//! own connection/database; migration of legacy history is a separate step.
use crate::{
    Connection, Network,
    contract::{Column, identifier},
    publication::{self, Block, State, literal},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const FACTS: [&str; 12] = [
    "aml_transactions",
    "aml_address_relationships",
    "aml_transaction_features",
    "aml_semantic_aml_events",
    "aml_bitcoin_inputs",
    "aml_bitcoin_outputs",
    "aml_evm_logs",
    "aml_tron_contracts",
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
pub(crate) fn projection(columns: &[Column]) -> String {
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
pub(crate) fn decode_rows(
    table_name: &str,
    network: Network,
    rows: Vec<String>,
) -> Result<Vec<Value>> {
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
/// Requires the same exclusive cursor ownership as publication::reserve.
pub async fn publish_batch(
    db: &impl Connection,
    database: &str,
    previous: &State,
    block: &Block,
    facts: &BTreeMap<String, Vec<Value>>,
    raw_evidence: &str,
) -> Result<State> {
    identifier(database)?;
    ensure!(
        previous.initialized <= 1 && block.revision > previous.revision,
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
    ensure!(
        block.block_number
            == if previous.initialized == 1 {
                previous
                    .last_block_number
                    .checked_add(1)
                    .context("height exhausted")?
            } else {
                block.block_number
            },
        "noncontiguous core batch"
    );
    ensure!(
        previous.initialized == 0 || block.parent_hash == previous.last_block_hash,
        "core parent mismatch"
    );
    ensure!(
        facts.contains_key("aml_transactions"),
        "transaction family coverage is required"
    );
    ensure!(
        facts["aml_transactions"].len() == block.transaction_count as usize,
        "block transaction count mismatch"
    );
    let contract = crate::current_contract()?;
    let header = serde_json::to_value(block)?;
    for (table, rows) in facts {
        ensure!(
            FACTS.contains(&table.as_str()),
            "unsupported core fact family"
        );
        for row in rows {
            crate::semantics::validate_row(&contract, table, row)?;
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
        }
    }
    let counts = facts
        .iter()
        .map(|(t, r)| (t.clone(), r.len() as u64))
        .collect::<BTreeMap<_, _>>();
    let marker = block_row(block, &counts, raw_evidence)?;
    crate::semantics::validate_row(&contract, "aml_ingested_blocks", &marker)?;
    publication::reserve(db, database, block).await?;
    for (table, rows) in facts {
        publication::facts(db, database, block, table, rows).await?;
    }
    complete(db, database, block, &counts, raw_evidence).await?;
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
        observed_rpc_height: previous.observed_rpc_height.max(block.block_number),
        published_at_unix_ms: block.created_at_unix_ms,
    };
    publication::publish(db, database, network, previous, &next).await?;
    Ok(next)
}
