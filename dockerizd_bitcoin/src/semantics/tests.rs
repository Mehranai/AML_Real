use super::*;
fn tx(ins: &[(u64, &str)], outs: &[(u64, &str)]) -> Tx {
    let build = |values: &[(u64, &str)], input: bool| {
        values
            .iter()
            .enumerate()
            .map(|(i, (value, subject))| Leg {
                index: i as u32,
                subject: (*subject).into(),
                value: *value,
                relationship_id: format!(
                    "{}:{}:{i}",
                    if input { "in" } else { "out" },
                    "a".repeat(64)
                ),
                prev_tx_hash: input.then(|| format!("{:064x}", i + 1)),
                prev_output_index: input.then_some(0),
            })
            .collect::<Vec<_>>()
    };
    let total = ins.iter().map(|i| i.0).sum::<u64>();
    let out = outs.iter().map(|i| i.0).sum::<u64>();
    Tx {
        h: Header {
            block_number: 1,
            block_hash: "b".repeat(64),
            tx_hash: "a".repeat(64),
            transaction_index: 1,
            block_timestamp_unix_ms: 1000,
            commit_id: "fixture".into(),
            revision: 1,
            coinbase: 0,
            input_count: Some(ins.len() as u32),
            output_count: Some(outs.len() as u32),
            fee_raw: (total - out).to_string(),
        },
        inputs: build(ins, true),
        outputs: build(outs, false),
        total,
        fee: total - out,
    }
}
#[test]
fn thresholds_distinguish_maintenance_and_batch_shapes() {
    assert!(detect(&tx(&[(10, "a"); 4], &[(39, "b")])).is_empty());
    let consolidation = detect(&tx(&[(10, "a"); 5], &[(49, "b"), (0, "script:zero")]));
    assert_eq!(
        consolidation[0]["event_type"],
        "bitcoin_consolidation_candidate"
    );
    let fan = detect(&tx(
        &[(100, "a")],
        &[(10, "a"), (10, "b"), (10, "c"), (10, "d"), (10, "e")],
    ));
    assert_eq!(fan[0]["event_type"], "bitcoin_fan_out_candidate");
    assert!(detect(&tx(&[(100, "a")], &[(10, "b"); 5])).is_empty());
}
#[test]
fn equal_outputs_require_distinct_input_subjects_and_output_addresses() {
    let ins = [(100, "a"), (100, "b"), (100, "c")];
    let outs = [(90, "d"), (90, "e"), (90, "f")];
    assert_eq!(
        detect(&tx(&ins, &outs))[0]["event_type"],
        "bitcoin_possible_coinjoin"
    );
    assert!(detect(&tx(&[(100, "a"); 3], &outs)).is_empty());
    assert!(detect(&tx(&ins, &[(90, "d"); 3])).is_empty());
    assert!(
        detect(&tx(
            &ins,
            &[(90, "script:1"), (90, "script:2"), (90, "script:3")]
        ))
        .is_empty()
    );
}
#[test]
fn coinbase_and_large_transactions_never_infer_patterns() {
    let mut value = tx(&[(100, "a")], &[(80, "b"), (19, "c")]);
    assert_eq!(detect(&value).len(), 1);
    value.h.coinbase = 1;
    assert!(detect(&value).is_empty());
    value.h.coinbase = 0;
    value.outputs.extend((0..255).map(|i| Leg {
        index: i + 2,
        value: 0,
        subject: "script:zero".into(),
        relationship_id: format!("zero:{i}"),
        prev_tx_hash: None,
        prev_output_index: None,
    }));
    assert!(detect(&value).is_empty());
}
#[test]
fn peeling_retains_script_context_without_calling_it_change() {
    let a = tx(&[(100, "a")], &[(80, "script:51"), (19, "a")]);
    let e = detect(&a);
    assert_eq!(e[0]["event_type"], "bitcoin_peel_step_candidate");
    assert_eq!(
        e[0]["evidence"]["transactions"][0]["outputs"][0]["subject_type"],
        "script"
    );
    assert_eq!(e[0]["evidence"]["match"]["change_identified"], false);
    assert!(detect(&tx(&[(100, "a")], &[(79, "b"), (20, "c")])).is_empty());
    assert!(detect(&tx(&[(50, "a"), (50, "a")], &[(80, "b"), (19, "c")])).is_empty());
}
#[test]
fn event_identity_includes_occurrence_and_preserves_integer_evidence() {
    let mut a = tx(
        &[(2_100_000_000_000_000, "a")],
        &[(2_000_000_000_000_000, "b"), (99_999_999_999_999, "c")],
    );
    let e = detect(&a);
    assert_eq!(e, detect(&a));
    assert_eq!(
        e[0]["evidence"]["transactions"][0]["input_total_sats"],
        "2100000000000000"
    );
    a.h.block_hash = "c".repeat(64);
    assert_ne!(e[0]["event_id"], detect(&a)[0]["event_id"]);
}
#[test]
fn pattern_only_evidence_does_not_create_risk_or_ownership() {
    let e = detect(&tx(
        &[(100, "a"), (100, "b"), (100, "c")],
        &[(90, "d"), (90, "e"), (90, "f")],
    ));
    for event in &e {
        for field in [
            "score_eligible",
            "ownership_claimed",
            "fund_continuity_claimed",
        ] {
            assert_eq!(event[field], false);
        }
    }
    let input = json!({"network_id":NETWORK_ID,"fingerprint":{},"entities":[],"semantic_events":e,"exposure_paths":[]});
    let risk = aml_risk_policy::assess("bitcoin", &input);
    assert_eq!(risk["risk_level"], "UNKNOWN");
    assert!(risk["risk_score"].is_null());
}
#[test]
fn compiled_config_rejects_version_and_definition_changes() {
    let mut c = config();
    assert!(current(&c));
    c["definition_hash"] = json!("a".repeat(64));
    assert!(!current(&c));
    c = config();
    c["detector_version"] = json!("bitcoin_semantics_v2");
    assert!(!current(&c));
}
#[test]
fn graph_keys_and_native_row_ids_produce_identical_semantic_witnesses() {
    let mut a = tx(&[(100, "a")], &[(80, "b"), (19, "c")]);
    let native = detect(&a);
    for leg in a.inputs.iter_mut().chain(&mut a.outputs) {
        leg.relationship_id = format!(
            "{NETWORK_ID}/{}/{}/{}/{}",
            a.h.block_hash, a.h.commit_id, a.h.revision, leg.relationship_id
        );
    }
    assert_eq!(detect(&a), native);
}
