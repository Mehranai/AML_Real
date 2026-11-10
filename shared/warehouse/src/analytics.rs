//! Schema-level access for analytics and specialized evidence. Creating a row
//! or a current view never implies that an analytic worker has run.
use crate::{Connection, Network, core, publication};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// Operational evidence used by investigation/metadata, separate from chain facts.
/// Fingerprint complete latest rows, including updates sharing a revision millisecond.
pub async fn operational_snapshot(
    db: &impl Connection,
    database: &str,
    network: Network,
) -> Result<Value> {
    crate::contract::identifier(database)?;
    let tables = [
        "aml_asset_metadata",
        "aml_intelligence_sources",
        "aml_intelligence_claims",
        "aml_intelligence_reviews",
        "aml_cluster_claims",
        "aml_cluster_memberships",
        "aml_cluster_runs",
        "aml_cluster_versions",
        "aml_exposure_seeds",
        "aml_exposure_runs",
        "aml_exposure_paths",
        "aml_address_exposures",
    ];
    let checksums=[1,9,17,25].map(|offset|format!("toString(groupBitXor(reinterpretAsUInt64(substring(SHA256(toJSONString(tuple(*))),{offset},8))))")).join(",");
    let selections=tables.iter().map(|table|format!("(SELECT tuple(toString(count()),toString(max(revision)),{checksums}) FROM {database}.{table} FINAL WHERE network_id={})",publication::literal(network.id()))).collect::<Vec<_>>().join(",");
    let rows = db
        .query(&format!("SELECT toJSONString(tuple({selections}))"))
        .await?;
    let raw = rows.first().context("operational snapshot missing")?;
    let versions: Value = serde_json::from_str(raw)?;
    use sha2::{Digest, Sha256};
    Ok(
        serde_json::json!({"contract":"operational_v1","network_id":network.id(),
        "snapshot_id":format!("{:x}",Sha256::digest(format!("{}:{raw}",network.id()).as_bytes())),
        "tables":tables,"versions":versions}),
    )
}
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
