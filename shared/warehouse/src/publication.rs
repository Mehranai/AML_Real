//! Shared, append-only block publication. One writer must own a network cursor.
//! Facts and complete markers stay invisible until the final sync-state insert.
use crate::{Connection, Network, contract::identifier};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub network_id: String,
    pub revision: u64,
    pub initialized: u8,
    pub start_block: u64,
    pub last_block_number: u64,
    pub last_block_hash: String,
    pub observed_rpc_height: u64,
    pub published_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub network_id: String,
    pub block_number: u64,
    pub block_hash: String,
    pub block_timestamp_unix_ms: u64,
    pub commit_id: String,
    pub revision: u64,
    pub parent_hash: String,
    pub canonical: u8,
    pub ingestion_status: String,
    pub transaction_count: u32,
    pub receipt_coverage: Option<u8>,
    pub trace_coverage: Option<u8>,
    pub prevout_coverage: Option<u8>,
    pub source_id: String,
    pub created_at_unix_ms: u64,
}

pub async fn state(db: &impl Connection, database: &str, network: Network) -> Result<State> {
    identifier(database)?;
    let rows = db.query(&format!("SELECT toJSONString(map('network_id',network_id,'revision',toString(revision),'initialized',toString(initialized),'start_block',toString(start_block),'last_block_number',toString(last_block_number),'last_block_hash',last_block_hash,'observed_rpc_height',toString(observed_rpc_height),'published_at_unix_ms',toString(published_at_unix_ms))) FROM {database}.aml_sync_state_current WHERE network_id={}", literal(network.id()))).await?;
    if rows.is_empty() {
        return Ok(State {
            network_id: network.id().into(),
            ..State::default()
        });
    }
    let row: Value = serde_json::from_str(rows.first().context("missing warehouse state")?)?;
    let number = |key: &str| -> Result<u64> {
        Ok(row[key].as_str().context("invalid state number")?.parse()?)
    };
    Ok(State {
        network_id: network.id().into(),
        revision: number("revision")?,
        initialized: u8::try_from(number("initialized")?)?,
        start_block: number("start_block")?,
        last_block_number: number("last_block_number")?,
        last_block_hash: row["last_block_hash"]
            .as_str()
            .context("missing tip hash")?
            .into(),
        observed_rpc_height: number("observed_rpc_height")?,
        published_at_unix_ms: number("published_at_unix_ms")?,
    })
}

pub async fn insert(
    db: &impl Connection,
    database: &str,
    table: &str,
    rows: &[Value],
) -> Result<()> {
    identifier(database)?;
    let contract = crate::current_contract()?;
    let schema = contract
        .tables
        .iter()
        .find(|t| t.name == table)
        .context("unsupported warehouse table")?;
    for row in rows {
        crate::semantics::validate_row(&contract, table, row)?;
        let object = row.as_object().context("warehouse row must be an object")?;
        ensure!(
            object.len() == schema.columns.len()
                && schema
                    .columns
                    .iter()
                    .all(|column| object.contains_key(&column.name)),
            "warehouse row must explicitly supply every contract column: {table}"
        );
    }
    if let Some(first) = rows.first() {
        let network = first["network_id"]
            .as_str()
            .context("missing warehouse network")?;
        ensure!(
            rows.iter().all(|row| row["network_id"] == network),
            "mixed networks in warehouse insert"
        );
        let identities = db.query(&format!("SELECT toJSONString(map('network_id',network_id)) FROM {database}.aml_warehouse_identity FINAL")).await?;
        ensure!(
            identities.len() == 1,
            "warehouse database must have one network identity"
        );
        let identity: Value = serde_json::from_str(&identities[0])?;
        ensure!(
            identity["network_id"] == network,
            "warehouse insert belongs to another network"
        );
    }
    for chunk in rows.chunks(1000) {
        let mut body = String::new();
        for row in chunk {
            body.push_str(&serde_json::to_string(row)?);
            body.push('\n');
        }
        db.insert_json(&format!("{database}.{table}"), &body)
            .await?;
    }
    Ok(())
}

