//! Versioned, descriptive Bitcoin patterns. No ownership or laundering inference.
use crate::{
    NETWORK_ID,
    bitcoin::transactions::{Header, Leg, Reader, Tx},
    db::Database,
    model,
};
use aml_warehouse::{publication::Block, visibility};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

pub const VERSION: &str = "bitcoin_semantics_v1";
const MAX_LEGS: usize = 256;
const MAX_ROOTS: usize = 25;
const MAX_CONTEXTS: usize = 60;
const MAX_TOTAL_LEGS: usize = 4096;
const MAX_EVENTS: usize = 128;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHAIN: usize = 5;
fn digest(v: &Value) -> String {
    aml_risk_policy::digest(v)
}
pub fn definition() -> Value {
    json!({"contract":"bitcoin_semantics_v1","network_id":NETWORK_ID,"detector_version":VERSION,
        "rules":{"consolidation":{"minimum_inputs":5,"positive_outputs":1,"confidence":0.6},
        "fan_out":{"minimum_inputs":1,"maximum_inputs":2,"minimum_positive_outputs":5,"minimum_output_subjects":3,"confidence":0.6},
        "possible_coinjoin":{"minimum_input_subjects":3,"minimum_equal_outputs_to_distinct_addresses":3,"confidence":0.35},
        "peel_step":{"inputs":1,"positive_outputs":2,"minimum_large_small_ratio":4,"confidence":0.45},
        "peel_chain":{"minimum_steps":3,"maximum_steps":MAX_CHAIN,"maximum_backward_steps":2,"link":"actual_spent_largest_output","confidence":0.5}},
        "limits":{"legs_per_transaction":MAX_LEGS,"roots":MAX_ROOTS,"context_transactions":MAX_CONTEXTS,"total_legs":MAX_TOTAL_LEGS,"events":MAX_EVENTS,"evidence_bytes":MAX_BYTES},
        "score_eligible":false,"ownership_claimed":false,"fund_continuity_claimed":false,
        "confidence_semantics":"versioned_pattern_match_strength_not_probability_of_crime",
        "limitations":["Project heuristics; thresholds are not a regulatory standard or a crime classifier.","Batch payments, wallet maintenance and collaborative transactions can match these patterns.","Equal outputs identify only possible CoinJoin; they do not identify every CoinJoin or a mixing service.","Largest output is a shape observation, not identification of change or common ownership.","No bridge or mixer event is produced without independent protocol evidence."]})
}
pub fn config() -> Value {
    let spec = definition();
    json!({"network_id":NETWORK_ID,"contract":"bitcoin_semantics_config_v1","detector_version":VERSION,"definition_hash":digest(&spec),"definition":spec})
}
pub fn current(metadata: &Value) -> bool {
    metadata["detector_version"] == VERSION
        && metadata["definition_hash"] == config()["definition_hash"]
}
fn positive(tx: &Tx) -> Vec<&Leg> {
    tx.outputs.iter().filter(|o| o.value > 0).collect()
}
fn peel(tx: &Tx) -> Option<usize> {
    if tx.h.coinbase == 1 || tx.inputs.len() != 1 || tx.inputs.len() + tx.outputs.len() > MAX_LEGS {
        return None;
    }
    let outputs = positive(tx);
    if outputs.len() != 2 {
        return None;
    }
    let (large, small) = if outputs[0].value > outputs[1].value {
        (outputs[0], outputs[1])
    } else {
        (outputs[1], outputs[0])
    };
    (large.value / small.value >= 4 && large.value < tx.inputs[0].value)
        .then_some(large.index as usize)
}
fn leg(leg: &Leg) -> Value {
    // Graph keys include network/block/commit/revision; preserve the native row
    // ID here, with the complete occurrence header alongside the leg witnesses.
    let relationship_id = native_relationship_id(leg);
    json!({"index":leg.index,"subject_id":leg.subject,"subject_type":if leg.subject.starts_with("script:"){"script"}else{"address"},
        "value_sats":leg.value.to_string(),"relationship_id":relationship_id,"prev_tx_hash":leg.prev_tx_hash,"prev_output_index":leg.prev_output_index})
}
fn native_relationship_id(leg: &Leg) -> &str {
    leg.relationship_id.rsplit('/').next().unwrap()
}
fn event(kind: &str, confidence: f64, txs: &[&Tx], details: Value) -> Value {
    let anchor = txs[0];
    let keys: Vec<String> = txs.iter().map(|t| t.key()).collect();
    let mut refs = BTreeSet::new();
    let transactions:Vec<Value>=txs.iter().map(|tx| {
        refs.insert(format!("tx:{}",tx.h.tx_hash));
        refs.insert(format!("block:{}",tx.h.block_hash));
        for l in tx.inputs.iter().chain(&tx.outputs) {
            refs.insert(format!("relationship:{}",native_relationship_id(l)));
            if let (Some(hash),Some(index))=(&l.prev_tx_hash,l.prev_output_index) {refs.insert(format!("outpoint:{hash}:{index}"));}
        }
        for l in &tx.outputs {refs.insert(format!("outpoint:{}:{}",tx.h.tx_hash,l.index));}
        json!({"subject_id":tx.key(),"block_number":tx.h.block_number,"block_hash":tx.h.block_hash,"tx_hash":tx.h.tx_hash,"transaction_index":tx.h.transaction_index,
            "block_timestamp_unix_ms":tx.h.block_timestamp_unix_ms,"commit_id":tx.h.commit_id,"revision":tx.h.revision.to_string(),
            "input_total_sats":tx.total.to_string(),"output_total_sats":tx.outputs.iter().map(|l|l.value).sum::<u64>().to_string(),"fee_sats":tx.fee.to_string(),
            "inputs":tx.inputs.iter().map(leg).collect::<Vec<_>>(),"outputs":tx.outputs.iter().map(leg).collect::<Vec<_>>()})
    }).collect();
    json!({"event_id":format!("btc-sem-{}",digest(&json!([VERSION,kind,keys]))),"network_id":NETWORK_ID,"event_type":kind,"protocol":"bitcoin",
        "subject_id":anchor.key(),"tx_hash":anchor.h.tx_hash,"detector_version":VERSION,"definition_hash":config()["definition_hash"],"confidence":confidence,
        "score_eligible":false,"ownership_claimed":false,"fund_continuity_claimed":false,"amounts_are_wallet_attributed":false,
        "evidence_refs":refs,"evidence":{"transactions":transactions,"match":details},"limitations":definition()["limitations"]})
}
fn detect(tx: &Tx) -> Vec<Value> {
    if tx.h.coinbase == 1 || tx.inputs.len() + tx.outputs.len() > MAX_LEGS {
        return vec![];
    }
    let outs = positive(tx);
    let mut events = Vec::new();
    if tx.inputs.len() >= 5 && outs.len() == 1 {
        events.push(event(
            "bitcoin_consolidation_candidate",
            0.6,
            &[tx],
            json!({"input_count":tx.inputs.len(),"positive_output_count":1}),
        ));
    }
    if (1..=2).contains(&tx.inputs.len())
        && outs.len() >= 5
        && outs
            .iter()
            .map(|o| &o.subject)
            .collect::<BTreeSet<_>>()
            .len()
            >= 3
    {
        events.push(event(
            "bitcoin_fan_out_candidate",
            0.6,
            &[tx],
            json!({"input_count":tx.inputs.len(),"positive_output_count":outs.len()}),
        ));
    }
    if tx
        .inputs
        .iter()
        .map(|i| &i.subject)
        .collect::<BTreeSet<_>>()
        .len()
        >= 3
    {
        let mut groups: BTreeMap<u64, BTreeSet<&str>> = BTreeMap::new();
        for o in &outs {
            if !o.subject.starts_with("script:") {
                groups.entry(o.value).or_default().insert(&o.subject);
            }
        }
        if let Some((value, subjects)) = groups
            .iter()
            .filter(|(_, s)| s.len() >= 3)
            .max_by_key(|(v, s)| (s.len(), **v))
        {
            events.push(event("bitcoin_possible_coinjoin",0.35,&[tx],json!({"equal_output_sats":value.to_string(),"distinct_equal_output_addresses":subjects,"identification":"possible_only"})));
        }
    }
    if let Some(index) = peel(tx) {
        events.push(event(
            "bitcoin_peel_step_candidate",
            0.45,
            &[tx],
            json!({"largest_output_index":index,"change_identified":false}),
        ));
    }
    events
}
fn fact(block: &Block, tx: &Tx, event: &Value) -> Value {
    json!({"network_id":NETWORK_ID,"block_number":block.block_number,"block_hash":block.block_hash,"block_timestamp_unix_ms":block.block_timestamp_unix_ms,
        "commit_id":block.commit_id,"revision":block.revision,"created_at_unix_ms":block.created_at_unix_ms,
        "event_id":event["event_id"],"tx_hash":tx.h.tx_hash,"subject_id":tx.key(),"event_type":event["event_type"],"protocol":"bitcoin",
        "detector_version":VERSION,"confidence":event["confidence"],"asset_in":"native:BTC","asset_out":"native:BTC","amount_in":tx.total.to_string(),
        "amount_out":tx.outputs.iter().map(|l|l.value).sum::<u64>().to_string(),"remote_network_id":null,"evidence_refs":event["evidence_refs"],
        "evidence_json":event.to_string(),"event_index":tx.h.transaction_index,"event_sub_index":0,"detector":"bitcoin_patterns","raw_evidence_json":event.to_string()})
}
/// Called before the block's complete marker: semantic rows share its fence/counts.
pub fn block_facts(block: &Block, evidence: &model::Evidence) -> Result<Vec<Value>> {
    let mut inputs: HashMap<&str, Vec<Leg>> = HashMap::new();
    let mut outputs: HashMap<&str, Vec<Leg>> = HashMap::new();
    for i in &evidence.inputs {
        inputs.entry(&i.txid).or_default().push(Leg {
            index: i.input_index,
            subject: i.output.subject(),
            value: i.output.value_sats,
            relationship_id: format!("{}:{}:in:{}", block.block_hash, i.txid, i.input_index),
            prev_tx_hash: Some(i.prev_txid.clone()),
            prev_output_index: Some(i.prev_vout),
        });
    }
    for o in &evidence.outputs {
        outputs.entry(&o.txid).or_default().push(Leg {
            index: o.output_index,
            subject: o.output.subject(),
            value: o.output.value_sats,
            relationship_id: format!("{}:{}:out:{}", block.block_hash, o.txid, o.output_index),
            prev_tx_hash: None,
            prev_output_index: None,
        });
    }
    let mut rows = Vec::new();
    for t in &evidence.transactions {
        let ins = inputs.remove(t.txid.as_str()).unwrap_or_default();
        let outs = outputs.remove(t.txid.as_str()).unwrap_or_default();
        let tx = Tx {
            h: Header {
                block_number: block.block_number,
                block_hash: block.block_hash.clone(),
                tx_hash: t.txid.clone(),
                transaction_index: t.tx_index,
                block_timestamp_unix_ms: block.block_timestamp_unix_ms,
                commit_id: block.commit_id.clone(),
                revision: block.revision,
                coinbase: t.coinbase,
                input_count: Some(t.input_count),
                output_count: Some(t.output_count),
                fee_raw: t.fee_sats.to_string(),
            },
            total: ins.iter().map(|l| l.value).sum(),
            fee: t.fee_sats,
            inputs: ins,
            outputs: outs,
        };
        rows.extend(detect(&tx).iter().map(|e| fact(block, &tx, e)));
    }
    Ok(rows)
}
pub fn block_manifest(evidence: &model::Evidence) -> Value {
    let skipped: Vec<&str> = evidence
        .transactions
        .iter()
        .filter(|t| t.coinbase == 0 && t.input_count as usize + t.output_count as usize > MAX_LEGS)
        .map(|t| t.txid.as_str())
        .collect();
    json!({"detector_version":VERSION,"definition_hash":config()["definition_hash"],"scope":"single_transaction_patterns",
        "processed_transactions":evidence.transactions.iter().filter(|t|t.coinbase==0).count()-skipped.len(),"ignored_coinbase":evidence.transactions.iter().filter(|t|t.coinbase==1).count(),
        "skipped_transactions":skipped.len(),"skipped_tx_hashes":skipped.iter().take(25).collect::<Vec<_>>(),"skipped_ids_truncated":skipped.len()>25,"maximum_legs_per_transaction":MAX_LEGS,"peel_chains":"computed_at_investigation"})
}
async fn chain(reader: &mut Reader<'_>, root: Arc<Tx>) -> Result<Vec<Arc<Tx>>> {
    if peel(&root).is_none() {
        return Ok(vec![]);
    }
    let mut txs = vec![root];
    for _ in 0..2 {
        let first = &txs[0];
        let Some((parent, index)) = reader.previous(first, &first.inputs[0]).await? else {
            break;
        };
        if peel(&parent) != Some(index) {
            break;
        }
        txs.insert(0, parent);
    }
    while txs.len() < MAX_CHAIN {
        let last = txs.last().unwrap();
        let Some((next, input)) = reader.spender(last, peel(last).unwrap()).await? else {
            break;
        };
        if input != 0 || peel(&next).is_none() {
            break;
        }
        txs.push(next);
    }
    Ok(txs)
}
/// Bounded read of canonical facts. Historical blocks are never rewritten here.
pub async fn read(db: &Database, address: &str, analytics: &Value) -> Result<Value> {
    model::address(address)?;
    let snapshot =
        visibility::Snapshot::from_state(crate::warehouse::to_state(&db.state().await?))?;
    ensure!(
        snapshot.value() == analytics["source_snapshot"],
        "semantic activity source changed; retry"
    );
    let roots = analytics["recent_transactions"]
        .as_array()
        .context("semantic roots missing")?;
    let mut reader = Reader::new(db, MAX_CONTEXTS, MAX_TOTAL_LEGS, MAX_LEGS);
    let mut events = BTreeMap::<String, Value>::new();
    let mut skipped = Vec::new();
    let mut verified = 0;
    let mut computed = 0;
    let mut bytes = 0;
    let mut event_limit = false;
    for root in roots.iter().take(MAX_ROOTS) {
        let block = root["block_hash"]
            .as_str()
            .context("semantic block missing")?;
        let hash = root["tx_hash"]
            .as_str()
            .context("semantic transaction missing")?;
        let Some(tx) = reader.load(block, hash).await? else {
            skipped.push(json!({"block_hash":block,"tx_hash":hash}));
            continue;
        };
        let expected = detect(&tx);
        let stored:Vec<Value>=db.rows("SELECT event_id,evidence_json FROM aml_semantic_aml_events_canonical WHERE network_id={network:String} AND block_hash={block:String} AND tx_hash={tx:String} AND detector_version={version:String} LIMIT 5",&[("network",NETWORK_ID.into()),("block",block.into()),("tx",hash.into()),("version",VERSION.into())]).await?;
        ensure!(stored.len() <= 4, "too many canonical semantic events");
        for row in &stored {
            let body: Value = serde_json::from_str(
                row["evidence_json"]
                    .as_str()
                    .context("semantic evidence missing")?,
            )?;
            ensure!(
                expected
                    .iter()
                    .any(|e| e["event_id"] == row["event_id"] && *e == body),
                "stored semantic evidence differs from canonical transaction or compiled detector"
            );
        }
        let mut candidates = expected;
        let txs = chain(&mut reader, tx.clone()).await?;
        if txs.len() >= 3 {
            candidates.push(event("bitcoin_peel_chain_candidate",0.5,&txs.iter().map(AsRef::as_ref).collect::<Vec<_>>(),json!({"step_count":txs.len(),"link":"actual_spent_largest_output","change_identified":false,"overlapping_chains_additive":false})));
        }
        for mut candidate in candidates {
            let id = candidate["event_id"].as_str().unwrap().to_owned();
            if events.contains_key(&id) {
                continue;
            }
            let persisted = stored.iter().any(|r| r["event_id"] == id);
            candidate["storage"] = json!(if persisted {
                "canonical_fact"
            } else {
                "computed_from_canonical_facts"
            });
            let size = candidate.to_string().len();
            if events.len() >= MAX_EVENTS || bytes + size > MAX_BYTES {
                event_limit = true;
                break;
            }
            if persisted {
                verified += 1;
            } else {
                computed += 1;
            }
            bytes += size;
            events.insert(id, candidate);
        }
        if event_limit {
            break;
        }
    }
    let events: Vec<Value> = events.into_values().collect();
    let roots_truncated =
        roots.len() > MAX_ROOTS || analytics["activity_coverage"]["transactions_truncated"] == true;
    let coverage = json!({"status":if snapshot.state().initialized==0{"waiting_for_blocks"}else if roots_truncated || reader.truncated || event_limit || reader.missing>0 {"partial"}else{"complete_within_bounded_scope"},
        "detector_version":VERSION,"definition_hash":config()["definition_hash"],"root_scope":"recent_transactions_in_selected_activity_scope","query_scope":analytics["query_scope"]["analytics"],
        "context_scope":"canonical_outpoint_neighbors_may_extend_outside_activity_filters","roots_available":roots.len(),"roots_truncated":roots_truncated,"roots_limit":MAX_ROOTS,
        "context_transactions":reader.cache.len(),"context_limited":reader.truncated,"missing_prevouts":reader.missing,"skipped_roots":skipped,"event_limit_reached":event_limit,
        "canonical_fact_events":verified,"computed_events":computed,"historical_fact_backfill_complete":false,"limits":definition()["limits"],
        "no_events_does_not_prove_no_patterns":true,"score_eligible":false,"ownership_claimed":false,"fund_continuity_claimed":false});
    let mut semantic_snapshot = json!({"contract":"bitcoin_semantics_v1","network_id":NETWORK_ID,"detector_version":VERSION,"definition_hash":config()["definition_hash"],
        "source_snapshot_id":snapshot.id(),"root_occurrences":roots.iter().take(MAX_ROOTS).map(|r|json!([r["block_hash"],r["tx_hash"]])).collect::<Vec<_>>(),"events_hash":digest(&json!(events)),"coverage_hash":digest(&coverage)});
    semantic_snapshot["snapshot_id"] = json!(digest(&semantic_snapshot));
    visibility::unchanged(&db.warehouse_connection(), db.database(), &snapshot).await?;
    Ok(
        json!({"network_id":NETWORK_ID,"address":address,"source_snapshot":snapshot.value(),"semantic_events":events,"semantic_coverage":coverage,"semantic_snapshot":semantic_snapshot}),
    )
}

#[cfg(test)]
mod tests;
