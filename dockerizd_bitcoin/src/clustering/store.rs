use super::*;
use crate::{
    NETWORK_ID,
    bitcoin::transactions::Reader,
    db::{self, Database},
    model,
    writer_lock::WriterLock,
};
use aml_warehouse::{Network, analytics, visibility};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
static NONCE: AtomicU64 = AtomicU64::new(0);
const MAX_BYTES: usize = 512 * 1024;
#[derive(Debug)]
pub struct Conflict;
impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Cluster review changed; reload the candidate version before reviewing"
        )
    }
}
impl std::error::Error for Conflict {}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub network_id: String,
    pub run_id: String,
    pub cluster_id: String,
    pub expected_version: String,
    pub decision: String,
    pub reviewed_by: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}
fn text(v: &str, max: usize) -> Result<()> {
    ensure!(
        !v.trim().is_empty() && v.len() <= max && !v.chars().any(char::is_control),
        "invalid empty, oversized or control-containing review field"
    );
    Ok(())
}
pub fn validate_id(id: &str) -> Result<()> {
    ensure!(
        id.strip_prefix("btc-cluster-")
            .or_else(|| id.strip_prefix("btc-clrun-"))
            .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())),
        "invalid clustering identifier"
    );
    Ok(())
}
impl Review {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.network_id == NETWORK_ID, "review network mismatch");
        for id in [&self.run_id, &self.cluster_id, &self.expected_version] {
            validate_id(id)?;
        }
        ensure!(
            self.run_id.starts_with("btc-clrun-")
                && self.expected_version == self.run_id
                && self.cluster_id.starts_with("btc-cluster-"),
            "review must name the exact candidate version"
        );
        ensure!(
            ["approved", "rejected", "revoked"].contains(&self.decision.as_str()),
            "decision must be approved, rejected or revoked"
        );
        text(&self.reviewed_by, 128)?;
        text(&self.reason, 2000)?;
        ensure!(
            !self.evidence_refs.is_empty() && self.evidence_refs.len() <= 20,
            "review requires 1..20 evidence references"
        );
        for r in &self.evidence_refs {
            text(r, 512)?;
        }
        Ok(())
    }
}
fn run_id() -> String {
    format!(
        "btc-clrun-{}",
        digest(&json!([
            std::process::id(),
            db::timestamp(),
            NONCE.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string()
        ]))
    )
}
fn row(table: &str, mut value: Value) -> Result<Value> {
    let contract = aml_warehouse::current_contract()?;
    let schema = contract
        .tables
        .iter()
        .find(|t| t.name == table)
        .context("missing cluster table")?;
    for c in &schema.columns {
        if value.get(&c.name).is_none() {
            let raw = c
                .default
                .as_ref()
                .with_context(|| format!("missing cluster field {}", c.name))?;
            value[&c.name] = if raw == "NULL" {
                Value::Null
            } else if raw.starts_with('\'') && raw.ends_with('\'') {
                json!(&raw[1..raw.len() - 1])
            } else {
                serde_json::from_str(raw)?
            };
        }
    }
    aml_warehouse::semantics::validate_row(&contract, table, &value)?;
    Ok(value)
}
fn rows(bundle: &Value, revision: u64) -> Result<BTreeMap<String, Vec<Value>>> {
    let id = &bundle["run_id"];
    let source = &bundle["source_snapshot"];
    let partial = bundle["cluster_coverage"]["status"] != "complete_within_bounded_scope";
    let mut out = BTreeMap::from(
        visibility::family_tables("cluster")?
            .iter()
            .map(|t| (t.to_string(), vec![]))
            .collect::<BTreeMap<_, _>>(),
    );
    out.get_mut("aml_cluster_runs").unwrap().push(row("aml_cluster_runs",json!({"network_id":NETWORK_ID,"run_id":id,"status":if partial{"truncated"}else{"complete"},"as_of_block":source["as_of_block"],
        "source_revision":source["source_revision"].as_str().context("missing cluster source revision")?.parse::<u64>()?,"detector_version":bundle["cluster_coverage"]["detector_version"],
        "cursor_json":"{}","started_at_unix_ms":revision,"completed_at_unix_ms":revision,"truncated":u8::from(partial),"revision":revision,"created_at_unix_ms":revision,
        "entity_memberships":0,"control_claims":0,"settings_json":bundle.to_string(),"raw_evidence_json":"{}"}))?);
    for c in bundle["clusters"]
        .as_array()
        .context("cluster list missing")?
    {
        let mut claim_ids = vec![];
        let review = &c["review"];
        for member in c["members"].as_array().context("cluster members missing")? {
            let claim = format!(
                "btc-clclaim-{}",
                digest(&json!([id, c["cluster_id"], member]))
            );
            claim_ids.push(claim.clone());
            out.get_mut("aml_cluster_claims").unwrap().push(row("aml_cluster_claims",json!({"network_id":NETWORK_ID,"claim_id":claim,"cluster_id":c["cluster_id"],"subject_id":member,
                "heuristic":c["heuristic"],"detector_version":c["detector_version"],"confidence":c["confidence"],"review_status":c["review_status"],"source_id":"bitcoin-canonical-cluster-leads",
                "evidence_refs":c["evidence_refs"],"evidence_json":c["evidence"].to_string(),"revision":revision,"created_at_unix_ms":revision,"same_entity":null,"run_id":id,
                "source_record_id":c["evidence"]["tx_hash"],"created_by":"bitcoin-cluster-detector","reviewed_by":review["reviewed_by"],"review_id":review["review_id"],
                "cluster_type":"investigation_lead","address_role":"candidate_member","raw_evidence_json":c.to_string()}))?);
            out.get_mut("aml_cluster_memberships").unwrap().push(row("aml_cluster_memberships",json!({"network_id":NETWORK_ID,"cluster_id":c["cluster_id"],"cluster_version":id,"subject_id":member,"claim_id":claim,
                "status":if c["review_status"]=="revoked"{"revoked"}else if c["review_status"]=="rejected"{"inactive"}else{"pending"},"revision":revision,"created_at_unix_ms":revision,
                "membership_id":format!("btc-clmember-{}",digest(&json!([id,c["cluster_id"],member]))),"run_id":id,"entity_id":null,"membership_type":"candidate_only",
                "confidence":c["confidence"],"review_id":review["review_id"],"cluster_type":"investigation_lead","address_role":"candidate_member","evidence_json":json!({"evidence_hash":c["evidence_hash"],"ownership_claimed":false}).to_string()}))?);
        }
        out.get_mut("aml_cluster_versions").unwrap().push(row("aml_cluster_versions",json!({"network_id":NETWORK_ID,"cluster_id":c["cluster_id"],"cluster_version":id,"cluster_type":"investigation_lead",
            "display_name":"Bitcoin investigation lead","change_type":if review["origin_run_id"]==*id{"review"}else{"candidate"},"change_reason":if review["origin_run_id"]==*id{review["reason"].clone()}else{json!("bounded canonical detection; no entity assignment")},
            "source_claim_ids":claim_ids,"active_member_count":0,"created_by":"bitcoin-cluster-detector","evidence_refs":c["evidence_refs"],"raw_evidence_json":c.to_string(),"revision":revision,"created_at_unix_ms":revision,"run_id":id}))?);
    }
    Ok(out)
}
async fn query(db: &Database, sql: &str, params: &[(&str, String)]) -> Result<Vec<Value>> {
    db.rows(
        &format!("{sql} SETTINGS max_threads=2,max_memory_usage=536870912,max_execution_time=15"),
        params,
    )
    .await
}
async fn latest_review(db: &Database, id: &str) -> Result<Value> {
    let list=query(db,"SELECT run_id,raw_evidence_json AS candidate_json FROM aml_cluster_versions_visible WHERE network_id={network:String} AND cluster_id={id:String} AND change_type='review' ORDER BY revision DESC LIMIT 1",&[("network",NETWORK_ID.into()),("id",id.into())]).await?;
    Ok(if let Some(v) = list.first() {
        // Avoid expanding the publication views twice in one join. The first
        // query grants visibility; the immutable parent is verified separately.
        let parent=query(db,"SELECT settings_json FROM aml_cluster_runs_current WHERE network_id={network:String} AND run_id={run:String} LIMIT 2",&[("network",NETWORK_ID.into()),("run",v["run_id"].as_str().context("review run missing")?.into())]).await?;
        ensure!(parent.len() == 1, "review parent missing or ambiguous");
        let bundle: Value = serde_json::from_str(
            parent[0]["settings_json"]
                .as_str()
                .context("review settings missing")?,
        )?;
        verify_frame(&bundle)?;
        let candidate: Value = serde_json::from_str(
            v["candidate_json"]
                .as_str()
                .context("review candidate missing")?,
        )?;
        ensure!(
            bundle["run_id"] == v["run_id"]
                && candidate["cluster_id"] == id
                && candidate["review"]["origin_run_id"] == v["run_id"]
                && candidate["review"]["review_id"].is_string()
                && bundle["clusters"]
                    .as_array()
                    .is_some_and(|cs| cs.contains(&candidate)),
            "review does not match its sealed candidate version"
        );
        if candidate["detector_version"] != VERSION
            || candidate["definition_hash"] != config()["definition_hash"]
        {
            Value::Null
        } else {
            candidate["review"].clone()
        }
    } else {
        Value::Null
    })
}
async fn reviews_unchanged(db: &Database, bundle: &Value) -> Result<bool> {
    for c in bundle["clusters"].as_array().context("missing leads")? {
        let actual = latest_review(
            db,
            c["cluster_id"]
                .as_str()
                .context("cluster identity missing")?,
        )
        .await?;
        if actual["review_id"] != c["review"]["review_id"] {
            return Ok(false);
        }
    }
    Ok(true)
}
fn frame(bundle: &mut Value) {
    bundle.as_object_mut().unwrap().remove("cluster_snapshot");
    let mut f = json!({"contract":"bitcoin_clustering_v1","network_id":NETWORK_ID,"address":bundle["address"],"run_id":bundle["run_id"],
        "source_snapshot_id":bundle["source_snapshot"]["snapshot_id"],"detector_version":VERSION,"definition_hash":config()["definition_hash"],
        "clusters_hash":digest(&bundle["clusters"]),"coverage_hash":digest(&bundle["cluster_coverage"]),"body_hash":digest(bundle),"audit_only":bundle["cluster_coverage"]["persisted"]!=true,
        "source_current":true,"current":bundle["cluster_coverage"]["persisted"]==true});
    let mut unsigned = f.clone();
    unsigned.as_object_mut().unwrap().remove("current");
    unsigned.as_object_mut().unwrap().remove("source_current");
    f["snapshot_id"] = json!(digest(&unsigned));
    bundle["cluster_snapshot"] = f;
}
fn verify_frame(bundle: &Value) -> Result<()> {
    let mut body = bundle.clone();
    let f = body
        .as_object_mut()
        .context("invalid cluster bundle")?
        .remove("cluster_snapshot")
        .context("cluster frame missing")?;
    let mut unsigned = f.clone();
    let object = unsigned.as_object_mut().context("invalid cluster frame")?;
    object.remove("snapshot_id");
    object.remove("current");
    object.remove("source_current");
    ensure!(
        f["contract"] == "bitcoin_clustering_v1"
            && f["network_id"] == NETWORK_ID
            && f["address"] == bundle["address"]
            && f["run_id"] == bundle["run_id"]
            && f["source_snapshot_id"] == bundle["source_snapshot"]["snapshot_id"]
            && f["snapshot_id"] == digest(&unsigned)
            && f["body_hash"] == digest(&body)
            && f["clusters_hash"] == digest(&bundle["clusters"])
            && f["coverage_hash"] == digest(&bundle["cluster_coverage"]),
        "cluster evidence binding mismatch"
    );
    Ok(())
}
async fn publish(
    db: &Database,
    source: &visibility::Snapshot,
    mut bundle: Value,
    revision: u64,
) -> Result<Value> {
    frame(&mut bundle);
    let data = rows(&bundle, revision)?;
    let id = bundle["run_id"].as_str().unwrap();
    if bundle["cluster_coverage"]["persisted"] == true {
        visibility::publish_analysis(
            &db.warehouse_connection(),
            db.database(),
            source,
            "cluster",
            id,
            revision,
            &data,
        )
        .await?;
    } else {
        for (table, batch) in &data {
            analytics::insert(&db.warehouse_connection(), db.database(), table, batch).await?;
        }
    }
    visibility::unchanged(&db.warehouse_connection(), db.database(), source).await?;
    if !reviews_unchanged(db, &bundle).await? {
        return Err(Conflict.into());
    }
    visibility::unchanged(&db.warehouse_connection(), db.database(), source).await?;
    // Review writes are independent of the ingestion fence. Inspection checks
    // them again so a concurrent review cannot silently refresh an old payload.
    Ok(bundle)
}
pub async fn compute(db: &Database, address: &str, activity: &Value) -> Result<Value> {
    let address = model::address(address)?;
    let source =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    ensure!(
        activity["source_snapshot"] == source.value(),
        "cluster activity source changed; retry"
    );
    if source.state().initialized == 0 {
        return Ok(
            json!({"network_id":NETWORK_ID,"address":address,"clusters":[],"cluster_snapshot":null,"cluster_coverage":{"status":"waiting_for_blocks","persisted":false,"search_complete":false}}),
        );
    }
    let roots = activity["recent_transactions"]
        .as_array()
        .context("cluster roots missing")?;
    let mut reader = Reader::new(db, MAX_ROOTS, 4096, 256);
    let mut leads = BTreeMap::new();
    let mut suppressed = vec![];
    let mut bytes = 0;
    let mut byte_limit = false;
    for root in roots.iter().take(MAX_ROOTS) {
        let Some(tx) = reader
            .load(
                root["block_hash"]
                    .as_str()
                    .context("cluster block missing")?,
                root["tx_hash"].as_str().context("cluster tx missing")?,
            )
            .await?
        else {
            continue;
        };
        let (candidates, reason) = detect::detect(&tx);
        if let Some(reason) = reason {
            suppressed
                .push(json!({"tx_hash":tx.h.tx_hash,"block_hash":tx.h.block_hash,"reason":reason}));
        }
        for mut c in candidates {
            if !c["members"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m == &address)
            {
                continue;
            }
            let review = latest_review(db, c["cluster_id"].as_str().unwrap()).await?;
            if review.is_object()
                && review["evidence_hash"] == c["evidence_hash"]
                && review["source_snapshot_id"] == source.id()
                && review["definition_hash"] == config()["definition_hash"]
            {
                c["review_status"] = review["decision"].clone();
                c["review"] = review;
            }
            bytes += serde_json::to_vec(&c)?.len();
            if bytes > MAX_BYTES {
                byte_limit = true;
                break;
            }
            leads.insert(c["cluster_id"].as_str().unwrap().to_owned(), c);
        }
        if byte_limit {
            break;
        }
    }
    let partial = reader.truncated
        || byte_limit
        || roots.len() > MAX_ROOTS
        || activity["activity_coverage"]["transactions_truncated"] == true;
    let id = run_id();
    let revision = db::timestamp().max(1);
    let mut clusters = leads.into_values().collect::<Vec<_>>();
    for c in &mut clusters {
        c["cluster_version"] = json!(id);
    }
    let bundle = json!({"network_id":NETWORK_ID,"address":address,"run_id":id,"source_snapshot":source.value(),"clusters":clusters,
        "cluster_coverage":{"status":if partial{"partial"}else{"complete_within_bounded_scope"},"persisted":!partial,"storage":if partial{"unsealed_audit_only"}else{"sealed_shared_tables"},"search_complete":false,"detector_version":VERSION,
        "definition_hash":config()["definition_hash"],"roots_available":roots.len(),"roots_limit":MAX_ROOTS,"transactions_examined":reader.cache.len(),"root_scope":activity["query_scope"]["analytics"],
        "roots_truncated":roots.len()>MAX_ROOTS || activity["activity_coverage"]["transactions_truncated"]==true,"context_limited":reader.truncated,"evidence_limit_reached":byte_limit,
        "suppressed_transactions":suppressed,"ownership_claimed":false,"score_eligible":false,"transitive_union":false,"limitations":config()["definition"]["limitations"]}});
    publish(db, &source, bundle, revision).await
}
pub async fn inspect(db: &Database, address: &str, id: &str) -> Result<Value> {
    let address = model::address(address)?;
    validate_id(id)?;
    let params = [("network", NETWORK_ID.into()), ("run", id.into())];
    let run=query(db,"SELECT * FROM aml_cluster_runs_current WHERE network_id={network:String} AND run_id={run:String} LIMIT 2",&params).await?;
    ensure!(run.len() == 1, "cluster run missing or ambiguous");
    let original: Value = serde_json::from_str(
        run[0]["settings_json"]
            .as_str()
            .context("cluster settings missing")?,
    )?;
    ensure!(
        original["address"] == address
            && original["run_id"] == id
            && original["network_id"] == NETWORK_ID,
        "cluster run scope mismatch"
    );
    verify_frame(&original)?;
    let f = &original["cluster_snapshot"];
    let revision = run[0]["revision"]
        .as_u64()
        .context("run revision missing")?;
    let expected = rows(&original, revision)?;
    for (table, wanted) in expected {
        let mut actual=query(db,&format!("SELECT * FROM {table}_current WHERE network_id={{network:String}} AND run_id={{run:String}} LIMIT 501"),&params).await?;
        let mut wanted = wanted;
        actual.sort_by_key(Value::to_string);
        wanted.sort_by_key(Value::to_string);
        ensure!(
            actual == wanted,
            "cluster output rows or counts do not match their framed publication"
        );
    }
    let source =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    let seals=query(db,"SELECT count() AS n FROM aml_cluster_runs_visible WHERE network_id={network:String} AND run_id={run:String}",&params).await?;
    let source_current = source.value() == original["source_snapshot"]
        && f["detector_version"] == VERSION
        && f["definition_hash"] == config()["definition_hash"]
        && reviews_unchanged(db, &original).await?;
    visibility::unchanged(&db.warehouse_connection(), db.database(), &source).await?;
    let mut result = original;
    result["cluster_snapshot"]["source_current"] = json!(source_current);
    result["cluster_snapshot"]["current"] = json!(source_current && seals[0]["n"] == 1);
    Ok(result)
}
pub async fn snapshot(db: &Database, address: &str, id: &str) -> Result<Value> {
    let value = inspect(db, address, id).await?;
    Ok(
        json!({"network_id":NETWORK_ID,"cluster_snapshot":value["cluster_snapshot"],"cluster_config":config()}),
    )
}
pub async fn review(db: &Database, address: &str, decision: Review) -> Result<Value> {
    decision.validate()?;
    // A separate review lock does not stop the long-running ingestion owner.
    let _lock = WriterLock::for_database(&format!("{}_cluster_review", db.database()))?;
    let mut value = inspect(db, address, &decision.run_id).await?;
    if value["cluster_snapshot"]["current"] != true {
        return Err(Conflict.into());
    }
    let source_id = value["source_snapshot"]["snapshot_id"].clone();
    let c = value["clusters"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["cluster_id"] == decision.cluster_id)
        .context("candidate not present in this run")?;
    let id = run_id();
    let revision = db::timestamp()
        .max(c["review"]["revision"].as_u64().unwrap_or(0) + 1)
        .max(1);
    c["review"] = json!({"review_id":format!("btc-clreview-{}",digest(&json!([id,&decision]))),"origin_run_id":id,"previous_version":decision.run_id,"previous_review_id":c["review"]["review_id"],
        "decision":decision.decision,"reviewed_by":decision.reviewed_by,"reason":decision.reason,"evidence_refs":decision.evidence_refs,
        "revision":revision,"evidence_hash":c["evidence_hash"],"definition_hash":config()["definition_hash"],"source_snapshot_id":source_id,"accepts_lead_only":true});
    c["review_status"] = json!(decision.decision);
    value["run_id"] = json!(id);
    for c in value["clusters"].as_array_mut().unwrap() {
        c["cluster_version"] = json!(id);
    }
    let source =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    ensure!(
        source.value() == value["source_snapshot"],
        "cluster source changed during review"
    );
    publish(db, &source, value, revision).await
}
