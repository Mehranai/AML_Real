//! Reviewed address attribution on the existing shared v5 tables. Imports are
//! pending; a separate review publishes a claim. A review written before a crash
//! suppresses an older approval until the matching claim revision is committed.
pub mod cli;
#[cfg(test)]
mod tests;
use crate::{
    NETWORK_ID,
    db::{self, Database},
    model,
    writer_lock::WriterLock,
};
use aml_warehouse::{Network, analytics, values};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

const SOURCES: &str = "aml_intelligence_sources";
const CLAIMS: &str = "aml_intelligence_claims";
const REVIEWS: &str = "aml_intelligence_reviews";
const LIMIT: usize = 100;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub network_id: String,
    pub source_id: String,
    pub source_name: String,
    pub source_type: String,
    pub reference_url: String,
    pub trust_tier: String,
    pub license: String,
    pub is_active: bool,
    pub created_by: String,
    pub expected_revision: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub network_id: String,
    pub claim_id: String,
    pub address: String,
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub address_role: String,
    pub confidence: f64,
    pub risk_level: u8,
    pub is_exposure_seed: bool,
    pub seed_category: String,
    pub source_id: String,
    pub source_reference: String,
    pub evidence_refs: Vec<String>,
    pub submitted_by: String,
    pub valid_from_unix_ms: Option<u64>,
    pub valid_to_unix_ms: Option<u64>,
    pub case_id: Option<String>,
    pub expected_revision: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub network_id: String,
    pub claim_id: String,
    pub expected_revision: String,
    pub decision: String,
    pub reviewed_by: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}

fn text(value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control),
        "invalid empty, oversized or control-containing field"
    );
    Ok(())
}
fn id(value: &str) -> Result<()> {
    text(value, 128)?;
    ensure!(
        value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b)),
        "invalid intelligence identifier"
    );
    Ok(())
}
fn network(value: &str) -> Result<()> {
    ensure!(
        value == NETWORK_ID,
        "intelligence belongs to another network"
    );
    Ok(())
}
fn refs(value: &[String]) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 50,
        "1..50 evidence references required"
    );
    for item in value {
        text(item, 2000)?;
        values::evidence_ref(Network::Bitcoin, item)?;
    }
    Ok(())
}
fn revision(value: &Value) -> Result<u64> {
    value
        .as_str()
        .context("revision must be an exact decimal string")?
        .parse()
        .context("invalid revision")
}
fn expected(previous: Option<&Value>, supplied: Option<&str>) -> Result<()> {
    match previous {
        Some(row) => ensure!(
            supplied == row["revision"].as_str(),
            "current revision changed; inspect and supply expected_revision"
        ),
        None => ensure!(
            supplied.is_none(),
            "expected_revision supplied for an absent record"
        ),
    }
    Ok(())
}
impl Source {
    pub fn validate(&self) -> Result<()> {
        network(&self.network_id)?;
        id(&self.source_id)?;
        for v in [
            &self.source_name,
            &self.source_type,
            &self.reference_url,
            &self.trust_tier,
            &self.license,
            &self.created_by,
        ] {
            text(v, 2000)?;
        }
        let url = reqwest::Url::parse(&self.reference_url)?;
        ensure!(
            matches!(url.scheme(), "https" | "http")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none(),
            "source reference must be credential-free HTTP(S)"
        );
        Ok(())
    }
}
impl Claim {
    pub fn validate(&mut self, now: u64) -> Result<()> {
        network(&self.network_id)?;
        id(&self.claim_id)?;
        id(&self.source_id)?;
        id(&self.entity_id)?;
        self.address = model::address(&self.address)?;
        for v in [
            &self.entity_name,
            &self.entity_type,
            &self.address_role,
            &self.source_reference,
            &self.submitted_by,
        ] {
            text(v, 2000)?;
        }
        refs(&self.evidence_refs)?;
        ensure!(
            self.confidence.is_finite()
                && (0.0..=1.0).contains(&self.confidence)
                && self.risk_level <= 100,
            "invalid confidence or risk designation"
        );
        ensure!(
            !self.is_exposure_seed
                || (self.risk_level > 0 && !self.seed_category.trim().is_empty()),
            "risk seed requires explicit risk level and category"
        );
        if !self.seed_category.is_empty() {
            text(&self.seed_category, 128)?;
        }
        if let Some(case) = &self.case_id {
            text(case, 128)?;
        }
        let from = *self.valid_from_unix_ms.get_or_insert(now);
        ensure!(
            self.valid_to_unix_ms.is_none_or(|until| until > from),
            "invalid claim validity interval"
        );
        Ok(())
    }
}
impl Review {
    pub fn validate(&self) -> Result<()> {
        network(&self.network_id)?;
        id(&self.claim_id)?;
        ensure!(
            matches!(self.decision.as_str(), "approved" | "rejected" | "revoked"),
            "invalid review decision"
        );
        text(&self.reviewed_by, 256)?;
        text(&self.reason, 4000)?;
        refs(&self.evidence_refs)?;
        ensure!(
            self.expected_revision.parse::<u64>()? > 0,
            "invalid expected_revision"
        );
        Ok(())
    }
}

