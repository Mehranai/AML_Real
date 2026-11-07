//! Governed TRON operational records in the existing shared contract.
use super::{Client, now_ms, row};
use aml_warehouse::{Network, publication::literal, visibility};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs::File};

pub fn operational_lock(client: &Client) -> Result<File> {
    crate::services::tron::publication_store::writer_lock(&format!(
        "{}_operations",
        client.database()
    ))
}
pub fn operational_snapshot(versions: &[String]) -> Value {
    use sha2::{Digest, Sha256};
    let body =
        json!({"contract":"tron_operational_v1","network_id":"tron:mainnet","versions":versions});
    let id = format!("{:x}", Sha256::digest(body.to_string().as_bytes()));
    let mut value = body;
    value["snapshot_id"] = json!(id);
    value
}
pub fn refs(values: &Value) -> Vec<String> {
    values
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|v| {
            if aml_warehouse::values::evidence_ref(Network::Tron, v).is_ok() {
                v.into()
            } else if let Ok(hash) = aml_warehouse::values::normalize_hash(Network::Tron, v) {
                format!("tx:{hash}")
            } else {
                format!("source:{v}")
            }
        })
        .collect()
}
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub async fn invalidate(client: &Client) -> Result<()> {
    for family in ["cluster", "exposure", "risk"] {
        client.invalidate_active(family).await?;
    }
    Ok(())
}
pub async fn source(client: &Client, v: Value) -> Result<()> {
    let record = row(
        "aml_intelligence_sources",
        json!({"source_id":v["source_id"],"source_name":v["source_name"],"source_type":v["source_type"],"trust_tier":v["trust_tier"],"reference_url":v["reference_url"],"license":v["license"],"is_active":v["is_active"],"created_by":v["created_by"],"metadata_json":"{}","raw_evidence_json":v.to_string()}),
        client.revision("aml_intelligence_sources").await?,
        now_ms(),
    )?;
    // Invalidate before changing dependencies; an interrupted refresh fails closed.
    invalidate(client).await?;
    client
        .insert_rows("aml_intelligence_sources", &[record])
        .await
}
pub async fn claim(client: &Client, v: Value, cluster: bool) -> Result<()> {
    let id = if cluster {
        text(&v, "claim_id")
    } else {
        text(&v, "label_id")
    };
    let existing = client
        .rows(
            "aml_intelligence_claims",
            &format!("claim_id={}", literal(id)),
        )
        .await?;
    if let Some(old) = existing.first() {
        let mut old_body: Value = serde_json::from_str(text(old, "raw_evidence_json"))?;
        let mut new_body = v.clone();
        old_body
            .as_object_mut()
            .context("claim source object")?
            .remove("created_at_unix_ms");
        new_body
            .as_object_mut()
            .context("claim object")?
            .remove("created_at_unix_ms");
        ensure!(
            old_body == new_body,
            "claim ID already exists with different content; submit a new ID and supersedes reference"
        );
        return Ok(());
    }
    let evidence = if cluster {
        refs(&v["evidence_tx_hashes"])
    } else {
        refs(&v["evidence_refs"])
    };
    let risk = if cluster {
        0
    } else {
        v["risk_percent"].as_u64().context("risk_percent")?
    };
    let data = row(
        "aml_intelligence_claims",
        json!({"claim_id":id,"address":v["address"],"claim_kind":if cluster {"cluster"} else {"label"},"entity_id":if cluster {v["cluster_id"].clone()} else {v["entity_id"].clone()},"entity_name":if cluster {v["cluster_id"].clone()} else {v["entity_name"].clone()},"entity_type":if cluster {v["cluster_type"].clone()} else {v["entity_type"].clone()},"address_role":text(&v,"address_role").to_ascii_lowercase(),"confidence":v["confidence"],"risk_level":risk,"is_exposure_seed":u8::from(risk>0),"seed_category":if risk>0 {text(&v,"entity_type")} else {""},"source_id":v["source"],"source_reference":v["source_record_id"],"evidence_refs":evidence,"review_status":"pending","reviewed_by":"","review_note":"","valid_from_unix_ms":now_ms(),"valid_to_unix_ms":null,"source_record_id":v["source_record_id"],"supersedes_claim_id":if cluster {v["supersedes_claim_id"].clone()} else {v["supersedes_label_id"].clone()},"submitted_by":if cluster {v["created_by"].clone()} else {v["submitted_by"].clone()},"case_id":if cluster {Value::Null} else {v["case_id"].clone()},"evidence_json":if cluster {v["evidence_json"].clone()} else {json!("{}")},"raw_evidence_json":v.to_string()}),
        client.revision("aml_intelligence_claims").await?,
        now_ms(),
    )?;
    client.insert_rows("aml_intelligence_claims", &[data]).await
}
pub async fn review(client: &Client, v: Value) -> Result<()> {
    let id = text(&v, "subject_id");
    let mut claim = client
        .rows(
            "aml_intelligence_claims",
            &format!("claim_id={}", literal(id)),
        )
        .await?
        .pop()
        .context("review subject missing")?;
    let kind = if v["subject_type"] == "CLUSTER_CLAIM" {
        "cluster"
    } else {
        "label"
    };
    ensure!(claim["claim_kind"] == kind, "review subject type mismatch");
    ensure!(
        !text(&v, "reviewer").trim().is_empty() && !text(&v, "reason").trim().is_empty(),
        "review actor and reason are required"
    );
    let source = client
        .rows(
            "aml_intelligence_sources",
            &format!("source_id={}", literal(text(&claim, "source_id"))),
        )
        .await?;
    if v["decision"] == "APPROVED" {
        ensure!(
            source.first().is_some_and(|s| s["is_active"] == 1),
            "source disabled; approval refused"
        );
    }
    let decision = text(&v, "decision").to_ascii_lowercase();
    let supersedes = text(&claim, "supersedes_claim_id");
    let superseded = if decision == "approved" && !supersedes.is_empty() {
        ensure!(supersedes != id, "a claim cannot supersede itself");
        let old = client
            .rows(
                "aml_intelligence_claims",
                &format!("claim_id={}", literal(supersedes)),
            )
            .await?
            .pop()
            .context("superseded claim missing")?;
        ensure!(
            old["address"] == claim["address"] && old["claim_kind"] == claim["claim_kind"],
            "superseded claim must have the same subject and kind"
        );
        Some(old)
    } else {
        None
    };
    let superseded_revision = if superseded.is_some() {
        Some(client.revision("aml_intelligence_claims").await?)
    } else {
        None
    };
    let rev = client.revision("aml_intelligence_claims").await?;
    let audit = row(
        "aml_intelligence_reviews",
        json!({"review_id":v["review_id"],"subject_type":"claim","subject_id":id,"decision":decision,"reviewed_by":v["reviewer"],"reason":v["reason"],"evidence_refs":refs(&v["evidence_refs"]),"subject_revision":rev,"raw_evidence_json":v.to_string()}),
        client.revision("aml_intelligence_reviews").await?,
        now_ms(),
    )?;
    invalidate(client).await?;
    // Audit precedes activation. A crash can leave an audit-only attempt, never an unaudited approval.
    client
        .insert_rows("aml_intelligence_reviews", &[audit])
        .await?;
    if let Some(mut old) = superseded {
        let old_rev = superseded_revision.context("superseded revision missing")?;
        let reason = format!("Superseded by {id}: {}", text(&v, "reason"));
        let mut raw = v.clone();
        raw["subject_id"] = old["claim_id"].clone();
        raw["decision"] = json!("REVOKED");
        raw["reason"] = json!(reason);
        let audit = row(
            "aml_intelligence_reviews",
            json!({"review_id":format!("{}:superseded",text(&v,"review_id")),"subject_type":"claim","subject_id":old["claim_id"],"decision":"revoked","reviewed_by":v["reviewer"],"reason":reason,"evidence_refs":refs(&v["evidence_refs"]),"subject_revision":old_rev,"raw_evidence_json":raw.to_string()}),
            client.revision("aml_intelligence_reviews").await?,
            now_ms(),
        )?;
        client
            .insert_rows("aml_intelligence_reviews", &[audit])
            .await?;
        old["review_status"] = json!("revoked");
        old["reviewed_by"] = v["reviewer"].clone();
        old["review_note"] = json!(reason);
        old["reviewed_at_unix_ms"] = json!(now_ms());
        old["revision"] = json!(old_rev);
        client
            .insert_rows("aml_intelligence_claims", &[old])
            .await?;
    }
    claim["review_status"] = json!(decision);
    claim["reviewed_by"] = v["reviewer"].clone();
    claim["review_note"] = v["reason"].clone();
    claim["reviewed_at_unix_ms"] = json!(now_ms());
    claim["revision"] = json!(rev);
    client
        .insert_rows("aml_intelligence_claims", &[claim.clone()])
        .await?;
    if decision == "approved" && kind == "label" {
        project_label(client, &claim).await?;
    }
    publish_clusters(client).await
}
async fn project_label(client: &Client, c: &Value) -> Result<()> {
    let now = now_ms();
    let entity = row(
        "aml_entities",
        json!({"entity_id":c["entity_id"],"entity_name":c["entity_name"],"entity_type":c["entity_type"],"source_id":c["source_id"],"source_reference":c["source_reference"],"is_active":1,"review_status":"approved","reviewed_by":c["reviewed_by"],"reviewed_at_unix_ms":now,"review_note":c["review_note"],"evidence_refs":c["evidence_refs"],"raw_evidence_json":c.to_string()}),
        client.revision("aml_entities").await?,
        now,
    )?;
    client.insert_rows("aml_entities", &[entity]).await?;
    let address = row(
        "aml_entity_addresses",
        json!({"entity_id":c["entity_id"],"address":c["address"],"address_role":c["address_role"],"claim_id":c["claim_id"],"source_id":c["source_id"],"confidence":c["confidence"],"is_active":1,"first_seen_block":null,"last_seen_block":null,"review_status":"approved","reviewed_by":c["reviewed_by"],"reviewed_at_unix_ms":now,"review_note":c["review_note"],"evidence_refs":c["evidence_refs"],"raw_evidence_json":c.to_string()}),
        client.revision("aml_entity_addresses").await?,
        now,
    )?;
    client
        .insert_rows("aml_entity_addresses", &[address])
        .await?;
    if c["is_exposure_seed"] == 1 {
        let seed = row(
            "aml_exposure_seeds",
            json!({"seed_id":format!("tron:seed:{}",text(c,"claim_id")),"address":c["address"],"claim_id":c["claim_id"],"category":c["seed_category"],"is_active":1,"entity_id":c["entity_id"],"entity_name":c["entity_name"],"entity_type":c["entity_type"],"risk_level":c["risk_level"],"source_id":c["source_id"],"claim_revision":c["revision"],"evidence_refs":c["evidence_refs"],"raw_evidence_json":c.to_string()}),
            client.revision("aml_exposure_seeds").await?,
            now,
        )?;
        client.insert_rows("aml_exposure_seeds", &[seed]).await?;
    }
    Ok(())
}
/// Reviewed membership only. Candidate heuristics remain separately reviewable claims.
pub async fn publish_clusters(client: &Client) -> Result<()> {
    let snapshot = visibility::capture(client, client.database(), Network::Tron).await?;
    if snapshot.state().initialized == 0 {
        return Ok(());
    }
    client.invalidate_active("cluster").await?;
    let rev = client.revision("aml_cluster_runs").await?;
    let run = format!("tron_cluster_{rev}");
    let now = now_ms();
    let mut rows = visibility::family_tables("cluster")?
        .iter()
        .map(|t| (t.to_string(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let active = client
        .rows_from("aml_intelligence_claims", "active", "claim_kind='cluster'")
        .await?;
    let mut grouped = BTreeMap::<String, Vec<&Value>>::new();
    for c in &active {
        let original: Value = serde_json::from_str(text(c, "raw_evidence_json"))?;
        // Source transaction evidence must still belong to the indexed canonical history.
        let mut valid = true;
        for hash in original["evidence_tx_hashes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let hash = aml_warehouse::values::normalize_hash(Network::Tron, hash)?;
            if client.query("SELECT count() FROM aml_transactions_canonical WHERE network_id='tron:mainnet' AND tx_hash=?").bind(hash).fetch_one::<u64>().await?!=1 {valid=false;break;}
        }
        if valid {
            grouped
                .entry(text(c, "entity_id").into())
                .or_default()
                .push(c);
        }
    }
    let mut count = 0;
    for (cluster, members) in grouped {
        let version = format!("{run}:{cluster}");
        for c in &members {
            let original: Value = serde_json::from_str(text(c, "raw_evidence_json"))?;
            rows.get_mut("aml_cluster_claims").unwrap().push(row("aml_cluster_claims",json!({"claim_id":format!("{run}:{}",text(c,"claim_id")),"cluster_id":cluster,"subject_id":c["address"],"heuristic":original["claim_method"],"detector_version":"tron_structural_heuristics_v1","confidence":c["confidence"],"review_status":"approved","source_id":c["source_id"],"evidence_refs":c["evidence_refs"],"evidence_json":c["evidence_json"],"same_entity":null,"run_id":run,"source_record_id":c["claim_id"],"created_by":c["submitted_by"],"reviewed_by":c["reviewed_by"],"cluster_type":c["entity_type"],"address_role":c["address_role"],"raw_evidence_json":c.to_string()}),rev,now)?);
            rows.get_mut("aml_cluster_memberships").unwrap().push(row("aml_cluster_memberships",json!({"cluster_id":cluster,"cluster_version":version,"subject_id":c["address"],"claim_id":format!("{run}:{}",text(c,"claim_id")),"status":"active","membership_id":format!("{run}:{}",text(c,"claim_id")),"run_id":run,"membership_type":"reviewed_structural_claim","confidence":c["confidence"],"cluster_type":c["entity_type"],"address_role":c["address_role"],"evidence_json":json!({"intelligence_claim_id":c["claim_id"],"claim_revision":c["revision"]}).to_string()}),rev,now)?);
            count += 1;
        }
        rows.get_mut("aml_cluster_versions").unwrap().push(row("aml_cluster_versions",json!({"cluster_id":cluster,"cluster_version":version,"cluster_type":members[0]["entity_type"],"display_name":cluster,"change_type":"refresh","change_reason":"Active reviewed structural claims; no automatic ownership assertion","source_claim_ids":members.iter().map(|c|c["claim_id"].clone()).collect::<Vec<_>>(),"active_member_count":members.len(),"created_by":"tron_analytics_worker","raw_evidence_json":"{}","evidence_refs":members.iter().map(|c|format!("claim:{}",text(c,"claim_id"))).collect::<Vec<_>>(),"run_id":run}),rev,now)?);
    }
    rows.get_mut("aml_cluster_runs").unwrap().push(row("aml_cluster_runs",json!({"run_id":run,"status":"complete","as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"detector_version":"tron_structural_heuristics_v1","cursor_json":"{}","started_at_unix_ms":now,"completed_at_unix_ms":now_ms(),"truncated":0,"entity_memberships":count,"control_claims":0}),rev,now)?);
    visibility::publish_analysis(
        client,
        client.database(),
        &snapshot,
        "cluster",
        &run,
        rev,
        &rows,
    )
    .await
}
