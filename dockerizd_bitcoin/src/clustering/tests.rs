use super::*;
use crate::bitcoin::transactions::{Header, Leg, Tx};
fn address(byte: u8, p2sh: bool) -> String {
    let hex = if p2sh {
        format!("a914{}87", format!("{byte:02x}").repeat(20))
    } else {
        format!("76a914{}88ac", format!("{byte:02x}").repeat(20))
    };
    crate::model::ScriptOutput::from_rpc(&json!({"value":"0.00000001","scriptPubKey":{"hex":hex}}))
        .unwrap()
        .address
}
fn tx(ins: Vec<(String, u64)>, outs: Vec<(String, u64)>) -> Tx {
    let leg = |(i, (subject, value)): (usize, (String, u64))| Leg {
        index: i as u32,
        subject,
        value,
        relationship_id: format!("native-{i}"),
        prev_tx_hash: Some("a".repeat(64)),
        prev_output_index: Some(i as u32),
    };
    Tx {
        h: Header {
            block_number: 1,
            block_hash: "b".repeat(64),
            tx_hash: "c".repeat(64),
            transaction_index: 1,
            block_timestamp_unix_ms: 1000,
            commit_id: "fixture".into(),
            revision: 1,
            coinbase: 0,
            input_count: Some(ins.len() as u32),
            output_count: Some(outs.len() as u32),
            fee_raw: "1".into(),
        },
        total: ins.iter().map(|x| x.1).sum(),
        fee: 1,
        inputs: ins.into_iter().enumerate().map(leg).collect(),
        outputs: outs.into_iter().enumerate().map(leg).collect(),
    }
}
#[test]
fn common_inputs_are_distinct_versioned_leads_with_exact_witnesses() {
    let t = tx(
        vec![(address(1, false), 600), (address(2, false), 400)],
        vec![(address(3, false), 999)],
    );
    let (c, r) = detect::detect(&t);
    assert!(r.is_none());
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["members"].as_array().unwrap().len(), 2);
    assert_eq!(c[0]["review_status"], "pending");
    assert_eq!(c[0]["evidence"]["inputs"][0]["value_sats"], "600");
    for key in [
        "score_eligible",
        "ownership_claimed",
        "is_exposure_seed",
        "transitive_union",
    ] {
        assert_eq!(c[0][key], false);
    }
    assert!(c[0]["same_entity"].is_null());
    assert_eq!(detect::detect(&t).0, c);
    let mut other = t.clone();
    other.h.block_hash = "d".repeat(64);
    assert_ne!(
        detect::detect(&other).0[0]["cluster_id"],
        c[0]["cluster_id"]
    );
}
#[test]
fn common_repeated_address_is_not_a_cluster() {
    let t = tx(
        vec![(address(1, false), 600), (address(1, false), 400)],
        vec![(address(3, false), 999)],
    );
    assert!(detect::detect(&t).0.is_empty());
}
#[test]
fn possible_collaborative_equal_outputs_suppress_leads() {
    let t = tx(
        vec![(address(1, false), 600), (address(2, false), 400)],
        vec![
            (address(3, false), 400),
            (address(4, false), 400),
            (address(5, false), 199),
        ],
    );
    assert_eq!(
        detect::detect(&t),
        (vec![], Some("possible_collaborative_transaction"))
    );
}
#[test]
fn change_is_only_a_narrow_ambiguous_control_lead() {
    let mut t = tx(
        vec![(address(1, false), 1000)],
        vec![(address(3, false), 600), (address(4, true), 399)],
    );
    let c = detect::detect(&t).0;
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["heuristic"], "change_candidate");
    assert_eq!(c[0]["evidence"]["match"]["change_identified"], false);
    t.outputs[1].subject = address(4, false);
    assert!(detect::detect(&t).0.is_empty());
    t.outputs[0].subject = address(1, false);
    assert!(detect::detect(&t).0.is_empty());
}
#[test]
fn coinbase_script_zero_and_member_limits_do_not_invent_membership() {
    let mut t = tx(
        vec![(address(1, false), 600), (address(2, false), 400)],
        vec![(address(3, false), 999)],
    );
    t.h.coinbase = 1;
    assert!(detect::detect(&t).0.is_empty());
    t.h.coinbase = 0;
    t.inputs[0].value = 0;
    assert_eq!(detect::detect(&t).1, Some("script_or_zero_input_context"));
    t.inputs[0].value = 600;
    t.inputs[0].subject = format!("script:{}", "e".repeat(64));
    assert!(detect::detect(&t).0.is_empty());
    let t = tx(
        (1..=17).map(|i| (address(i, false), 10)).collect(),
        vec![(address(30, false), 169)],
    );
    assert_eq!(detect::detect(&t).1, Some("member_limit"));
}
#[test]
fn review_requires_explicit_version_actor_reason_and_evidence() {
    let id = format!("btc-clrun-{}", "a".repeat(64));
    let mut r = Review {
        network_id: crate::NETWORK_ID.into(),
        run_id: id.clone(),
        cluster_id: format!("btc-cluster-{}", "b".repeat(64)),
        expected_version: id,
        decision: "approved".into(),
        reviewed_by: "fixture-reviewer".into(),
        reason: "lead checked".into(),
        evidence_refs: vec!["case:synthetic".into()],
    };
    assert!(r.validate().is_ok());
    r.decision = "owner".into();
    assert!(r.validate().is_err());
    r.decision = "approved".into();
    r.reason = "".into();
    assert!(r.validate().is_err());
    r.reason = "checked".into();
    r.expected_version = format!("btc-clrun-{}", "c".repeat(64));
    assert!(r.validate().is_err());
    assert!(validate_id("x' OR 1=1").is_err());
    assert_eq!(config()["definition"]["automatic_approval"], false);
}