pub async fn next_revision(
    db: &impl Connection,
    database: &str,
    network: Network,
    previous: u64,
) -> Result<u64> {
    identifier(database)?;
    let values = db.query(&format!("SELECT toString(max(revision)) FROM {database}.aml_ingested_blocks WHERE network_id={}", literal(network.id()))).await?;
    let written: u64 = values
        .first()
        .context("missing revision aggregate")?
        .parse()?;
    // A reservation occupies r; its facts and complete marker use r+1. After
    // a crash before the complete marker, the next reservation must exceed r+1.
    previous
        .max(written)
        .checked_add(3)
        .context("warehouse revision exhausted")
}

pub async fn reserve(db: &impl Connection, database: &str, block: &Block) -> Result<()> {
    ensure!(
        block.revision >= 2 && block.canonical == 1 && block.ingestion_status == "complete",
        "invalid complete block"
    );
    let network = Network::ALL
        .into_iter()
        .find(|network| network.id() == block.network_id)
        .context("unknown publication network")?;
    crate::migration::validate_identity(db, database, network).await?;
    let mut reservation = block.clone();
    reservation.revision -= 1;
    reservation.canonical = 0;
    reservation.ingestion_status = "processing".into();
    insert(
        db,
        database,
        "aml_ingested_blocks",
        &[serde_json::to_value(reservation)?],
    )
    .await
}

pub async fn facts(
    db: &impl Connection,
    database: &str,
    block: &Block,
    table: &str,
    rows: &[Value],
) -> Result<()> {
    ensure!(
        [
            "aml_transactions",
            "aml_address_relationships",
            "aml_transaction_features",
            "aml_semantic_aml_events",
            "aml_bitcoin_inputs",
            "aml_bitcoin_outputs",
            "aml_evm_logs",
            "aml_tron_contracts"
        ]
        .contains(&table),
        "unsupported block fact table"
    );
    let header = serde_json::to_value(block)?;
    for row in rows {
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
                "fact does not match block publication: {key}"
            );
        }
    }
    insert(db, database, table, rows).await
}

pub async fn complete(db: &impl Connection, database: &str, block: &Block) -> Result<()> {
    insert(
        db,
        database,
        "aml_ingested_blocks",
        &[serde_json::to_value(block)?],
    )
    .await
}

pub async fn publish(
    db: &impl Connection,
    database: &str,
    network: Network,
    previous: &State,
    next: &State,
) -> Result<()> {
    ensure!(
        next.network_id == network.id() && previous.network_id == network.id(),
        "publication network mismatch"
    );
    ensure!(
        next.revision > previous.revision && next.initialized <= 1,
        "invalid publication revision"
    );
    ensure!(
        state(db, database, network).await? == *previous,
        "warehouse checkpoint changed; single writer ownership required"
    );
    if next.initialized == 1 {
        ensure!(
            next.last_block_number >= next.start_block,
            "invalid publication range"
        );
        let rows = db.query(&format!("SELECT toJSONString(map('hash',block_hash,'canonical',toString(canonical),'status',ingestion_status)) FROM {database}.aml_ingested_blocks FINAL WHERE network_id={} AND block_number={} AND revision<={} ORDER BY revision DESC LIMIT 1", literal(network.id()), next.last_block_number, next.revision)).await?;
        let block: Value =
            serde_json::from_str(rows.first().context("cannot publish a missing block")?)?;
        ensure!(
            block["hash"] == next.last_block_hash
                && block["canonical"] == "1"
                && block["status"] == "complete",
            "cannot publish an incomplete or conflicting block"
        );
    }
    insert(
        db,
        database,
        "aml_sync_state",
        &[serde_json::to_value(next)?],
    )
    .await
}
