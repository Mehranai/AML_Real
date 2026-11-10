use super::*;

#[path = "risk_corpus_tests.rs"]
mod corpus;

const NETWORKS: [&str; 4] = ["bitcoin", "ethereum", "bsc", "tron"];
const BITCOIN: &str = "bip122:000000000019d6689c085ae165831e93";

fn claim(status: &str) -> Value {
    json!({"label_id":"fixture-label","review_status":status,"source_id":"fixture-source",
        "evidence_refs":["fixture:case"],"risk_level":100,"risk_percent":100,"is_exposure_seed":true,"active":true,
        "reviewed_by":"fixture-reviewer","reviewed_at_unix_ms":1})
}

fn labelled_wallet(network: &str, review: &str) -> Value {
    let mut claim = claim(review);
    match network {
        "bitcoin" => {
            claim["network_id"] = json!(BITCOIN);
            claim["address"] = json!("wallet");
            json!({"network_id":BITCOIN,"address":"wallet","entities":[claim],
            "intelligence_snapshot":{"contract":"bitcoin_intelligence_v1","network_id":BITCOIN,
                "address":"wallet","snapshot_id":"a".repeat(64)},"fingerprint":{"transfer_count":3}})
        }
        "tron" => json!({"intelligence":{"active_entity":{"source_label_id":"fixture-label"},
            "entity_claims":[claim]},"fingerprint":{"flows":{"total_transfers":3}}}),
        _ => json!({"entities":[claim],"fingerprint":{"transfer_count":3}}),
    }
}

fn assess_fixture(network: &str, mut wallet: Value, source: &str) -> Value {
    let assessment = risk::assess(network, &wallet);
    risk::attach_assessment(&mut wallet, assessment);
    risk::record_source_validity(&mut wallet, network, source, 100);
    wallet
}

#[test]
fn labelled_fixtures_count_errors_and_abstentions_separately() {
    let mut outcomes = corpus::Counts::default();
    let mut check = |name: &str, network, wallet, positive, source, expected: Option<f64>| {
        let response = assess_fixture(network, wallet, source);
        let assessed = &response["risk_engine"];
        let score = assessed["risk_score"].as_f64();
        assert_eq!(score, expected, "{network}/{name}");
        assert_eq!(
            response["source_validity"]["current_risk_usable"],
            score.is_some()
        );
        assert_eq!(assessed["probability_claimed"], false);
        if let Some(score) = score {
            let signals = assessed["signals"].as_array().unwrap();
            let total: f64 = signals
                .iter()
                .map(|signal| {
                    assert!(!signal["summary"].as_str().unwrap().is_empty());
                    assert!(!signal["evidence"].is_null());
                    signal["contribution"].as_f64().unwrap()
                })
                .sum();
            assert_eq!((total.min(100.0) * 10.0).round() / 10.0, score);
            assert_eq!(assessed["requires_analyst_review"], score >= 25.0);
        } else {
            assert_eq!(assessed["risk_level"], "UNKNOWN");
        }
        outcomes.record(
            positive,
            score.map(|value| value >= aml_risk_policy::MEDIUM),
        );
    };

    for network in NETWORKS {
        let approved = labelled_wallet(network, "approved");
        check(
            "approved",
            network,
            approved.clone(),
            true,
            "current",
            Some(70.0),
        );
        check("stale", network, approved, true, "stale", None);
        check(
            "insufficient history",
            network,
            json!({}),
            true,
            "current",
            None,
        );
        for state in ["pending", "revoked"] {
            check(
                state,
                network,
                labelled_wallet(network, state),
                false,
                "current",
                (network != "bitcoin").then_some(0.0),
            );
        }
    }

    let path = json!({"hop_count":1,"direction":"RECEIVED_FROM_SEED","exposure_score":1,
        "seed_address":"seed","path_addresses":["seed","wallet"],
        "relationship_ids":["edge"],"tx_hashes":["tx"],"seed_claim":claim("approved")});
    check(
        "verified repeated path",
        "bsc",
        json!({"exposure_paths":[path.clone(),path.clone()]}),
        true,
        "current",
        Some(55.0),
    );
    let mut service = path.clone();
    service["service_mediated"] = json!(true);
    check(
        "service-mediated",
        "bsc",
        json!({"exposure_paths":[service],"fingerprint":{"transfer_count":3}}),
        false,
        "current",
        Some(0.0),
    );
    let mut unwitnessed = path;
    unwitnessed["tx_hashes"] = json!([]);
    check(
        "missing witness",
        "ethereum",
        json!({"exposure_paths":[unwitnessed],"fingerprint":{"transfer_count":3}}),
        false,
        "current",
        Some(0.0),
    );

    // Independent synthetic ground truth can disagree with the operational review decision.
    check(
        "wrong approved attribution",
        "ethereum",
        labelled_wallet("ethereum", "approved"),
        false,
        "current",
        Some(70.0),
    );
    check(
        "positive below threshold",
        "tron",
        json!({"exposure":{"top_sources":[{
        "hop_distance":2,"source_address":"seed","last_tx_hash":"tx",
        "propagation_run_id":"run","exposure_type":"DIRECTED_FUND_FLOW","effective_score":0.5}]}}),
        true,
        "current",
        Some(12.5),
    );

    let summary = outcomes.summary();
    for (key, expected) in [
        ("true_positive", 5),
        ("true_negative", 8),
        ("false_positive", 1),
        ("false_negative", 1),
        ("unknown_positive", 8),
        ("unknown_negative", 2),
    ] {
        assert_eq!(summary[key], expected, "{key}");
    }
}

#[test]
fn fresh_source_cannot_make_an_old_or_changed_assessment_current() {
    for network in NETWORKS {
        let saved = assess_fixture(network, labelled_wallet(network, "approved"), "current");
        assert_eq!(saved["source_validity"]["current_risk_usable"], true);
        let mut legacy = saved.clone();
        legacy.as_object_mut().unwrap().remove("risk_policy_hash");
        let mut old_hash = saved.clone();
        old_hash["risk_policy_hash"] = json!("b".repeat(64));
        let mut changed_version = saved.clone();
        changed_version["risk_engine"]["policy_version"] = json!("old-policy");
        let mut changed_score = saved.clone();
        changed_score["risk_engine"]["risk_score"] = json!(91);
        let mut changed_evidence = labelled_wallet(network, "revoked");
        changed_evidence["risk_policy_hash"] = saved["risk_policy_hash"].clone();
        changed_evidence["risk_engine"] = saved["risk_engine"].clone();

        for mut record in [
            legacy,
            old_hash,
            changed_version,
            changed_score,
            changed_evidence,
        ] {
            let original = record["risk_engine"].clone();
            risk::record_source_validity(&mut record, network, "current", 101);
            assert_eq!(record["source_validity"]["status"], "current");
            assert_eq!(record["source_validity"]["current_risk_usable"], false);
            assert_eq!(record["risk_engine"]["status"], "policy_unverified");
            assert!(record["risk_engine"]["risk_score"].is_null());
            assert_eq!(record["historical_risk_engine"], original);
        }
        let mut unavailable = saved.clone();
        risk::record_source_validity(&mut unavailable, network, "unverified", 101);
        assert_eq!(
            unavailable["risk_engine"]["status"],
            "source_snapshot_invalid"
        );
        risk::record_source_validity(&mut unavailable, network, "current", 102);
        assert_eq!(unavailable["source_validity"]["current_risk_usable"], false);
        assert!(unavailable["risk_engine"]["risk_score"].is_null());
        assert_eq!(unavailable["historical_risk_engine"], saved["risk_engine"]);
    }
}
