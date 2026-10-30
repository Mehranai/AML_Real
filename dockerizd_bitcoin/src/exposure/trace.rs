use super::{Options, POLICY, bounds};
use crate::bitcoin::transactions::{Leg, Reader, Tx};
use crate::{NETWORK_ID, db::Database, intelligence};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

#[derive(Clone)]
struct Hop {
    tx: Arc<Tx>,
    ins: Vec<usize>,
    out: usize,
}
struct Work {
    tx: Arc<Tx>,
    indices: Vec<usize>,
    path: Vec<Hop>,
    incoming: bool,
}
pub struct Scan {
    pub paths: Vec<Value>,
    pub dependencies: BTreeMap<String, Value>,
    pub truncated: bool,
    pub transactions: usize,
    pub states: usize,
    pub missing_prevouts: usize,
    pub service_boundaries: usize,
}
struct Loader<'a> {
    db: &'a Database,
    reader: Reader<'a>,
    labels: BTreeMap<String, intelligence::Attribution>,
    truncated: bool,
    boundaries: usize,
}
fn text(row: &Value, key: &str) -> Result<String> {
    Ok(row[key]
        .as_str()
        .with_context(|| format!("missing exposure field {key}"))?
        .into())
}
impl Loader<'_> {
    async fn labels(&mut self, subject: &str) -> Result<Vec<Value>> {
        if subject.starts_with("script:") {
            return Ok(vec![]);
        }
        if !self.labels.contains_key(subject) {
            if self.labels.len() >= 64 {
                self.truncated = true;
                return Ok(vec![]);
            }
            self.labels
                .insert(subject.into(), intelligence::read(self.db, subject).await?);
        }
        Ok(self.labels[subject].entities.clone())
    }
    async fn load(&mut self, block: &str, hash: &str) -> Result<Option<Arc<Tx>>> {
        let result = self.reader.load(block, hash).await;
        self.truncated |= self.reader.truncated;
        result
    }
    async fn previous(&mut self, tx: &Tx, input: &Leg) -> Result<Option<(Arc<Tx>, usize)>> {
        let result = self.reader.previous(tx, input).await;
        self.truncated |= self.reader.truncated;
        result
    }
    async fn spender(&mut self, tx: &Tx, index: usize) -> Result<Option<(Arc<Tx>, usize)>> {
        let result = self.reader.spender(tx, index).await;
        self.truncated |= self.reader.truncated;
        result
    }
}
pub fn service(labels: &[Value]) -> bool {
    labels.iter().any(|v| {
        matches!(
            v["entity_type"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "exchange" | "custodian" | "bridge" | "dex" | "mixer"
        )
    })
}
fn seed(claim: &Value, at: u64) -> bool {
    claim["is_exposure_seed"] == true
        && claim["risk_level"].as_u64().is_some_and(|v| v > 0)
        && claim["valid_from_unix_ms"]
            .as_u64()
            .is_some_and(|v| v <= at)
        && (claim["valid_to_unix_ms"].is_null()
            || claim["valid_to_unix_ms"].as_u64().is_some_and(|v| v > at))
}
fn make_path(
    address: &str,
    incoming: bool,
    path: &[Hop],
    claim: &Value,
    seed_snapshot: &Value,
    mediated: bool,
    at: u64,
) -> Result<Option<Value>> {
    let mut lower = path[0].ins.iter().try_fold(0u64, |n, i| {
        n.checked_add(path[0].tx.inputs[*i].value)
            .context("origin amount overflow")
    })?;
    let mut upper = lower;
    let mut allocation = Vec::new();
    let mut hops = Vec::new();
    let mut ids = Vec::new();
    let mut subjects = vec![path[0].tx.inputs[path[0].ins[0]].subject.clone()];
    for (i, hop) in path.iter().enumerate() {
        let output = &hop.tx.outputs[hop.out];
        if i > 0 {
            let prior = &path[i - 1];
            let prev = &hop.tx.inputs[hop.ins[0]];
            ensure!(
                hop.ins.len() == 1
                    && prev.prev_tx_hash.as_deref() == Some(&prior.tx.h.tx_hash)
                    && prev.prev_output_index == Some(prior.out as u32)
                    && prev.value == prior.tx.outputs[prior.out].value
                    && prev.subject == prior.tx.outputs[prior.out].subject
                    && prior.tx.position() < hop.tx.position(),
                "exposure route does not follow an actual spent outpoint"
            );
        }
        let (lo, hi) = bounds::allocate(lower, upper, hop.tx.total, output.value)?;
        allocation.push(json!({"lower_input_sats":lower.to_string(),"upper_input_sats":upper.to_string(),"total_input_sats":hop.tx.total.to_string(),"output_sats":output.value.to_string(),"fee_sats":hop.tx.fee.to_string(),"lower_output_sats":lo.to_string(),"upper_output_sats":hi.to_string()}));
        lower = lo;
        upper = hi;
        let inputs = hop
            .ins
            .iter()
            .map(|i| hop.tx.inputs[*i].clone())
            .collect::<Vec<_>>();
        ids.extend(inputs.iter().map(|v| v.relationship_id.clone()));
        ids.push(output.relationship_id.clone());
        subjects.push(output.subject.clone());
        hops.push(json!({"tx_hash":hop.tx.h.tx_hash,"block_hash":hop.tx.h.block_hash,"block_number":hop.tx.h.block_number,"transaction_index":hop.tx.h.transaction_index,
            "timestamp_unix_ms":hop.tx.h.block_timestamp_unix_ms,"commit_id":hop.tx.h.commit_id,"revision":hop.tx.h.revision.to_string(),"inputs":inputs,"output":output}));
    }
    if upper == 0 {
        return Ok(None);
    }
    let last = path.last().unwrap();
    let output = last.tx.outputs[last.out].value;
    let share = lower as f64 / output.max(1) as f64;
    let weight = 0.5f64
        .powf(at.saturating_sub(last.tx.h.block_timestamp_unix_ms) as f64 / 86_400_000.0 / 365.0);
    let strength = (claim["confidence"].as_f64().unwrap_or(0.0)
        * claim["risk_level"].as_f64().unwrap_or(0.0)
        / 100.0
        * share
        * 0.65f64.powi(path.len() as i32 - 1)
        * weight)
        .clamp(0.0, 1.0);
    let seed_id = format!(
        "btc-seed-{:x}",
        Sha256::digest(serde_json::to_vec(&json!([
            claim["claim_id"],
            seed_snapshot
        ]))?)
    );
    let path_id = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!([
            address,
            incoming,
            claim["claim_id"],
            ids
        ]))?)
    );
    let tx_hashes = path
        .iter()
        .map(|h| h.tx.h.tx_hash.clone())
        .collect::<Vec<_>>();
    let mut refs = claim["evidence_refs"]
        .as_array()
        .context("seed evidence missing")?
        .clone();
    for hop in path {
        refs.push(json!(format!("tx:{}", hop.tx.h.tx_hash)));
        refs.push(json!(format!("outpoint:{}:{}", hop.tx.h.tx_hash, hop.out)));
    }
    Ok(Some(
        json!({"network_id":NETWORK_ID,"path_id":path_id,"run_id":"","subject_id":address,"seed_id":seed_id,"seed_address":claim["address"],"seed_claim":claim,"seed_snapshot":seed_snapshot,
        "direction":if incoming {"RECEIVED_FROM_SEED"}else{"SENT_TO_SEED"},"hop_count":path.len(),"asset_id":"native:BTC","amount_raw":lower.to_string(),"amount_share":share,
        "exposure_score":if mediated {0.0}else{strength},"score_eligible":lower>0 && !mediated,"attribution_method":POLICY,"fund_continuity_claimed":false,"ownership_claimed":false,
        "service_mediated":mediated,"continuity_type":"exact_outpoints_with_conservative_value_bounds","path_subjects":subjects,"path_addresses":subjects,
        "tx_hashes":tx_hashes,"relationship_ids":ids,"evidence_refs":refs,"hops":hops,"time_weight":weight,"hop_decay":0.65,
        "first_transfer_unix_ms":path[0].tx.h.block_timestamp_unix_ms,"last_transfer_unix_ms":last.tx.h.block_timestamp_unix_ms,
        "allocation":{"method":POLICY,"minimum_sats":lower.to_string(),"maximum_sats":upper.to_string(),"amount_basis":"lower_bound","paths_additive":false,"steps":allocation}}),
    ))
}
pub async fn scan(db: &Database, address: &str, options: Options, at: u64) -> Result<Scan> {
    let mut loader = Loader {
        db,
        reader: Reader::new(db, options.max_transactions, 4000, 2000),
        labels: BTreeMap::new(),
        truncated: false,
        boundaries: 0,
    };
    let subject_service = service(&loader.labels(address).await?);
    let roots:Vec<Value>=db.rows(&format!("SELECT block_hash,tx_hash,block_number,transaction_index FROM aml_address_relationships_canonical WHERE network_id={{network:String}} AND (from_address={{address:String}} OR to_address={{address:String}}) GROUP BY block_hash,tx_hash,block_number,transaction_index ORDER BY block_number DESC,transaction_index DESC LIMIT {}",options.max_transactions+1),&[("network",NETWORK_ID.into()),("address",address.into())]).await?;
    loader.truncated |= roots.len() > options.max_transactions;
    let mut queue = VecDeque::new();
    for root in roots.iter().take(options.max_transactions) {
        let Some(tx) = loader
            .load(&text(root, "block_hash")?, &text(root, "tx_hash")?)
            .await?
        else {
            continue;
        };
        if tx.h.coinbase == 1 {
            continue;
        }
        for (i, out) in tx
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, v)| v.subject == address && v.value > 0)
        {
            if queue.len() >= 1000 {
                loader.truncated = true;
                break;
            }
            let _ = out;
            queue.push_back(Work {
                tx: tx.clone(),
                indices: vec![i],
                path: vec![],
                incoming: true,
            });
        }
        let ins = tx
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, v)| v.subject == address)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if !ins.is_empty() && queue.len() >= 1000 {
            loader.truncated = true;
        }
        if !ins.is_empty() && queue.len() < 1000 {
            queue.push_back(Work {
                tx,
                indices: ins,
                path: vec![],
                incoming: false,
            });
        }
    }
    let mut paths = BTreeMap::<String, Value>::new();
    let mut states = 0;
    let mut bytes = 0;
    'walk: while let Some(work) = queue.pop_front() {
        let choices = if work.incoming {
            work.tx.inputs.len()
        } else {
            work.tx.outputs.len()
        };
        for index in 0..choices {
            states += 1;
            if states > 1000 {
                loader.truncated = true;
                break 'walk;
            }
            let leg = if work.incoming {
                &work.tx.inputs[index]
            } else {
                &work.tx.outputs[index]
            };
            let labels = loader.labels(&leg.subject).await?;
            let boundary = service(&labels);
            let mut found = false;
            let mut path = work.path.clone();
            let hop = Hop {
                tx: work.tx.clone(),
                ins: if work.incoming {
                    vec![index]
                } else {
                    work.indices.clone()
                },
                out: if work.incoming {
                    work.indices[0]
                } else {
                    index
                },
            };
            if work.incoming {
                path.insert(0, hop);
            } else {
                path.push(hop);
            }
            for claim in labels
                .iter()
                .filter(|c| c["address"] != address && seed(c, work.tx.h.block_timestamp_unix_ms))
            {
                found = true;
                let mut route = path.clone();
                if work.incoming {
                    route[0].ins = work
                        .tx
                        .inputs
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| v.subject == leg.subject)
                        .map(|(i, _)| i)
                        .collect();
                }
                let snapshot = &loader.labels[&leg.subject].snapshot;
                if let Some(p) = make_path(
                    address,
                    work.incoming,
                    &route,
                    claim,
                    snapshot,
                    subject_service || boundary,
                    at,
                )? {
                    let key = p["path_id"].as_str().unwrap().to_owned();
                    if !paths.contains_key(&key) {
                        let size = serde_json::to_vec(&p)?.len();
                        if paths.len() >= options.max_paths || bytes + size > 4 * 1024 * 1024 {
                            loader.truncated = true;
                            continue;
                        }
                        bytes += size;
                        paths.insert(key, p);
                    }
                }
            }
            if boundary {
                loader.boundaries += 1;
            }
            if found || boundary || subject_service || path.len() >= usize::from(options.max_hops) {
                continue;
            }
            let next = if work.incoming {
                loader.previous(&work.tx, leg).await?
            } else {
                loader.spender(&work.tx, index).await?
            };
            if let Some((tx, i)) = next {
                if tx.h.coinbase == 1 {
                    continue;
                }
                if queue.len() >= 1000 {
                    loader.truncated = true;
                } else {
                    queue.push_back(Work {
                        tx,
                        indices: vec![i],
                        path,
                        incoming: work.incoming,
                    });
                }
            }
        }
    }
    Ok(Scan {
        paths: paths.into_values().collect(),
        dependencies: loader
            .labels
            .into_iter()
            .map(|(a, v)| (a, v.snapshot))
            .collect(),
        truncated: loader.truncated,
        transactions: loader.reader.cache.len(),
        states,
        missing_prevouts: loader.reader.missing,
        service_boundaries: loader.boundaries,
    })
}