fn lock(db: &Database) -> Result<WriterLock> {
    // Shares the VM lock volume while allowing ingestion to continue independently.
    WriterLock::for_database(&format!(
        "intel_{:x}",
        Sha256::digest(db.database().as_bytes())
    ))
}
async fn next(db: &Database) -> Result<u64> {
    let rows: Vec<Value> = db.rows("SELECT toString(max(revision)) AS revision FROM (SELECT revision FROM aml_intelligence_sources WHERE network_id={network:String} UNION ALL SELECT revision FROM aml_intelligence_claims WHERE network_id={network:String} UNION ALL SELECT revision FROM aml_intelligence_reviews WHERE network_id={network:String})", &[("network", NETWORK_ID.into())]).await?;
    Ok(revision(&rows[0]["revision"])?
        .checked_add(1)
        .context("intelligence revision exhausted")?
        .max(db::timestamp()))
}
async fn write(db: &Database, table: &str, rows: &[Value]) -> Result<()> {
    analytics::insert(&db.warehouse_connection(), db.database(), table, rows).await
}
async fn current(db: &Database, table: &str, key: &str, value: &str) -> Result<Option<Value>> {
    ensure!(
        [SOURCES, CLAIMS].contains(&table) && ["source_id", "claim_id"].contains(&key),
        "invalid lookup"
    );
    let rows: Vec<Value> = db.rows(&format!("SELECT * REPLACE(toString(revision) AS revision) FROM {table}_current WHERE network_id={{network:String}} AND {key}={{id:String}} LIMIT 2"), &[("network", NETWORK_ID.into()), ("id", value.into())]).await?;
    ensure!(rows.len() <= 1, "ambiguous current intelligence record");
    Ok(rows.into_iter().next())
}
pub async fn register(db: &Database, source: Source, dry_run: bool) -> Result<Value> {
    source.validate()?;
    let _owner = lock(db)?;
    let old = current(db, SOURCES, "source_id", &source.source_id).await?;
    expected(old.as_ref(), source.expected_revision.as_deref())?;
    let r = next(db).await?;
    let now = db::timestamp();
    let mut row = serde_json::to_value(&source)?;
    row.as_object_mut().unwrap().remove("expected_revision");
    row["is_active"] = json!(u8::from(source.is_active));
    row["revision"] = json!(r);
    row["created_at_unix_ms"] = old
        .as_ref()
        .map(|o| o["created_at_unix_ms"].clone())
        .unwrap_or(json!(now));
    row["updated_at_unix_ms"] = json!(now);
    row["metadata_json"] = json!(json!({"updated_by":source.created_by}).to_string());
    if let Some(previous) = &old {
        row["created_by"] = previous["created_by"].clone();
    }
    row["raw_evidence_json"] = json!(serde_json::to_string(&source)?);
    aml_warehouse::semantics::validate_row(&aml_warehouse::current_contract()?, SOURCES, &row)?;
    if !dry_run {
        write(db, SOURCES, &[row]).await?;
    }
    Ok(json!({"source_id":source.source_id,"revision":r.to_string(),"written":!dry_run}))
}
pub async fn import(db: &Database, mut claims: Vec<Claim>, dry_run: bool) -> Result<Value> {
    ensure!(
        !claims.is_empty() && claims.len() <= LIMIT,
        "import requires 1..100 claims"
    );
    let now = db::timestamp();
    let mut ids = HashSet::new();
    for claim in &mut claims {
        claim.validate(now)?;
        ensure!(
            ids.insert(claim.claim_id.clone()),
            "duplicate claim id in import"
        );
    }
    let _owner = lock(db)?;
    let mut r = next(db).await?;
    let mut rows = Vec::new();
    let mut result = Vec::new();
    // Complete all validation/preconditions before the single pending insert.
    for claim in claims {
        let source = current(db, SOURCES, "source_id", &claim.source_id)
            .await?
            .context("register the intelligence source before import")?;
        ensure!(source["is_active"] == 1, "source is inactive");
        let old = current(db, CLAIMS, "claim_id", &claim.claim_id).await?;
        expected(old.as_ref(), claim.expected_revision.as_deref())?;
        let mut row = serde_json::to_value(&claim)?;
        row.as_object_mut().unwrap().remove("expected_revision");
        row["claim_kind"] = json!("label");
        row["review_status"] = json!("pending");
        row["reviewed_by"] = json!("");
        row["review_note"] = json!("");
        row["is_exposure_seed"] = json!(u8::from(claim.is_exposure_seed));
        row["revision"] = json!(r);
        row["created_at_unix_ms"] = old
            .as_ref()
            .map(|o| o["created_at_unix_ms"].clone())
            .unwrap_or(json!(now));
        row["raw_evidence_json"] = json!(serde_json::to_string(&claim)?);
        row["source_record_id"] = Value::Null;
        row["supersedes_claim_id"] = Value::Null;
        row["reviewed_at_unix_ms"] = Value::Null;
        row["evidence_json"] = json!("{}");
        aml_warehouse::semantics::validate_row(&aml_warehouse::current_contract()?, CLAIMS, &row)?;
        result.push(
            json!({"claim_id":claim.claim_id,"revision":r.to_string(),"review_status":"pending"}),
        );
        rows.push(row);
        r = r
            .checked_add(1)
            .context("intelligence revision exhausted")?;
    }
    if !dry_run {
        write(db, CLAIMS, &rows).await?;
    }
    Ok(json!({"written":!dry_run,"claims":result}))
}
pub async fn review(db: &Database, decision: Review, dry_run: bool) -> Result<Value> {
    decision.validate()?;
    let _owner = lock(db)?;
    let mut claim = current(db, CLAIMS, "claim_id", &decision.claim_id)
        .await?
        .context("claim not found")?;
    expected(Some(&claim), Some(&decision.expected_revision))?;
    let before = claim.clone();
    let source = current(
        db,
        SOURCES,
        "source_id",
        claim["source_id"].as_str().context("missing source")?,
    )
    .await?;
    if decision.decision == "approved" {
        ensure!(
            source.as_ref().context("source not found")?["is_active"] == 1,
            "cannot approve an inactive source"
        );
        ensure!(
            claim["valid_to_unix_ms"].is_null()
                || claim["valid_to_unix_ms"]
                    .as_u64()
                    .is_some_and(|end| end > db::timestamp()),
            "claim is expired"
        );
    }
    let r = next(db).await?;
    let now = db::timestamp();
    let review_id = format!("review-{r}");
    let mut row = json!({"network_id":NETWORK_ID,"review_id":review_id,"subject_type":"intelligence_claim","subject_id":decision.claim_id,
        "decision":decision.decision,"reviewed_by":decision.reviewed_by,"reason":decision.reason,"evidence_refs":decision.evidence_refs,
        "subject_revision":r,"revision":r,"created_at_unix_ms":now,"case_id":claim["case_id"],"supersedes_review_id":null,"evidence_json":"{}","raw_evidence_json":serde_json::to_string(&decision)?});
    claim["review_status"] = json!(decision.decision);
    claim["reviewed_by"] = json!(decision.reviewed_by);
    claim["review_note"] = json!(decision.reason);
    claim["reviewed_at_unix_ms"] = json!(now);
    claim["revision"] = json!(r);
    row["evidence_json"] = json!(
        json!({"claim_before":before,"claim_after":claim,"source_at_review":source}).to_string()
    );
    let contract = aml_warehouse::current_contract()?;
    aml_warehouse::semantics::validate_row(&contract, REVIEWS, &row)?;
    aml_warehouse::semantics::validate_row(&contract, CLAIMS, &claim)?;
    if !dry_run {
        write(db, REVIEWS, &[row]).await?;
        write(db, CLAIMS, &[claim]).await?;
    }
    Ok(
        json!({"claim_id":decision.claim_id,"review_id":review_id,"revision":r.to_string(),"review_status":decision.decision,"written":!dry_run}),
    )
}

