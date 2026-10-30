//! Bounded, on-demand UTXO exposure. Only sealed runs with unchanged chain and
//! attribution dependencies are current. All value attribution is an interval.
pub mod bounds;
mod trace;
use crate::{
    NETWORK_ID,
    db::{self, Database},
    intelligence, model,
};
use aml_warehouse::{Network, analytics, visibility};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
pub const POLICY: &str = "bitcoin_utxo_bounds_v1";
static NONCE: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub max_hops: u8,
    pub max_transactions: usize,
    pub max_paths: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            max_hops: 3,
            max_transactions: 100,
            max_paths: 50,
        }
    }
}
impl Options {
    pub fn validate(self) -> Result<()> {
        ensure!(
            (1..=5).contains(&self.max_hops)
                && (1..=200).contains(&self.max_transactions)
                && (1..=100).contains(&self.max_paths),
            "exposure limits: hops 1..5, transactions 1..200, paths 1..100"
        );
        Ok(())
    }
}
pub struct Outcome {
    pub paths: Vec<Value>,
    pub coverage: Value,
    pub snapshot: Value,
    pub run_id: Option<String>,
}
impl Outcome {
    pub fn value(&self) -> Value {
        json!({"network_id":NETWORK_ID,"exposure_paths":self.paths,"exposure_coverage":self.coverage,"exposure_snapshot":self.snapshot,"run_id":self.run_id})
    }
}
fn hash(value: &Value) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
fn row(table: &str, mut value: Value) -> Result<Value> {
    let contract = aml_warehouse::current_contract()?;
    let schema = contract
        .tables
        .iter()
        .find(|t| t.name == table)
        .context("missing exposure schema")?;
    for column in &schema.columns {
        if !value.as_object().unwrap().contains_key(&column.name) {
            let raw = column
                .default
                .as_ref()
                .with_context(|| format!("missing exposure column {}", column.name))?;
            value[&column.name] = if raw == "NULL" {
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
async fn put(db: &Database, table: &str, rows: &[Value]) -> Result<()> {
    analytics::insert(&db.warehouse_connection(), db.database(), table, rows).await
}
async fn dependencies(db: &Database, previous: &BTreeMap<String, Value>) -> Result<bool> {
    let current =
        intelligence::read_many(db, &previous.keys().cloned().collect::<Vec<_>>()).await?;
    Ok(current
        .into_iter()
        .all(|(a, v)| previous.get(&a) == Some(&v.snapshot)))
}
fn parent(
    id: &str,
    snapshot: &visibility::Snapshot,
    options: Options,
    revision: u64,
    status: &str,
    settings: Value,
    paths: &[Value],
    deps: &BTreeMap<String, Value>,
) -> Result<Value> {
    let seeds = paths
        .iter()
        .map(|p| p["seed_id"].as_str().unwrap().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    row(
        "aml_exposure_runs",
        json!({"network_id":NETWORK_ID,"run_id":id,"seed_id":"","status":status,"as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,
        "policy_version":POLICY,"max_hops":options.max_hops,"started_at_unix_ms":revision,"completed_at_unix_ms":db::timestamp(),"truncated":u8::from(status=="truncated"),"revision":revision,"created_at_unix_ms":revision,
        "seed_ids":seeds,"seed_count":seeds.len(),"path_count":paths.len(),"seed_snapshot_json":serde_json::to_string(deps)?,"detector_version":POLICY,"hop_decay":0.65,"time_half_life_days":365.0,
        "max_paths_per_subject":options.max_paths,"settings_json":settings.to_string(),"error_message":if status=="failed"{"analysis failed; recompute with a fresh run"}else{""}}),
    )
}
fn output_rows(
    id: &str,
    snapshot: &visibility::Snapshot,
    revision: u64,
    paths: &[Value],
) -> Result<BTreeMap<String, Vec<Value>>> {
    let mut stored = Vec::new();
    let mut summaries = Vec::new();
    for p in paths {
        stored.push(row("aml_exposure_paths",json!({"network_id":NETWORK_ID,"path_id":p["path_id"],"run_id":id,"subject_id":p["subject_id"],"seed_id":p["seed_id"],"direction":p["direction"],"hop_count":p["hop_count"],"asset_id":"native:BTC",
            "amount_raw":p["amount_raw"],"amount_share":p["amount_share"],"exposure_score":p["exposure_score"],"attribution_method":POLICY,"fund_continuity_claimed":0,"path_subjects":p["path_subjects"],"relationship_ids":p["relationship_ids"],"tx_hashes":p["tx_hashes"],"evidence_refs":p["evidence_refs"],
            "revision":revision,"created_at_unix_ms":revision,"seed_entity_id":p["seed_claim"]["entity_id"],"seed_category":p["seed_claim"]["seed_category"],"seed_risk_level":p["seed_claim"]["risk_level"],"time_weight":p["time_weight"],
            "service_mediated":u8::from(p["service_mediated"]==true),"service_entity_id":null,"continuity_type":p["continuity_type"],"first_transfer_unix_ms":p["first_transfer_unix_ms"],"last_transfer_unix_ms":p["last_transfer_unix_ms"],"detector_version":POLICY,
            "allocation_json":p["allocation"].to_string(),"evidence_json":p.to_string(),"raw_evidence_json":p.to_string()}))?);
        summaries.push(row("aml_address_exposures",json!({"network_id":NETWORK_ID,"exposure_id":p["path_id"],"run_id":id,"seed_id":p["seed_id"],"source_subject_id":p["seed_address"],"subject_id":p["subject_id"],"hop_distance":p["hop_count"],
            "exposure_score":p["exposure_score"],"path_count":1,"last_tx_hash":p["tx_hashes"].as_array().unwrap().last(),"last_seen_block":p["hops"].as_array().unwrap().last().unwrap()["block_number"],"exposure_type":p["direction"],"best_path_amount_share":p["amount_share"],"best_path_time_weight":p["time_weight"],
            "service_mediated":u8::from(p["service_mediated"]==true),"as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"evidence_refs":p["evidence_refs"],"raw_evidence_json":p.to_string(),"revision":revision,"created_at_unix_ms":revision}))?);
    }
    // Reconverging paths are correlated. Keep one strongest summary per seed
    // and direction; count paths, but never sum their amounts or scores.
    let mut grouped = BTreeMap::<String, Value>::new();
    for mut summary in summaries {
        let key = format!("{}:{}", summary["seed_id"], summary["exposure_type"]);
        summary["exposure_id"] = json!(hash(&json!([id, key]))?);
        if let Some(old) = grouped.get_mut(&key) {
            let count = old["path_count"].as_u64().unwrap() + 1;
            if summary["exposure_score"].as_f64().unwrap() > old["exposure_score"].as_f64().unwrap()
            {
                *old = summary;
            }
            old["path_count"] = json!(count);
        } else {
            grouped.insert(key, summary);
        }
    }
    Ok(BTreeMap::from([
        ("aml_exposure_paths".into(), stored),
        (
            "aml_address_exposures".into(),
            grouped.into_values().collect(),
        ),
    ]))
}
pub async fn compute(db: &Database, address: &str, options: Options) -> Result<Outcome> {
    options.validate()?;
    let address = model::address(address)?;
    let snapshot =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    if snapshot.state().initialized == 0 {
        return Ok(Outcome {
            paths: vec![],
            snapshot: Value::Null,
            run_id: None,
            coverage: json!({"status":"waiting_for_blocks","search_complete":false,"truncated":false,"policy_version":POLICY}),
        });
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos()
        .to_string();
    let id = format!(
        "btc-exp-{}",
        hash(&json!([
            snapshot.id(),
            address,
            nonce,
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ]))?
    );
    let revision = db::timestamp().max(1);
    let result=async {
        let time:Vec<Value>=db.rows("SELECT block_timestamp_unix_ms AS at FROM aml_ingested_blocks_canonical WHERE network_id={network:String} AND block_hash={hash:String} LIMIT 1",&[("network",NETWORK_ID.into()),("hash",snapshot.state().last_block_hash.clone())]).await?;
        let at=time.first().and_then(|v|v["at"].as_u64()).context("source tip timestamp unavailable")?;
        let mut scan=trace::scan(db,&address,options,at).await?;
        ensure!(dependencies(db,&scan.dependencies).await?,"exposure attribution changed during computation; retry");
        visibility::unchanged(&db.warehouse_connection(),db.database(),&snapshot).await?;
        let path_ids=scan.paths.iter().map(|p|p["path_id"].clone()).collect::<Vec<_>>();
        let frame=json!({"contract":"bitcoin_exposure_v1","network_id":NETWORK_ID,"address":address,"run_id":id,"path_ids":path_ids,"snapshot_id":hash(&json!([snapshot.id(),id,POLICY,scan.dependencies,path_ids]))?,"source_snapshot_id":snapshot.id(),"current":true});
        let coverage=json!({"status":if scan.truncated{"truncated"}else{"complete_within_bounds"},"truncated":scan.truncated,"search_complete":false,"max_hops":options.max_hops,"max_transactions":options.max_transactions,"max_paths":options.max_paths,
            "transactions_examined":scan.transactions,"states_examined":scan.states,"missing_prevout_context":scan.missing_prevouts,"service_boundaries":scan.service_boundaries,"policy_version":POLICY,"allocation":"conservative_lower_and_upper_satoshi_bounds","paths_additive":false,
            "run_id":id,"persisted":true,"source_snapshot":snapshot.value(),"limitations":["No paths is not proof of no exposure; the canonical ingested window and query budgets apply.","Only positive lower bounds may be scored; possible-only allocations and service-mediated routes remain context.","Paths can share outputs and must never be summed as independent amounts or votes."]});
        for p in &mut scan.paths {p["run_id"]=json!(id);p["policy_version"]=json!(POLICY);if scan.truncated {p["score_eligible"]=json!(false);p["exposure_score"]=json!(0.0);}}
        let settings=json!({"address":address,"options":options,"snapshot":frame,"coverage":coverage,"source_snapshot":snapshot.value()});
        let run=parent(&id,&snapshot,options,revision,if scan.truncated{"truncated"}else{"complete"},settings,&scan.paths,&scan.dependencies)?;
        let mut rows=output_rows(&id,&snapshot,revision,&scan.paths)?;rows.insert("aml_exposure_runs".into(),vec![run]);
        let mut seeds=BTreeMap::new();
        for p in &scan.paths {
            let c=&p["seed_claim"];
            seeds.insert(p["seed_id"].as_str().unwrap().to_owned(),row("aml_exposure_seeds",json!({"network_id":NETWORK_ID,"seed_id":p["seed_id"],"address":p["seed_address"],"claim_id":c["claim_id"],"category":c["seed_category"],"is_active":1,"revision":revision,"created_at_unix_ms":revision,
                "entity_id":c["entity_id"],"entity_name":c["entity_name"],"entity_type":c["entity_type"],"risk_level":c["risk_level"],"source_id":c["source_id"],"claim_revision":c["revision"].as_str().context("seed revision missing")?.parse::<u64>()?,"evidence_refs":c["evidence_refs"],"raw_evidence_json":json!({"claim":c,"snapshot":p["seed_snapshot"]}).to_string()}))?);
        }
        put(db,"aml_exposure_seeds",&seeds.into_values().collect::<Vec<_>>()).await?;
        if scan.truncated {for (table,batch) in rows {put(db,&table,&batch).await?;}}
        else {
            visibility::publish_analysis(&db.warehouse_connection(),db.database(),&snapshot,"exposure",&id,revision,&rows).await?;
            if !dependencies(db,&scan.dependencies).await? {
                visibility::invalidate_analysis(&db.warehouse_connection(),db.database(),Network::Bitcoin,"exposure",&id,revision.checked_add(1).context("revision exhausted")?).await?;
                anyhow::bail!("exposure attribution changed before publication; recompute");
            }
        }
        Ok::<_,anyhow::Error>(Outcome{paths:scan.paths,coverage,snapshot:if scan.truncated{Value::Null}else{frame},run_id:Some(id.clone())})
    }.await;
    if let Err(error) = result {
        // An interrupted unsealed attempt stays audit-only; handled errors also
        // retain a failed parent. Never delete facts or reuse the attempt ID.
        let failed = parent(
            &id,
            &snapshot,
            options,
            revision.saturating_add(2),
            "failed",
            json!({"address":address}),
            &[],
            &BTreeMap::new(),
        )?;
        let _ = put(db, "aml_exposure_runs", &[failed]).await;
        return Err(error.context(format!("Bitcoin exposure run {id} failed")));
    }
    result
}
pub fn validate_id(id: &str) -> Result<()> {
    ensure!(
        id.strip_prefix("btc-exp-").is_some_and(|s| s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
        "invalid exposure run id"
    );
    Ok(())
}
pub async fn inspect(db: &Database, address: &str, id: &str) -> Result<Value> {
    validate_id(id)?;
    let address = model::address(address)?;
    let params = [("network", NETWORK_ID.into()), ("run", id.into())];
    let runs:Vec<Value>=db.rows("SELECT * FROM aml_exposure_runs_current WHERE network_id={network:String} AND run_id={run:String} LIMIT 2",&params).await?;
    ensure!(runs.len() == 1, "exposure run not found or ambiguous");
    let run = &runs[0];
    let settings: Value = serde_json::from_str(
        run["settings_json"]
            .as_str()
            .context("run settings missing")?,
    )?;
    ensure!(
        settings["address"] == address,
        "exposure run belongs to another subject"
    );
    let mut frame = settings["snapshot"].clone();
    let stored:Vec<Value>=db.rows("SELECT evidence_json FROM aml_exposure_paths_current WHERE network_id={network:String} AND run_id={run:String} ORDER BY path_id LIMIT 101",&params).await?;
    let mut paths = stored
        .into_iter()
        .map(|v| {
            serde_json::from_str::<Value>(
                v["evidence_json"]
                    .as_str()
                    .context("exposure evidence missing")?,
            )
            .map_err(Into::into)
        })
        .collect::<Result<Vec<_>>>()?;
    let source =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    let seal:Vec<Value>=db.rows("SELECT count() AS n FROM aml_exposure_runs_visible WHERE network_id={network:String} AND run_id={run:String}",&params).await?;
    let deps: BTreeMap<String, Value> = serde_json::from_str(
        run["seed_snapshot_json"]
            .as_str()
            .context("run attribution snapshot missing")?,
    )?;
    let current = run["status"] == "complete"
        && seal[0]["n"] == 1
        && run["path_count"].as_u64() == Some(paths.len() as u64)
        && frame["contract"] == "bitcoin_exposure_v1"
        && frame["run_id"] == id
        && frame["network_id"] == NETWORK_ID
        && frame["address"] == address
        && frame["source_snapshot_id"] == settings["source_snapshot"]["snapshot_id"]
        && frame["path_ids"]
            == json!(
                paths
                    .iter()
                    .map(|p| p["path_id"].clone())
                    .collect::<Vec<_>>()
            )
        && settings["source_snapshot"] == source.value()
        && !deps.is_empty()
        && dependencies(db, &deps).await?;
    visibility::unchanged(&db.warehouse_connection(), db.database(), &source).await?;
    if !frame.is_object() {
        frame = json!({"contract":"bitcoin_exposure_v1","network_id":NETWORK_ID,"address":address,"run_id":id});
    }
    frame["current"] = json!(current);
    if !current {
        for path in &mut paths {
            path["score_eligible"] = json!(false);
            path["exposure_score"] = json!(0.0);
        }
    }
    Ok(
        json!({"network_id":NETWORK_ID,"address":address,"exposure_snapshot":frame,"exposure_paths":paths,"exposure_coverage":settings["coverage"],"run_status":run["status"],"current":current}),
    )
}
