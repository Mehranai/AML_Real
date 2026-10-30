//! Persist the shared evidence policy and immutable Bitcoin assessments locally.
//! A seal alone does not verify label/exposure dependencies; always use inspect.
use crate::{
    NETWORK_ID,
    db::{self, Database},
    exposure, intelligence, model,
};
use aml_risk_policy as policy;
use aml_warehouse::{Network, analytics, visibility};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
static NONCE: AtomicU64 = AtomicU64::new(0);
const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
fn same(a: &Value, b: &Value) -> bool {
    let integer = |v: &Value| v.as_u64().is_some() || v.as_i64().is_some();
    a == b
        || (a.is_number()
            && b.is_number()
            && (!integer(a) || !integer(b))
            && a.as_f64() == b.as_f64())
}

fn row(table: &str, mut value: Value) -> Result<Value> {
    let contract = aml_warehouse::current_contract()?;
    let schema = contract
        .tables
        .iter()
        .find(|t| t.name == table)
        .context("risk table missing")?;
    for c in &schema.columns {
        if value.get(&c.name).is_none() {
            let raw = c
                .default
                .as_deref()
                .with_context(|| format!("missing risk column {}", c.name))?;
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
fn policy_rows(at: u64) -> Result<(Value, Vec<Value>)> {
    let spec = policy::definition();
    let parent = row(
        "aml_risk_policies",
        json!({"network_id":NETWORK_ID,"policy_version":policy::POLICY,"policy_name":spec["policy_name"],"enabled":1,"medium_threshold":spec["medium_threshold"],"high_threshold":spec["high_threshold"],"revision":at,"created_at_unix_ms":at,"settings_json":spec.to_string(),"raw_evidence_json":spec.to_string()}),
    )?;
    let rules=spec["rules"].as_array().unwrap().iter().map(|r|row("aml_risk_policy_rules",json!({"network_id":NETWORK_ID,"policy_version":policy::POLICY,"rule_id":r["rule_id"],"signal_type":r["signal_type"],"max_contribution":r["max_contribution"],"description":r["description"],"enabled":1,"revision":at,"created_at_unix_ms":at,"settings_json":r["settings"].to_string(),"raw_evidence_json":r.to_string()}))).collect::<Result<Vec<_>>>()?;
    Ok((parent, rules))
}
pub async fn read_policy(db: &Database, version: &str) -> Result<Value> {
    ensure!(version.len() <= 128, "invalid policy version");
    let params = [("network", NETWORK_ID.into()), ("version", version.into())];
    let parents:Vec<Value>=db.rows("SELECT * FROM aml_risk_policies_current WHERE network_id={network:String} AND policy_version={version:String} LIMIT 2",&params).await?;
    let rules:Vec<Value>=db.rows("SELECT * FROM aml_risk_policy_rules_current WHERE network_id={network:String} AND policy_version={version:String} ORDER BY rule_id LIMIT 33",&params).await?;
    ensure!(
        parents.len() <= 1 && rules.len() <= 32,
        "ambiguous policy registry"
    );
    Ok(
        json!({"network_id":NETWORK_ID,"policy":parents.first(),"rules":rules,"persisted":!parents.is_empty(),"compiled_policy":policy::definition()}),
    )
}
fn policy_matches(stored: &Value) -> Result<bool> {
    let (parent, expected) = policy_rows(1)?;
    let equal = |actual: &Value, wanted: &Value| {
        wanted
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "revision" | "created_at_unix_ms"))
            .all(|(k, v)| same(&actual[k], v))
    };
    let Some(rules) = stored["rules"].as_array() else {
        return Ok(false);
    };
    Ok(equal(&stored["policy"], &parent)
        && rules.len() == expected.len()
        && expected.iter().all(|e| {
            rules
                .iter()
                .filter(|r| r["rule_id"] == e["rule_id"] && equal(r, e))
                .count()
                == 1
        }))
}
async fn ensure_policy(db: &Database) -> Result<()> {
    let stored = read_policy(db, policy::POLICY).await?;
    if stored["persisted"] == true {
        ensure!(
            policy_matches(&stored)?,
            "stored policy differs from compiled policy; use a new version"
        );
        return Ok(());
    }
    // Repeated/concurrent initial writes are identical in meaning; partial initial
    // policy publication is repaired, while a conflicting published policy fails.
    let (parent, rules) = policy_rows(db::timestamp().max(1))?;
    analytics::insert(
        &db.warehouse_connection(),
        db.database(),
        "aml_risk_policy_rules",
        &rules,
    )
    .await?;
    analytics::insert(
        &db.warehouse_connection(),
        db.database(),
        "aml_risk_policies",
        &[parent],
    )
    .await?;
    ensure!(
        policy_matches(&read_policy(db, policy::POLICY).await?)?,
        "policy publication incomplete"
    );
    Ok(())
}
async fn dependencies(db: &Database, address: &str, inputs: &Value) -> Result<bool> {
    if inputs["fingerprint"]["semantic_detection"].is_object()
        && !crate::semantics::current(&inputs["fingerprint"]["semantic_detection"])
    {
        return Ok(false);
    }
    if intelligence::read(db, address).await?.snapshot != inputs["intelligence_snapshot"] {
        return Ok(false);
    }
    if inputs["exposure_snapshot"].is_object() {
        let run = inputs["exposure_snapshot"]["run_id"]
            .as_str()
            .context("risk exposure identity missing")?;
        let actual = exposure::inspect(db, address, run).await?;
        if actual["current"] != true || actual["exposure_snapshot"] != inputs["exposure_snapshot"] {
            return Ok(false);
        }
        let sorted = |v: &Value| -> Result<Vec<Value>> {
            let mut paths = v.as_array().context("risk paths missing")?.clone();
            paths.sort_by_key(|p| p["path_id"].as_str().unwrap_or("").to_owned());
            Ok(paths)
        };
        if sorted(&actual["exposure_paths"])? != sorted(&inputs["exposure_paths"])? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k == "evidence_refs" {
                    if let Some(a) = v.as_array() {
                        out.extend(a.iter().filter_map(Value::as_str).map(str::to_owned));
                    }
                } else {
                    refs(v, out)
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                refs(v, out)
            }
        }
        _ => {}
    }
}
fn signal_rows(
    id: &str,
    address: &str,
    engine: &Value,
    snapshot: &visibility::Snapshot,
    revision: u64,
) -> Result<Vec<Value>> {
    engine["signals"].as_array().context("risk signals missing")?.iter().enumerate().map(|(index,s)| {
        let mut evidence_refs=Vec::new();refs(&s["evidence"],&mut evidence_refs);evidence_refs.sort();evidence_refs.dedup();
        row("aml_wallet_risk_signals",json!({"network_id":NETWORK_ID,"signal_id":policy::digest(&json!([id,index,s])),"assessment_id":id,"subject_id":address,"policy_version":policy::POLICY,"signal_type":s["signal_type"],"contribution":s["contribution"],"summary":s["summary"],"evidence_refs":evidence_refs,"evidence_json":s["evidence"].to_string(),"as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"revision":revision,"created_at_unix_ms":revision,"raw_evidence_json":s.to_string()}))
    }).collect()
}
fn frame(id: &str, address: &str, source: &Value, inputs: &Value, engine: &Value) -> Value {
    let mut value = json!({"contract":"bitcoin_risk_v1","network_id":NETWORK_ID,"address":address,"assessment_id":id,"policy_version":policy::POLICY,"policy_hash":policy::digest(&policy::definition()),"source_snapshot_id":source["snapshot_id"],"evidence_hash":policy::digest(inputs),"risk_engine_hash":policy::digest(engine)});
    value["snapshot_id"] = json!(policy::digest(&value));
    value["current"] = json!(true);
    value
}
pub fn validate_id(id: &str) -> Result<()> {
    ensure!(
        id.strip_prefix("btc-risk-")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        "invalid Bitcoin assessment ID"
    );
    Ok(())
}
/// Called only with the adapter's freshly assembled canonical investigation.
/// No HTTP endpoint accepts caller-supplied scores or arbitrary evidence.
pub async fn record(db: &Database, data: &Value) -> Result<Value> {
    let address = model::address(data["address"].as_str().context("risk address missing")?)?;
    ensure!(data["network_id"] == NETWORK_ID, "risk network mismatch");
    let inputs = policy::bitcoin_inputs(data);
    ensure!(
        serde_json::to_vec(&inputs)?.len() <= MAX_INPUT_BYTES,
        "risk evidence exceeds limit"
    );
    let engine = policy::assess("bitcoin", &inputs);
    let snapshot =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    ensure!(
        snapshot.value() == inputs["source_snapshot"],
        "risk source changed; recompute"
    );
    if snapshot.state().initialized == 0 {
        return Ok(
            json!({"risk_engine":engine,"risk_snapshot":null,"risk_persistence":{"status":"waiting_for_blocks","persisted":false}}),
        );
    }
    ensure!(
        dependencies(db, &address, &inputs).await?,
        "risk dependencies changed; recompute"
    );
    ensure_policy(db).await?;
    let revision = db::timestamp().max(1);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos()
        .to_string();
    let id = format!(
        "btc-risk-{}",
        policy::digest(&json!([
            snapshot.id(),
            address,
            nonce,
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ]))
    );
    let frame = frame(&id, &address, &snapshot.value(), &inputs, &engine);
    let signals = signal_rows(&id, &address, &engine, &snapshot, revision)?;
    let mut evidence_refs = Vec::new();
    refs(&engine, &mut evidence_refs);
    evidence_refs.sort();
    evidence_refs.dedup();
    let parent = row(
        "aml_wallet_risk_assessments",
        json!({"network_id":NETWORK_ID,"assessment_id":id,"subject_id":address,"policy_version":policy::POLICY,"risk_score":engine["risk_score"],"risk_level":engine["risk_level"],"probability_claimed":0,"signal_count":signals.len(),"exposure_run_id":if inputs["exposure_snapshot"].is_object(){inputs["exposure_snapshot"]["run_id"].clone()}else{Value::Null},"limitations":engine["limitations"],"as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"revision":revision,"created_at_unix_ms":revision,"top_reasons":engine["top_reasons"],"evidence_refs":evidence_refs,"scoring_method":engine["scoring_method"],"evidence_json":json!({"inputs":inputs,"risk_engine":engine,"snapshot":frame}).to_string()}),
    )?;
    let rows = BTreeMap::from([
        ("aml_wallet_risk_assessments".into(), vec![parent]),
        ("aml_wallet_risk_signals".into(), signals),
    ]);
    visibility::publish_analysis(
        &db.warehouse_connection(),
        db.database(),
        &snapshot,
        "risk",
        &id,
        revision,
        &rows,
    )
    .await?;
    let checked = inspect(db, &address, &id).await;
    if !checked.as_ref().is_ok_and(|v| v["current"] == true) {
        let _ = visibility::invalidate_analysis(
            &db.warehouse_connection(),
            db.database(),
            Network::Bitcoin,
            "risk",
            &id,
            revision.checked_add(1).context("risk revision exhausted")?,
        )
        .await;
        anyhow::bail!("risk dependencies/publication changed before completion; recompute");
    }
    Ok(
        json!({"risk_engine":engine,"risk_snapshot":frame,"risk_persistence":{"status":"complete","persisted":true,"assessment_id":id}}),
    )
}
pub async fn inspect(db: &Database, address: &str, id: &str) -> Result<Value> {
    validate_id(id)?;
    let address = model::address(address)?;
    let params = [
        ("network", NETWORK_ID.into()),
        ("id", id.into()),
        ("address", address.clone()),
    ];
    let parents:Vec<Value>=db.rows("SELECT * FROM aml_wallet_risk_assessments_current WHERE network_id={network:String} AND assessment_id={id:String} AND subject_id={address:String} LIMIT 2",&params).await?;
    ensure!(
        parents.len() == 1,
        "risk assessment not found or belongs to another subject"
    );
    let parent = &parents[0];
    let signals:Vec<Value>=db.rows("SELECT * FROM aml_wallet_risk_signals_current WHERE network_id={network:String} AND assessment_id={id:String} ORDER BY signal_id LIMIT 11",&params).await?;
    let blob: Value = serde_json::from_str(
        parent["evidence_json"]
            .as_str()
            .context("risk evidence missing")?,
    )?;
    let inputs = &blob["inputs"];
    let engine = &blob["risk_engine"];
    let mut frame = blob["snapshot"].clone();
    let source =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    let seal:Vec<Value>=db.rows("SELECT count() AS n FROM aml_wallet_risk_assessments_visible WHERE network_id={network:String} AND assessment_id={id:String}",&params).await?;
    let policy = read_policy(
        db,
        parent["policy_version"]
            .as_str()
            .context("risk policy missing")?,
    )
    .await?;
    let mut expected = signal_rows(
        id,
        &address,
        engine,
        &source,
        parent["revision"]
            .as_u64()
            .context("risk revision missing")?,
    )?;
    // Check historical rows against their historical source, not today's fence.
    for row in &mut expected {
        row["as_of_block"] = parent["as_of_block"].clone();
        row["source_revision"] = parent["source_revision"].clone();
    }
    expected.sort_by_key(|s| s["signal_id"].as_str().unwrap().to_owned());
    let signal_keys = [
        "network_id",
        "signal_id",
        "assessment_id",
        "subject_id",
        "policy_version",
        "signal_type",
        "contribution",
        "summary",
        "evidence_refs",
        "evidence_json",
        "as_of_block",
        "source_revision",
        "revision",
        "raw_evidence_json",
    ];
    let rows_match = signals.len() == expected.len()
        && signals
            .iter()
            .zip(&expected)
            .all(|(a, b)| signal_keys.iter().all(|k| same(&a[*k], &b[*k])));
    let mut evidence_refs = Vec::new();
    refs(engine, &mut evidence_refs);
    evidence_refs.sort();
    evidence_refs.dedup();
    let current = seal[0]["n"] == 1
        && frame["assessment_id"] == id
        && frame["address"] == address
        && parent["subject_id"] == address
        && parent["signal_count"].as_u64() == Some(signals.len() as u64)
        && same(&parent["risk_score"], &engine["risk_score"])
        && parent["risk_level"] == engine["risk_level"]
        && parent["probability_claimed"] == 0
        && parent["policy_version"] == policy::POLICY
        && parent["limitations"] == engine["limitations"]
        && parent["top_reasons"] == engine["top_reasons"]
        && parent["scoring_method"] == engine["scoring_method"]
        && parent["evidence_refs"] == json!(evidence_refs)
        && parent["exposure_run_id"]
            == if inputs["exposure_snapshot"].is_object() {
                inputs["exposure_snapshot"]["run_id"].clone()
            } else {
                Value::Null
            }
        && inputs["source_snapshot"] == source.value()
        && policy::bitcoin_binding_matches(&frame, inputs)
        && *engine == policy::assess("bitcoin", inputs)
        && frame == self::frame(id, &address, &inputs["source_snapshot"], inputs, engine)
        && rows_match
        && policy_matches(&policy)?
        && dependencies(db, &address, inputs).await?;
    visibility::unchanged(&db.warehouse_connection(), db.database(), &source).await?;
    frame["current"] = json!(current);
    Ok(
        json!({"network_id":NETWORK_ID,"address":address,"assessment_id":id,"current":current,"risk_snapshot":frame,"assessment":parent,"signals":signals,"policy":policy,
        "historical_risk_engine":engine,"risk_engine":if current {engine.clone()}else{json!({"risk_score":null,"risk_level":"UNKNOWN","signals":[],"policy_version":"source_snapshot_invalid","limitations":["Historical or incomplete risk assessment; recompute before using current risk."]})}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn float_storage_normalization_does_not_merge_exact_integer_revisions() {
        assert!(same(&json!(55), &json!(55.0)));
        assert!(!same(&json!(u64::MAX), &json!(u64::MAX - 1)));
        assert!(!same(
            &json!(9007199254740992u64),
            &json!(9007199254740993u64)
        ));
        assert!(!same(&Value::Null, &json!(0)));
    }
}
