use crate::{
    clustering::stable_id,
    config::AppConfig,
    db::{Client, database_client, now_ms, row},
};
use alloy::primitives::{U256, U512};
use aml_warehouse::{Network, visibility};
use anyhow::{Context, Result, ensure};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
const DETECTOR_VERSION: &str = "ethereum_exposure_propagation_v1";
#[derive(Debug, Clone, Copy)]
pub struct ExposureOptions {
    pub max_hops: u8,
    pub hop_decay: f64,
    pub time_half_life_days: f64,
    pub max_paths_per_subject: u16,
}

impl Default for ExposureOptions {
    fn default() -> Self {
        Self {
            max_hops: 5,
            hop_decay: 0.65,
            time_half_life_days: 365.0,
            max_paths_per_subject: 3,
        }
    }
}

impl ExposureOptions {
    fn validate(self) -> anyhow::Result<Self> {
        ensure!((1..=10).contains(&self.max_hops), "max_hops must be 1..=10");
        ensure!(
            self.hop_decay > 0.0 && self.hop_decay <= 1.0,
            "hop_decay must be greater than 0 and at most 1"
        );
        ensure!(
            self.time_half_life_days > 0.0,
            "time_half_life_days must be positive"
        );
        ensure!(
            (1..=25).contains(&self.max_paths_per_subject),
            "max_paths_per_subject must be 1..=25"
        );
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExposurePropagationReport {
    pub run_id: String,
    pub seed_count: u64,
    pub path_count: u64,
    pub max_hops: u8,
}
#[derive(Debug, Deserialize, Row)]
struct RunState {
    run_id: String,
    status: String,
}
pub(crate) async fn latest_complete_run(client: &Client, network: &str) -> Result<Option<String>> {
    let latest=client.query("SELECT run_id,status FROM eth_exposure_runs WHERE network_id=? ORDER BY revision DESC LIMIT 1").bind(network).fetch_optional::<RunState>().await?;
    let Some(run) = latest.filter(|r| r.status == "COMPLETE" || r.status == "NO_SEEDS") else {
        return Ok(None);
    };
    let visible = client
        .query("SELECT count() FROM aml_exposure_runs_visible WHERE network_id=? AND run_id=?")
        .bind(network)
        .bind(&run.run_id)
        .fetch_one::<u64>()
        .await?;
    Ok((visible == 1).then_some(run.run_id))
}
#[derive(Clone, Deserialize, Row)]
struct Edge {
    relationship_id: String,
    tx_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    from_address: String,
    to_address: String,
    asset_id: String,
    amount_raw: String,
}
#[derive(Clone)]
struct PathState {
    address: String,
    subjects: Vec<String>,
    relationships: Vec<String>,
    transactions: Vec<String>,
    asset: String,
    amount: String,
    share: f64,
    weight: f64,
    block: u64,
    first: u64,
    last: u64,
    stop: bool,
}
fn ratio(a: U256, total: U512) -> f64 {
    if total.is_zero() {
        0.0
    } else {
        (a.to_string().parse::<f64>().unwrap_or(0.0)
            / total.to_string().parse::<f64>().unwrap_or(f64::INFINITY))
        .clamp(0.0, 1.0)
    }
}
pub async fn propagate_exposure(
    config: &AppConfig,
    options: ExposureOptions,
) -> Result<ExposurePropagationReport> {
    let options = options.validate()?;
    let client = database_client(config);
    let snapshot = visibility::capture(&client, client.database(), Network::Ethereum).await?;
    if snapshot.state().initialized == 0 {
        return Ok(ExposurePropagationReport {
            run_id: "waiting_for_data".into(),
            seed_count: 0,
            path_count: 0,
            max_hops: options.max_hops,
        });
    }
    client.invalidate_active("exposure").await?;
    let rev = client.revision("aml_exposure_runs").await?;
    let run = format!("ethereum_exposure_{rev}");
    let now = now_ms();
    let result = produce(&client, &snapshot, &run, rev, now, options).await;
    if let Err(error) = &result {
        let failed = row(
            "aml_exposure_runs",
            json!({"run_id":run,"seed_id":"all_reviewed_seeds","status":"failed","as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"policy_version":DETECTOR_VERSION,"max_hops":options.max_hops,"started_at_unix_ms":now,"completed_at_unix_ms":now_ms(),"truncated":0,"error_message":error.to_string()}),
            client.revision("aml_exposure_runs").await?,
            now,
        )?;
        client.insert_rows("aml_exposure_runs", &[failed]).await?;
    }
    result
}
async fn produce(
    client: &Client,
    snapshot: &visibility::Snapshot,
    run: &str,
    rev: u64,
    now: u64,
    options: ExposureOptions,
) -> Result<ExposurePropagationReport> {
    let claims = client
        .rows_from("aml_intelligence_claims", "active", "claim_kind='label'")
        .await?;
    let seeds = claims
        .iter()
        .filter(|c| c["is_exposure_seed"] == 1 && c["risk_level"].as_u64().unwrap_or(0) > 0)
        .collect::<Vec<_>>();
    let entities = client
        .rows_from("aml_intelligence_claims", "active", "claim_kind='label'")
        .await?;
    let boundaries = entities
        .iter()
        .filter(|c| {
            matches!(
                c["entity_type"]
                    .as_str()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "exchange" | "custodian" | "bridge" | "mixer" | "dex"
            )
        })
        .filter_map(|c| {
            c["address"].as_str().map(|a| {
                (
                    a.to_owned(),
                    c["entity_id"].as_str().unwrap_or("").to_owned(),
                )
            })
        })
        .collect::<HashMap<_, _>>();
    let mut seed_ids = vec![];
    let mut output = vec![];
    let mut dependencies = vec![];
    let mut edge_budget = 0usize;
    let mut adjacency = HashMap::<(bool, String), Vec<Edge>>::new();
    for seed in &seeds {
        let seed_id = format!(
            "ethereum_seed:{}",
            seed["claim_id"].as_str().context("seed claim missing")?
        );
        seed_ids.push(seed_id.clone());
        let address = seed["address"].as_str().context("seed address missing")?;
        let record = row(
            "aml_exposure_seeds",
            json!({"seed_id":seed_id,"address":address,"claim_id":seed["claim_id"],"category":seed["seed_category"],"is_active":1,"entity_id":seed["entity_id"],"entity_name":seed["entity_name"],"entity_type":seed["entity_type"],"risk_level":seed["risk_level"],"source_id":seed["source_id"],"claim_revision":seed["revision"],"evidence_refs":seed["evidence_refs"]}),
            client.revision("aml_exposure_seeds").await?,
            now,
        )?;
        client
            .insert_rows("aml_exposure_seeds", &[record.clone()])
            .await?;
        dependencies.push(json!({"seed_id":seed_id,"revision":record["revision"],"claim_revision":seed["revision"]}));
        for direction in ["RECEIVED_FROM_SEED", "SENT_TO_SEED"] {
            let forward = direction == "RECEIVED_FROM_SEED";
            let score = |p: &PathState| {
                (seed["risk_level"].as_u64().unwrap_or(0) as f64 / 100.0)
                    * options.hop_decay.powi(p.relationships.len() as i32)
                    * p.share
                    * p.weight
            };
            let mut frontier = vec![PathState {
                address: address.into(),
                subjects: vec![address.into()],
                relationships: vec![],
                transactions: vec![],
                asset: String::new(),
                amount: "0".into(),
                share: 1.0,
                weight: 1.0,
                block: 0,
                first: 0,
                last: 0,
                stop: false,
            }];
            for _hop in 1..=options.max_hops {
                let mut candidates = Vec::new();
                for path in frontier.drain(..) {
                    if path.stop {
                        continue;
                    }
                    let cache_key = (forward, path.address.clone());
                    if !adjacency.contains_key(&cache_key) {
                        let field = if forward {
                            "from_address"
                        } else {
                            "to_address"
                        };
                        let edges=client.query(&format!("SELECT relationship_id,tx_hash,block_number,block_timestamp_unix_ms,ifNull(r.from_address,'') AS from_address,ifNull(r.to_address,'') AS to_address,asset_id,toString(r.amount_raw) AS amount_raw FROM aml_address_relationships_canonical r WHERE network_id='eip155:1' AND {field}=? AND r.amount_raw>0 AND from_address!='0x0000000000000000000000000000000000000000' AND to_address!='0x0000000000000000000000000000000000000000' ORDER BY block_number,transaction_index,event_index,event_sub_index LIMIT 10001")).bind(&path.address).fetch_all::<Edge>().await?;
                        ensure!(
                            edges.len() <= 10000,
                            "exposure adjacency exceeds bounded scope"
                        );
                        edge_budget += edges.len();
                        ensure!(
                            edge_budget <= 100000,
                            "exposure analysis exceeds bounded scope"
                        );
                        adjacency.insert(cache_key.clone(), edges);
                    }
                    let edges = &adjacency[&cache_key];
                    let mut totals = HashMap::<String, U512>::new();
                    for edge in edges {
                        let amount = edge.amount_raw.parse::<U256>()?;
                        let total = totals.entry(edge.asset_id.clone()).or_default();
                        *total = total
                            .checked_add(U512::from(amount))
                            .context("exposure amount aggregate overflow")?;
                    }
                    for edge in edges {
                        let next = if forward {
                            &edge.to_address
                        } else {
                            &edge.from_address
                        };
                        if path.subjects.contains(next)
                            || (!path.asset.is_empty() && path.asset != edge.asset_id)
                        {
                            continue;
                        }
                        if !path.relationships.is_empty()
                            && if forward {
                                edge.block_number <= path.block
                            } else {
                                edge.block_number >= path.block
                            }
                        {
                            continue;
                        }
                        let mut p = path.clone();
                        p.address = next.clone();
                        p.subjects.push(next.clone());
                        p.relationships.push(edge.relationship_id.clone());
                        p.transactions.push(edge.tx_hash.clone());
                        p.asset = edge.asset_id.clone();
                        p.amount = edge.amount_raw.clone();
                        p.block = edge.block_number;
                        p.last = edge.block_timestamp_unix_ms;
                        if p.first == 0 {
                            p.first = p.last;
                        }
                        p.share = p
                            .share
                            .min(ratio(edge.amount_raw.parse()?, totals[&edge.asset_id]));
                        let age =
                            now.saturating_sub(edge.block_timestamp_unix_ms) as f64 / 86400000.0;
                        p.weight = p.weight.min(
                            (-std::f64::consts::LN_2 * age / options.time_half_life_days).exp(),
                        );
                        p.stop = boundaries.contains_key(next);
                        candidates.push(p);
                        ensure!(
                            candidates.len() + output.len() <= 100000,
                            "exposure path budget exceeded"
                        );
                    }
                }
                // Rank the whole hop before applying the per-subject cap, so
                // RPC/source iteration order cannot discard a stronger path.
                candidates.sort_by(|a, b| {
                    score(b)
                        .total_cmp(&score(a))
                        .then_with(|| a.relationships.cmp(&b.relationships))
                });
                let mut retained = HashMap::<(String, String), usize>::new();
                for p in candidates {
                    let count = retained
                        .entry((p.address.clone(), p.asset.clone()))
                        .or_default();
                    if *count >= options.max_paths_per_subject as usize {
                        continue;
                    }
                    *count += 1;
                    let id = stable_id(
                        "ethereum_exposure_path",
                        &[run, &seed_id, direction, &p.relationships.join("|")],
                    );
                    output.push(row("aml_exposure_paths",json!({
                        "path_id":id,"run_id":run,"subject_id":p.address,"seed_id":seed_id,"direction":direction,
                        "hop_count":p.relationships.len(),"asset_id":p.asset,"amount_raw":p.amount,"amount_share":p.share,
                        "exposure_score":score(&p),"attribution_method":"observed_path","fund_continuity_claimed":0,
                        "path_subjects":p.subjects,"relationship_ids":p.relationships,"tx_hashes":p.transactions,
                        "evidence_refs":p.transactions.iter().map(|t|format!("tx:{t}")).collect::<Vec<_>>(),
                        "seed_entity_id":seed["entity_id"],"seed_category":seed["seed_category"],"seed_risk_level":seed["risk_level"],
                        "time_weight":p.weight,"service_mediated":u8::from(p.stop),"service_entity_id":boundaries.get(&p.address),
                        "continuity_type":"observed_same_asset_path","first_transfer_unix_ms":p.first.min(p.last),
                        "last_transfer_unix_ms":p.first.max(p.last),"detector_version":DETECTOR_VERSION,
                        "evidence_json":json!({"fund_continuity_claimed":false,"within_block_continuation":false}).to_string()
                    }),rev,now)?);
                    frontier.push(p);
                }
                if frontier.is_empty() {
                    break;
                }
            }
        }
    }
    let parent = row(
        "aml_exposure_runs",
        json!({"run_id":run,"seed_id":"all_reviewed_seeds","status":"complete","as_of_block":snapshot.state().last_block_number,"source_revision":snapshot.state().revision,"policy_version":DETECTOR_VERSION,"max_hops":options.max_hops,"started_at_unix_ms":now,"completed_at_unix_ms":now_ms(),"truncated":0,"seed_ids":seed_ids,"seed_snapshot_json":serde_json::to_string(&dependencies)?,"detector_version":DETECTOR_VERSION,"hop_decay":options.hop_decay,"time_half_life_days":options.time_half_life_days,"max_paths_per_subject":options.max_paths_per_subject,"seed_count":seeds.len(),"path_count":output.len()}),
        rev,
        now,
    )?;
    let count = output.len() as u64;
    let batches = BTreeMap::from([
        ("aml_exposure_runs".into(), vec![parent]),
        ("aml_exposure_paths".into(), output),
    ]);
    visibility::publish_analysis(
        client,
        client.database(),
        snapshot,
        "exposure",
        run,
        rev,
        &batches,
    )
    .await?;
    Ok(ExposurePropagationReport {
        run_id: run.into(),
        seed_count: seeds.len() as u64,
        path_count: count,
        max_hops: options.max_hops,
    })
}
