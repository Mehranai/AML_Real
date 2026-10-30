use super::*;
use crate::bitcoin::transactions::{Leg, Tx};
use std::collections::{BTreeMap, BTreeSet};
fn family(address: &str) -> &'static str {
    if address.starts_with('1') {
        "p2pkh"
    } else if address.starts_with('3') {
        "p2sh"
    } else if address.starts_with("bc1p") {
        "taproot"
    } else if address.starts_with("bc1q") && address.len() == 42 {
        "p2wpkh"
    } else if address.starts_with("bc1q") && address.len() == 62 {
        "p2wsh"
    } else {
        "other"
    }
}
fn witness(l: &Leg) -> Value {
    json!({"index":l.index,"subject_id":l.subject,"value_sats":l.value.to_string(),
        "relationship_id":l.relationship_id.rsplit('/').next().unwrap(),"prev_tx_hash":l.prev_tx_hash,"prev_output_index":l.prev_output_index})
}
fn lead(
    tx: &Tx,
    heuristic: &str,
    members: BTreeSet<String>,
    confidence: f64,
    detail: Value,
) -> Value {
    let id = format!(
        "btc-cluster-{}",
        digest(&json!([VERSION, heuristic, tx.key(), members]))
    );
    let mut refs = BTreeSet::from([
        format!("tx:{}", tx.h.tx_hash),
        format!("block:{}", tx.h.block_hash),
    ]);
    for l in tx.inputs.iter().chain(&tx.outputs) {
        refs.insert(format!(
            "relationship:{}",
            l.relationship_id.rsplit('/').next().unwrap()
        ));
        if let (Some(hash), Some(index)) = (&l.prev_tx_hash, l.prev_output_index) {
            refs.insert(format!("outpoint:{hash}:{index}"));
        }
    }
    let evidence = json!({"block_number":tx.h.block_number,"block_hash":tx.h.block_hash,"tx_hash":tx.h.tx_hash,
        "transaction_index":tx.h.transaction_index,"block_timestamp_unix_ms":tx.h.block_timestamp_unix_ms,
        "commit_id":tx.h.commit_id,"revision":tx.h.revision.to_string(),"inputs":tx.inputs.iter().map(witness).collect::<Vec<_>>(),
        "outputs":tx.outputs.iter().map(witness).collect::<Vec<_>>(),"fee_sats":tx.fee.to_string(),"match":detail});
    json!({"cluster_id":id,"cluster_type":"investigation_lead","heuristic":heuristic,"members":members,
        "confidence":confidence,"confidence_semantics":"heuristic_match_strength_not_ownership_or_crime_probability",
        "detector_version":VERSION,"definition_hash":config()["definition_hash"],"evidence_refs":refs,"evidence":evidence,
        "evidence_hash":digest(&evidence),"review_status":"pending","review":null,"ownership_claimed":false,
        "same_entity":null,"score_eligible":false,"is_exposure_seed":false,"transitive_union":false})
}
pub(crate) fn detect(tx: &Tx) -> (Vec<Value>, Option<&'static str>) {
    if tx.h.coinbase == 1 {
        return (vec![], None);
    }
    if tx
        .inputs
        .iter()
        .any(|l| l.value == 0 || l.subject.starts_with("script:"))
        || tx
            .outputs
            .iter()
            .any(|l| l.value > 0 && l.subject.starts_with("script:"))
    {
        return (vec![], Some("script_or_zero_input_context"));
    }
    let inputs = tx
        .inputs
        .iter()
        .map(|l| l.subject.clone())
        .collect::<BTreeSet<_>>();
    let outs = tx
        .outputs
        .iter()
        .filter(|l| l.value > 0)
        .collect::<Vec<_>>();
    let mut equal = BTreeMap::<u64, BTreeSet<&str>>::new();
    for o in &outs {
        equal.entry(o.value).or_default().insert(&o.subject);
    }
    if inputs.len() >= 2 && equal.values().any(|s| s.len() >= 2) {
        return (vec![], Some("possible_collaborative_transaction"));
    }
    if inputs.len() > MAX_MEMBERS {
        return (vec![], Some("member_limit"));
    }
    let mut leads = vec![];
    if inputs.len() >= 2 {
        leads.push(lead(
            tx,
            "common_input_candidate",
            inputs,
            0.5,
            json!({"distinct_input_addresses":true,"control_not_proven":true}),
        ));
    } else if tx.inputs.len() == 1
        && outs.len() == 2
        && outs[0].subject != outs[1].subject
        && outs.iter().all(|o| o.subject != tx.inputs[0].subject)
    {
        let input = &tx.inputs[0].subject;
        let matches = outs
            .iter()
            .filter(|o| family(&o.subject) == family(input))
            .collect::<Vec<_>>();
        if family(input) != "other" && matches.len() == 1 {
            let out = matches[0];
            leads.push(lead(tx,"change_candidate",BTreeSet::from([input.clone(),out.subject.clone()]),0.3,
                json!({"candidate_output_index":out.index,"matching_address_family":family(input),"change_identified":false})));
        }
    }
    (leads, None)
}
