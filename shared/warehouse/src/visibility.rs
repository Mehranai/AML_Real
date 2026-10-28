//! Source snapshot and derived-publication boundaries. Requires one cursor owner.
use crate::{
    Connection, Network,
    publication::{self, State, literal},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
pub const VERSION: &str = "canonical_v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    state: State,
}
impl Snapshot {
    pub fn from_state(state: State) -> Result<Self> {
        let network = Network::parse(&state.network_id)?;
        ensure!(state.initialized <= 1, "invalid snapshot state");
        if state.initialized == 1 {
            ensure!(
                state.revision > 0
                    && state.start_block <= state.last_block_number
                    && crate::values::normalize_hash(network, &state.last_block_hash)?
                        == state.last_block_hash,
                "invalid initialized snapshot"
            );
        } else {
            ensure!(
                state.last_block_number == 0 && state.last_block_hash.is_empty(),
                "uninitialized snapshot carries a tip"
            );
        }
        Ok(Self { state })
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn id(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&self.state).expect("snapshot serialization"))
        )
    }
    pub fn value(&self) -> Value {
        json!({"contract":VERSION,"snapshot_id":self.id(),"network_id":self.state.network_id,
        "source_revision":self.state.revision.to_string(),"initialized":self.state.initialized==1,
        "start_block":self.state.start_block,"as_of_block":if self.state.initialized==1{Some(self.state.last_block_number)}else{None},
        "block_hash":self.state.last_block_hash,"observed_rpc_height":self.state.observed_rpc_height,
        "successor_blocks_observed":if self.state.initialized==1{self.state.observed_rpc_height.checked_sub(self.state.last_block_number)}else{None},
        "finality":"not_asserted","history_complete":self.state.initialized==1&&self.state.start_block==0,"mempool_included":false})
    }
}
/// Includes the block itself, matching the existing Bitcoin ingestion setting.
/// This is a depth policy, never proof of consensus finality.
pub fn confirmed_tip(observed_height: u64, required_confirmations: u64) -> Option<u64> {
    required_confirmations
        .checked_sub(1)
        .and_then(|depth| observed_height.checked_sub(depth))
}
pub async fn capture(db: &impl Connection, database: &str, network: Network) -> Result<Snapshot> {
    crate::migration::validate_identity(db, database, network).await?;
    Snapshot::from_state(publication::state(db, database, network).await?)
}
pub async fn unchanged(db: &impl Connection, database: &str, snapshot: &Snapshot) -> Result<()> {
    ensure!(
        publication::state(db, database, Network::parse(&snapshot.state.network_id)?).await?
            == snapshot.state,
        "source snapshot changed; retry or recompute"
    );
    Ok(())
}
pub fn family_tables(family: &str) -> Result<&'static [&'static str]> {
    Ok(match family {
        "cluster" => &[
            "aml_cluster_runs",
            "aml_cluster_claims",
            "aml_cluster_memberships",
            "aml_cluster_versions",
        ],
        "exposure" => &[
            "aml_exposure_runs",
            "aml_exposure_paths",
            "aml_address_exposures",
        ],
        "risk" => &["aml_wallet_risk_assessments", "aml_wallet_risk_signals"],
        _ => anyhow::bail!("unsupported analysis family"),
    })
}
pub fn derived(table: &str) -> bool {
    ["cluster", "exposure", "risk"]
        .into_iter()
        .any(|f| family_tables(f).unwrap().contains(&table))
}
/// Write one immutable analysis attempt and seal it only after row-count and
/// source-fence verification. A crashed attempt is audit-only. Use a fresh ID.
pub async fn publish_analysis(
    db: &impl Connection,
    database: &str,
    snapshot: &Snapshot,
    family: &str,
    id: &str,
    output_revision: u64,
    rows: &BTreeMap<String, Vec<Value>>,
) -> Result<()> {
    crate::contract::identifier(database)?;
    let tables = family_tables(family)?;
    ensure!(
        !id.is_empty() && output_revision > 0 && snapshot.state.initialized == 1,
        "invalid analysis publication"
    );
    ensure!(
        rows.keys().all(|t| tables.contains(&t.as_str()))
            && rows.get(tables[0]).is_some_and(|r| r.len() == 1),
        "analysis must contain one parent and only its family tables"
    );
    let contract = crate::current_contract()?;
    let key = if family == "risk" {
        "assessment_id"
    } else {
        "run_id"
    };
    let mut counts = BTreeMap::new();
    for table in tables {
        let batch = rows.get(*table).map(Vec::as_slice).unwrap_or(&[]);
        let mut keys = HashSet::new();
        let schema = contract
            .tables
            .iter()
            .find(|t| t.name == *table)
            .context("analysis schema missing")?;
        for row in batch {
            crate::semantics::validate_row(&contract, table, row)?;
            ensure!(
                row["network_id"] == snapshot.state.network_id
                    && row[key] == id
                    && row["revision"] == output_revision,
                "analysis row identity/revision mismatch"
            );
            if row.get("source_revision").is_some() {
                ensure!(
                    row["source_revision"] == snapshot.state.revision
                        && row["as_of_block"] == snapshot.state.last_block_number,
                    "analysis source snapshot mismatch"
                );
            }
            ensure!(
                keys.insert(serde_json::to_string(
                    &schema.order_by.iter().map(|k| &row[k]).collect::<Vec<_>>()
                )?),
                "duplicate analysis row key"
            );
        }
        counts.insert(*table, batch.len() as u64);
    }
    let parent = &rows[tables[0]][0];
    if family != "risk" {
        ensure!(
            parent["status"] == "complete"
                && parent["truncated"] == 0
                && parent["completed_at_unix_ms"].is_u64(),
            "analysis parent is incomplete or truncated"
        );
    }
    if family == "risk" {
        ensure!(
            parent["signal_count"].as_u64() == Some(counts["aml_wallet_risk_signals"]),
            "risk signal count mismatch"
        );
    }
    if family == "exposure" && !parent["path_count"].is_null() {
        ensure!(
            parent["path_count"].as_u64() == Some(counts["aml_exposure_paths"]),
            "exposure path count mismatch"
        );
    }
    unchanged(db, database, snapshot).await?;
    if family == "risk" && !parent["exposure_run_id"].is_null() {
        let dependency = parent["exposure_run_id"]
            .as_str()
            .context("exposure run identity missing")?;
        let active=db.query(&format!("SELECT toString(count()) FROM {database}.aml_exposure_runs_visible WHERE network_id={} AND run_id={} AND source_revision={}",literal(&snapshot.state.network_id),literal(dependency),snapshot.state.revision)).await?;
        ensure!(
            active == ["1"],
            "risk dependency is not a visible exposure run"
        );
    }
    let existing=db.query(&format!("SELECT toString(count()) FROM {database}.aml_analysis_publications_current WHERE network_id={} AND family={} AND publication_id={}",literal(&snapshot.state.network_id),literal(family),literal(id))).await?;
    ensure!(
        existing == ["0"],
        "analysis publication IDs cannot be reused"
    );
    for table in tables {
        let existing=db.query(&format!("SELECT toString(count()) FROM {database}.{table} FINAL WHERE network_id={} AND {key}={}",literal(&snapshot.state.network_id),literal(id))).await?;
        ensure!(
            existing == ["0"],
            "analysis attempt ID already has output rows; use a fresh ID"
        );
    }
    for (table, batch) in rows {
        publication::insert(db, database, table, batch).await?;
    }
    for (table, count) in &counts {
        let actual=db.query(&format!("SELECT toString(count()) FROM {database}.{table} FINAL WHERE network_id={} AND {key}={} AND revision={output_revision}",literal(&snapshot.state.network_id),literal(id))).await?;
        ensure!(
            actual == [count.to_string()],
            "analysis output count verification failed"
        );
    }
    unchanged(db, database, snapshot).await?;
    publication::insert(db,database,"aml_analysis_publications",&[json!({"network_id":snapshot.state.network_id,"family":family,"publication_id":id,
        "source_revision":snapshot.state.revision,"as_of_block":snapshot.state.last_block_number,"block_hash":snapshot.state.last_block_hash,
        "output_revision":output_revision,"status":"complete","row_counts_json":serde_json::to_string(&counts)?,"revision":output_revision,"created_at_unix_ms":parent["created_at_unix_ms"]})]).await?;
    Ok(())
}

