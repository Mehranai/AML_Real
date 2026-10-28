use super::*;
fn claim() -> Claim {
    serde_json::from_value(json!({"network_id":NETWORK_ID,"claim_id":"fixture","address":"1BoatSLRHtKNngkdXEeobR76b53LETtpyT",
        "entity_id":"synthetic","entity_name":"Synthetic fixture","entity_type":"exchange","address_role":"deposit",
        "confidence":0.9,"risk_level":100,"is_exposure_seed":true,"seed_category":"fixture_only","source_id":"fixture-source",
        "source_reference":"case:fixture","evidence_refs":["case:fixture"],"submitted_by":"fixture-operator"})).unwrap()
}
#[test]
fn input_cannot_import_approval_or_another_network() {
    let mut c = claim();
    c.validate(100).unwrap();
    assert_eq!(c.valid_from_unix_ms, Some(100));
    let mut value = serde_json::to_value(&c).unwrap();
    value["review_status"] = json!("approved");
    assert!(serde_json::from_value::<Claim>(value).is_err());
    c.network_id = "eip155:1".into();
    assert!(c.validate(100).is_err());
}
#[test]
fn invalid_addresses_evidence_amounts_and_intervals_are_refused() {
    for change in [
        json!({"address":"0x1111111111111111111111111111111111111111"}),
        json!({"confidence":1.01}),
        json!({"risk_level":101}),
        json!({"evidence_refs":[]}),
        json!({"evidence_refs":["tx:not-a-hash"]}),
        json!({"is_exposure_seed":true,"seed_category":""}),
        json!({"valid_from_unix_ms":200,"valid_to_unix_ms":100}),
    ] {
        let mut value = serde_json::to_value(claim()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(
            serde_json::from_value::<Claim>(value)
                .unwrap()
                .validate(100)
                .is_err()
        );
    }
}
#[test]
fn review_requires_actor_reason_references_and_exact_revision() {
    let value = json!({"network_id":NETWORK_ID,"claim_id":"fixture","expected_revision":"10","decision":"approved","reviewed_by":"analyst","reason":"fixture review","evidence_refs":["case:fixture"]});
    serde_json::from_value::<Review>(value.clone())
        .unwrap()
        .validate()
        .unwrap();
    for (key, bad) in [
        ("reviewed_by", json!("")),
        ("reason", json!("")),
        ("evidence_refs", json!([])),
        ("decision", json!("pending")),
        ("expected_revision", json!("0")),
    ] {
        let mut v = value.clone();
        v[key] = bad;
        assert!(
            serde_json::from_value::<Review>(v)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    assert!(
        expected(
            Some(&json!({"revision":"9007199254740993"})),
            Some("9007199254740992")
        )
        .is_err()
    );
}
#[test]
fn only_matching_approved_review_active_source_and_current_time_are_active() {
    let c = json!({"review_status":"approved","claim_kind":"label","reviewed_by":"a","evidence_refs":["case:x"],"revision":"9","valid_from_unix_ms":100,"valid_to_unix_ms":200});
    let source = json!({"is_active":1});
    let review = json!({"decision":"approved","subject_revision":"9","reviewed_by":"a"});
    assert!(active(&c, Some(&&source), Some(&&review), 100));
    assert!(!active(&c, Some(&&source), Some(&&review), 99));
    assert!(!active(&c, Some(&&source), Some(&&review), 200));
    for change in [
        json!({"decision":"revoked"}),
        json!({"subject_revision":"10"}),
        json!({"reviewed_by":"other"}),
    ] {
        let mut r = review.clone();
        r.as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(!active(&c, Some(&&source), Some(&&r), 100));
    }
    assert!(!active(&c, None, Some(&&review), 100));
    assert!(!active(
        &c,
        Some(&&json!({"is_active":0})),
        Some(&&review),
        100
    ));
}
