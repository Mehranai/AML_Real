//! Schema-level access for analytics and specialized evidence. Creating a row
//! or a current view never implies that an analytic worker has run.
use crate::{Connection, Network, core, publication};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
#[derive(Clone, Copy)]
pub enum Mode {
    Current,
    Approved,
    /// Historical latest-row inspection. Never use this to score current risk.
    Audit,
}
fn supported(table: &str) -> Result<bool> {
    let m = crate::analytics_schema::migration()?;
    Ok(m.columns.contains_key(table) || m.tables.iter().any(|t| t.name == table))
}
pub fn select(
    database: &str,
    network: Network,
    table: &str,
    mode: Mode,
    limit: usize,
) -> Result<String> {
    crate::contract::identifier(database)?;
    ensure!(
        supported(table)? && (1..=500).contains(&limit),
        "unsupported analytics table or read limit"
    );
    let suffix = match mode {
        Mode::Approved => {
            ensure!(
                [
                    "aml_intelligence_claims",
                    "aml_protocol_registry",
                    "aml_entity_addresses"
                ]
                .contains(&table),
                "this table has no approved projection"
            );
            "active"
        }
        Mode::Current | Mode::Audit => {
            if core::FACTS.contains(&table) {
                "canonical"
            } else if matches!(mode, Mode::Current)
                && (crate::visibility::derived(table)
                    || ["aml_asset_metadata", "aml_metadata_jobs"].contains(&table))
            {
                "visible"
            } else {
                "current"
            }
        }
    };
    let contract = crate::current_contract()?;
    let schema = contract
        .tables
        .iter()
        .find(|t| t.name == table)
        .context("analytics table missing")?;
    let view = format!("{table}_{suffix}");
    ensure!(
        contract.views.iter().any(|v| v.name == view),
        "analytics view missing"
    );
    Ok(format!(
        "SELECT toJSONString(tuple({})) FROM {database}.{view} WHERE network_id={} ORDER BY {} LIMIT {limit}",
        core::projection(&schema.columns),
        publication::literal(network.id()),
        schema.order_by.join(",")
    ))
}
pub async fn read(
    db: &impl Connection,
    database: &str,
    network: Network,
    table: &str,
    mode: Mode,
    limit: usize,
) -> Result<Vec<Value>> {
    let sql = select(database, network, table, mode, limit)?;
    let before = publication::state(db, database, network).await?;
    let rows = core::decode_rows(table, network, db.query(&sql).await?)?;
    ensure!(
        publication::state(db, database, network).await? == before,
        "checkpoint changed during analytics read; retry"
    );
    Ok(rows)
}
pub async fn insert(
    db: &impl Connection,
    database: &str,
    table: &str,
    rows: &[Value],
) -> Result<()> {
    ensure!(supported(table)?, "unsupported analytics table");
    ensure!(
        !core::FACTS.contains(&table),
        "specialized facts require block publication"
    );
    publication::insert(db, database, table, rows).await
}