/// A review/policy/detector owner can explicitly invalidate an existing seal.
/// Recompute under a new publication ID; invalidated IDs cannot be republished.
pub async fn invalidate_analysis(
    db: &impl Connection,
    database: &str,
    network: Network,
    family: &str,
    id: &str,
    revision: u64,
) -> Result<()> {
    crate::contract::identifier(database)?;
    family_tables(family)?;
    crate::migration::validate_identity(db, database, network).await?;
    let contract = crate::current_contract()?;
    let table = contract
        .tables
        .iter()
        .find(|t| t.name == "aml_analysis_publications")
        .context("publication table missing")?;
    let rows=db.query(&format!("SELECT toJSONString(tuple({})) FROM {database}.aml_analysis_publications_current WHERE network_id={} AND family={} AND publication_id={}",crate::core::projection(&table.columns),literal(network.id()),literal(family),literal(id))).await?;
    let mut rows = crate::core::decode_rows("aml_analysis_publications", network, rows)?;
    ensure!(
        rows.len() == 1
            && rows[0]["revision"]
                .as_u64()
                .is_some_and(|old| revision > old),
        "invalidation requires an existing publication and a newer revision"
    );
    rows[0]["status"] = json!("invalidated");
    rows[0]["revision"] = json!(revision);
    publication::insert(db, database, "aml_analysis_publications", &rows).await
}