#[derive(Clone, Debug)]
pub struct Attribution {
    pub snapshot: Value,
    pub entities: Vec<Value>,
    pub claims: Vec<Value>,
    pub sources: Vec<Value>,
}
fn active(claim: &Value, source: Option<&&Value>, review: Option<&&Value>, now: u64) -> bool {
    let (Some(source), Some(review)) = (source, review) else {
        return false;
    };
    claim["review_status"] == "approved"
        && claim["claim_kind"] == "label"
        && source["is_active"] == 1
        && review["decision"] == "approved"
        && review["subject_revision"] == claim["revision"]
        && review["reviewed_by"] == claim["reviewed_by"]
        && !claim["reviewed_by"]
            .as_str()
            .unwrap_or("")
            .trim()
            .is_empty()
        && claim["evidence_refs"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
        && claim["valid_from_unix_ms"]
            .as_u64()
            .is_some_and(|from| from <= now)
        && (claim["valid_to_unix_ms"].is_null()
            || claim["valid_to_unix_ms"]
                .as_u64()
                .is_some_and(|to| to > now))
}
fn public(mut row: Value) -> Value {
    for key in ["raw_evidence_json", "evidence_json", "metadata_json"] {
        row.as_object_mut().unwrap().remove(key);
    }
    row
}
pub async fn read(db: &Database, address: &str) -> Result<Attribution> {
    let address = model::address(address)?;
    let params = [("network", NETWORK_ID.into()), ("address", address.clone())];
    let subquery = "SELECT claim_id FROM aml_intelligence_claims_current WHERE network_id={network:String} AND address={address:String}";
    let claims: Vec<Value> = db.rows("SELECT * REPLACE(toString(revision) AS revision) FROM aml_intelligence_claims_current WHERE network_id={network:String} AND address={address:String} ORDER BY claim_id LIMIT 101", &params).await?;
    ensure!(
        claims.len() <= LIMIT,
        "address has more than 100 current claims; attribution cannot be truncated silently"
    );
    let sources: Vec<Value> = db.rows("SELECT * REPLACE(toString(revision) AS revision) FROM aml_intelligence_sources_current WHERE network_id={network:String} AND source_id IN (SELECT source_id FROM aml_intelligence_claims_current WHERE network_id={network:String} AND address={address:String}) ORDER BY source_id LIMIT 101", &params).await?;
    let reviews: Vec<Value> = db.rows(&format!("SELECT * REPLACE(toString(revision) AS revision,toString(subject_revision) AS subject_revision) FROM aml_intelligence_reviews_current WHERE network_id={{network:String}} AND subject_type='intelligence_claim' AND subject_id IN ({subquery}) ORDER BY toUInt64(revision) DESC LIMIT 1 BY subject_id LIMIT 101"), &params).await?;
    ensure!(
        sources.len() <= LIMIT && reviews.len() <= LIMIT,
        "attribution dependency limit reached"
    );
    assemble(address, claims, sources, reviews)
}
/// Batch freshness checks include empty addresses, so a newly approved seed or
/// service boundary invalidates a run that previously observed no such label.
pub async fn read_many(
    db: &Database,
    addresses: &[String],
) -> Result<BTreeMap<String, Attribution>> {
    ensure!(
        !addresses.is_empty() && addresses.len() <= 64,
        "intelligence dependency limit is 64 addresses"
    );
    let mut addresses = addresses
        .iter()
        .map(|a| model::address(a))
        .collect::<Result<Vec<_>>>()?;
    addresses.sort();
    addresses.dedup();
    let list = addresses
        .iter()
        .map(|a| aml_warehouse::publication::literal(a))
        .collect::<Vec<_>>()
        .join(",");
    let predicate = format!("network_id={{network:String}} AND address IN ({list})");
    let params = [("network", NETWORK_ID.into())];
    let claims: Vec<Value> = db.rows(&format!("SELECT * REPLACE(toString(revision) AS revision) FROM aml_intelligence_claims_current WHERE {predicate} ORDER BY claim_id LIMIT 6401"),&params).await?;
    let sources: Vec<Value> = db.rows(&format!("SELECT * REPLACE(toString(revision) AS revision) FROM aml_intelligence_sources_current WHERE network_id={{network:String}} AND source_id IN (SELECT source_id FROM aml_intelligence_claims_current WHERE {predicate}) ORDER BY source_id LIMIT 6401"),&params).await?;
    let reviews: Vec<Value> = db.rows(&format!("SELECT * REPLACE(toString(revision) AS revision,toString(subject_revision) AS subject_revision) FROM aml_intelligence_reviews_current WHERE network_id={{network:String}} AND subject_type='intelligence_claim' AND subject_id IN (SELECT claim_id FROM aml_intelligence_claims_current WHERE {predicate}) ORDER BY toUInt64(revision) DESC LIMIT 1 BY subject_id LIMIT 6401"),&params).await?;
    ensure!(
        claims.len() <= 6400 && sources.len() <= 6400 && reviews.len() <= 6400,
        "intelligence dependency rows exceeded limit"
    );
    let mut result = BTreeMap::new();
    for address in addresses {
        let claims = claims
            .iter()
            .filter(|c| c["address"] == address)
            .cloned()
            .collect::<Vec<_>>();
        ensure!(
            claims.len() <= LIMIT,
            "address has more than 100 current claims"
        );
        let ids = claims
            .iter()
            .filter_map(|c| c["claim_id"].as_str())
            .collect::<HashSet<_>>();
        let source_ids = claims
            .iter()
            .filter_map(|c| c["source_id"].as_str())
            .collect::<HashSet<_>>();
        let s = sources
            .iter()
            .filter(|s| source_ids.contains(s["source_id"].as_str().unwrap_or("")))
            .cloned()
            .collect();
        let r = reviews
            .iter()
            .filter(|r| ids.contains(r["subject_id"].as_str().unwrap_or("")))
            .cloned()
            .collect();
        result.insert(address.clone(), assemble(address, claims, s, r)?);
    }
    Ok(result)
}
fn assemble(
    address: String,
    claims: Vec<Value>,
    sources: Vec<Value>,
    reviews: Vec<Value>,
) -> Result<Attribution> {
    let by_source: HashMap<_, _> = sources
        .iter()
        .map(|r| (r["source_id"].as_str().unwrap_or(""), r))
        .collect();
    let by_claim: HashMap<_, _> = reviews
        .iter()
        .map(|r| (r["subject_id"].as_str().unwrap_or(""), r))
        .collect();
    let now = db::timestamp();
    let mut entities = Vec::new();
    let mut shown = Vec::new();
    for claim in &claims {
        let enabled = active(
            claim,
            by_source.get(claim["source_id"].as_str().unwrap_or("")),
            by_claim.get(claim["claim_id"].as_str().unwrap_or("")),
            now,
        );
        let mut row = public(claim.clone());
        row["active"] = json!(enabled);
        row["is_exposure_seed"] = json!(claim["is_exposure_seed"] == 1);
        if enabled {
            entities.push(row.clone());
        }
        shown.push(row);
    }
    let snapshot_id = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(
            &json!({"address":address,"network_id":NETWORK_ID,"claims":claims,"sources":sources,"reviews":reviews,"entities":entities})
        )?)
    );
    Ok(Attribution {
        snapshot: json!({"contract":"bitcoin_intelligence_v1","network_id":NETWORK_ID,"address":address,"snapshot_id":snapshot_id}),
        entities,
        claims: shown,
        sources: sources.into_iter().map(public).collect(),
    })
}
pub async fn history(db: &Database, claim_id: &str) -> Result<Value> {
    id(claim_id)?;
    let params = [("network", NETWORK_ID.into()), ("id", claim_id.into())];
    let reviews: Vec<Value> = db.rows("SELECT * REPLACE(toString(revision) AS revision,toString(subject_revision) AS subject_revision) FROM aml_intelligence_reviews_current WHERE network_id={network:String} AND subject_type='intelligence_claim' AND subject_id={id:String} ORDER BY toUInt64(revision) DESC LIMIT 101", &params).await?;
    Ok(
        json!({"network_id":NETWORK_ID,"claim_id":claim_id,"current_claim":current(db,CLAIMS,"claim_id",claim_id).await?,"truncated":reviews.len()>LIMIT,"reviews":reviews.into_iter().take(LIMIT).collect::<Vec<_>>()}),
    )
}
