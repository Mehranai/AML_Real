use num_bigint::BigUint;
use serde_json::{Value, json};
use std::collections::HashSet;

pub const POLICY: &str = "aml_evidence_v2_verified_inputs";

pub fn assess(network: &str, data: &Value) -> Value {
    let mut signals = Vec::new();
    let mut limitations = vec![
        "Evidence score, not a calibrated probability or a legal conclusion.",
        "Only stored and returned evidence is assessed; missing history and labels can hide risk.",
    ];
    let tron = network == "tron";
    let transfers = if tron {
        data.pointer("/fingerprint/flows/total_transfers")
    } else {
        data.pointer("/fingerprint/transfer_count")
    }
    .and_then(Value::as_u64)
    .unwrap_or(0);
    let mut identity_score: f64 = 0.0;
    let mut identity_evidence = Value::Null;
    if tron {
        let active = data
            .pointer("/intelligence/active_entity/source_label_id")
            .and_then(Value::as_str);
        for claim in array(data.pointer("/intelligence/entity_claims")) {
            if active.is_some() && claim["label_id"].as_str() == active && approved_label(claim) {
                identity_score = (number(&claim["risk_percent"]) / 100.0).clamp(0.0, 1.0) * 70.0;
                identity_evidence = claim.clone();
            }
        }
    } else {
        for entity in array(data.get("entities")) {
            if entity["is_exposure_seed"] == true && approved_label(entity) {
                let score = (number(&entity["risk_level"]) / 100.0).clamp(0.0, 1.0) * 70.0;
                if score > identity_score {
                    identity_score = score;
                    identity_evidence = entity.clone();
                }
            }
        }
    }
    if identity_score > 0.0 {
        signals.push(signal(
            "APPROVED_RISK_LABEL",
            identity_score,
            "Current approved entity attribution carries an explicit risk designation.",
            identity_evidence,
        ));
    }

    // Multiple paths to the same seed are correlated evidence, not independent votes.
    let mut exposure_score = 0.0;
    let mut exposure_evidence = Value::Null;
    let paths = if tron {
        data.pointer("/exposure/top_sources")
    } else {
        data.get("exposure_paths")
    };
    for path in array(paths) {
        if !has_path_evidence(path, tron) {
            limitations
                .push("Exposure without transaction references and seed identity is not scored.");
            continue;
        }
        if network == "bsc" && !approved_label(&path["seed_claim"]) {
            limitations.push("Exposure without an approved seed claim is not scored.");
            continue;
        }
        if path["service_mediated"] == true {
            limitations.push(
                "Service-mediated exposure is shown as context and does not increase this score.",
            );
            continue;
        }
        let hops = if tron {
            &path["hop_distance"]
        } else {
            &path["hop_count"]
        }
        .as_u64()
        .unwrap_or(0);
        if hops == 0 {
            continue;
        }
        let direction = if tron {
            text(&path["exposure_type"])
        } else {
            text(&path["direction"])
        };
        // TRON's propagator walks outgoing seed edges, so this means received exposure.
        let incoming = matches!(
            direction,
            "RECEIVED_FROM_SEED" | "INCOMING" | "incoming" | "received"
        ) || (tron && direction == "DIRECTED_FUND_FLOW");
        let outgoing = matches!(direction, "SENT_TO_SEED" | "OUTGOING" | "outgoing" | "sent");
        if !incoming && !outgoing {
            limitations.push("Exposure with unspecified flow direction is not scored.");
            continue;
        }
        let weight = if hops > 1 {
            25.0
        } else if incoming {
            55.0
        } else {
            45.0
        };
        let strength = number(if tron {
            &path["effective_score"]
        } else {
            &path["exposure_score"]
        })
        .clamp(0.0, 1.0);
        let score = strength * weight;
        if score > exposure_score {
            exposure_score = score;
            exposure_evidence = path.clone();
        }
    }
    if exposure_score > 0.0 {
        signals.push(signal("ILLICIT_SEED_EXPOSURE", exposure_score,
            "Strongest directional exposure to an approved risky seed; intermediaries and existing decay reduce weight.", exposure_evidence));
    } else {
        limitations.push(
            "No scored directional exposure was supplied; this does not prove no exposure exists.",
        );
    }

    let events = if tron {
        data.pointer("/activity/recent_semantic_events")
    } else {
        data.get("semantic_events")
    };
    let mixer: Vec<_> = array(events)
        .iter()
        .filter(|event| {
            text(&event["event_type"]).starts_with("mixer_")
                && number(&event["confidence"]) >= 0.8
                && has_mixer_evidence(event)
        })
        .take(10)
        .cloned()
        .collect();
    if !mixer.is_empty() {
        signals.push(signal("MIXER_INTERACTION", 15.0,
            "High-confidence decoded mixer interaction; mixer use alone is not evidence of laundering.", json!(mixer)));
    }

    let known_service = if tron {
        data.pointer("/intelligence/active_exchange")
            .is_some_and(|v| !v.is_null())
    } else {
        array(data.get("entities")).iter().any(|e| {
            approved_label(e)
                && matches!(
                    text(&e["entity_type"]).to_ascii_lowercase().as_str(),
                    "exchange" | "dex" | "bridge" | "custodian"
                )
        })
    };
    let pairs = pass_through(data, tron);
    if pairs.len() >= 3 && !known_service {
        signals.push(signal("RAPID_SAME_ASSET_FORWARDING", 5.0,
            "At least three distinct receipts closely followed by a similar-sized outgoing transfer of the same asset. This is a timing pattern, not proven fund continuity.", json!(pairs)));
    }
    let truncated = data.pointer("/graph/truncated") == Some(&json!(true))
        || data.pointer("/fingerprint/is_truncated") == Some(&json!(true));
    if truncated {
        limitations.push("Graph or fingerprint was truncated by query safety limits.");
    }
    if !tron && data.pointer("/data_coverage/internal_transfer_coverage") != Some(&json!(true)) {
        limitations.push("Internal EVM transfer coverage is incomplete.");
    }
    if transfers < 3 {
        limitations.push("Fewer than three wallet transfers are available.");
    }
    if matches!(network, "bsc" | "ethereum") {
        if data.pointer("/data_coverage/history_from_genesis") != Some(&json!(true)) {
            limitations.push("EVM history is not complete from genesis; missing periods are not evidence of low risk.");
        }
        if data.pointer("/exposure_coverage/truncated") == Some(&json!(true)) {
            limitations
                .push("BSC exposure traversal reached safety limits; additional paths may exist.");
        }
    }
    limitations.sort_unstable();
    limitations.dedup();
    signals.sort_by(|a, b| number(&b["contribution"]).total_cmp(&number(&a["contribution"])));
    let score = signals
        .iter()
        .map(|s| number(&s["contribution"]))
        .sum::<f64>()
        .min(100.0);
    let available = !signals.is_empty() || transfers >= 3;
    let level = if !available {
        "UNKNOWN"
    } else if score >= 60.0 {
        "HIGH"
    } else if score >= 25.0 {
        "MEDIUM"
    } else if score > 0.0 {
        "LOW"
    } else {
        "NO_SIGNALS"
    };
    json!({
        "enabled":true, "status":if available {"available"} else {"insufficient_data"},
        "policy_version":POLICY, "scoring_method":"versioned_evidence_policy",
        "risk_score":if available {Some((score * 10.0).round() / 10.0)} else {None},
        "risk_level":level, "probability_claimed":false,
        "signals":signals, "top_reasons":signals.iter().map(|s| s["summary"].clone()).collect::<Vec<_>>(),
        "limitations":limitations, "observed_transfers":transfers,
        "assessment_scope":"returned_wallet_evidence", "requires_analyst_review":score >= 25.0
    })
}

fn signal(kind: &str, contribution: f64, summary: &str, evidence: Value) -> Value {
    json!({"signal_type":kind,"contribution":contribution,"summary":summary,"evidence":evidence})
}
fn number(value: &Value) -> f64 {
    value.as_f64().filter(|v| v.is_finite()).unwrap_or(0.0)
}
pub fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn array(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn approved_label(claim: &Value) -> bool {
    let refs = array(claim.get("evidence_refs"));
    text(&claim["review_status"]).eq_ignore_ascii_case("approved")
        && !text(&claim["source_id"]).trim().is_empty()
        && !refs.is_empty()
        && refs.iter().all(|r| !text(r).trim().is_empty())
}

fn has_path_evidence(path: &Value, tron: bool) -> bool {
    if tron {
        return !text(&path["source_address"]).is_empty()
            && !text(&path["last_tx_hash"]).is_empty()
            && !text(&path["propagation_run_id"]).is_empty();
    }
    let hops = path["hop_count"].as_u64().unwrap_or(0);
    let txs = array(path.get("tx_hashes"));
    let ids = array(path.get("relationship_ids"));
    let addresses = array(path.get("path_addresses"));
    (1..=10).contains(&hops)
        && !text(&path["seed_address"]).is_empty()
        && txs.len() == hops as usize
        && ids.len() == txs.len()
        && addresses.len() == txs.len() + 1
        && txs
            .iter()
            .chain(ids)
            .chain(addresses)
            .all(|v| !text(v).is_empty())
}

fn has_mixer_evidence(event: &Value) -> bool {
    if text(&event["tx_hash"]).is_empty() || text(&event["protocol_contract"]).is_empty() {
        return false;
    }
    let evidence = if event["evidence_json"].is_object() {
        event["evidence_json"].clone()
    } else {
        serde_json::from_str::<Value>(text(&event["evidence_json"])).unwrap_or(Value::Null)
    };
    // Both EVM decoders include a registry source and a concrete receipt-log reference.
    (!text(&evidence["registry_source"]).is_empty() && evidence["log_index"].as_u64().is_some())
        || (text(&evidence["registry"]["review_status"]).eq_ignore_ascii_case("approved")
            && !text(&evidence["registry"]["source_id"]).is_empty()
            && evidence["signal"]["log_index"].as_u64().is_some())
}

fn raw_amount(value: &Value) -> Option<BigUint> {
    let value = value.as_str()?;
    if value.is_empty() || value.len() > 78 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number = BigUint::parse_bytes(value.as_bytes(), 10)?;
    (number.bits() <= 256).then_some(number)
}

fn pass_through(data: &Value, tron: bool) -> Vec<Value> {
    let address = text(&data["address"]);
    if address.is_empty() {
        return Vec::new();
    }
    let edges = array(data.pointer("/graph/edges"));
    let from = if tron { "from" } else { "from_address" };
    let to = if tron { "to" } else { "to_address" };
    let asset = if tron { "token_address" } else { "asset_id" };
    let time = if tron {
        "timestamp"
    } else {
        "block_timestamp_unix_ms"
    };
    let mut used = HashSet::new();
    let mut pairs = Vec::new();
    for incoming in edges.iter().filter(|e| text(&e[to]) == address).take(500) {
        let Some(amount) = raw_amount(&incoming["amount"]) else {
            continue;
        };
        if amount == BigUint::from(0u8)
            || text(&incoming[asset]).is_empty()
            || text(&incoming["tx_hash"]).is_empty()
            || text(&incoming[from]) == address
            || used.contains(text(&incoming["tx_hash"]))
        {
            continue;
        }
        let Some(start) = incoming[time].as_u64().filter(|v| *v > 0) else {
            continue;
        };
        for outgoing in edges.iter().filter(|e| text(&e[from]) == address).take(500) {
            let Some(end) = outgoing[time].as_u64() else {
                continue;
            };
            if text(&outgoing[to]) == address
                || text(&outgoing["tx_hash"]).is_empty()
                || text(&incoming[asset]) != text(&outgoing[asset])
                || text(&incoming["token_id"]) != text(&outgoing["token_id"])
                || text(&incoming["tx_hash"]) == text(&outgoing["tx_hash"])
                || used.contains(text(&outgoing["tx_hash"]))
                || end <= start
                || end - start > 600_000
            {
                continue;
            }
            let Some(out_amount) = raw_amount(&outgoing["amount"]) else {
                continue;
            };
            let difference = if amount >= out_amount {
                &amount - &out_amount
            } else {
                &out_amount - &amount
            };
            if difference > &amount / 10u8 {
                continue;
            }
            used.insert(text(&incoming["tx_hash"]).to_string());
            used.insert(text(&outgoing["tx_hash"]).to_string());
            pairs.push(json!({"incoming_tx":incoming["tx_hash"],"outgoing_tx":outgoing["tx_hash"],
                "asset":incoming[asset],"incoming_amount":incoming["amount"],"outgoing_amount":outgoing["amount"]}));
            break;
        }
        if pairs.len() >= 10 {
            break;
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    fn label() -> Value {
        json!({"review_status":"approved","source_id":"reviewed-source","evidence_refs":["case:123"],
            "is_exposure_seed":true,"risk_level":100})
    }
    fn exposure_path() -> Value {
        json!({"hop_count":1,"direction":"RECEIVED_FROM_SEED","exposure_score":1,
            "seed_address":"seed","path_addresses":["seed","wallet"],
            "relationship_ids":["edge-1"],"tx_hashes":["tx-1"],"seed_claim":label()})
    }
    fn forwarding_fixture() -> Value {
        let mut edges = Vec::new();
        for index in 0..3 {
            let start = 1_000_000 + index * 1_000_000;
            edges.push(json!({"tx_hash":format!("in-{index}"),"from_address":"sender",
                "to_address":"wallet","asset_id":"asset","amount":"1000","block_timestamp_unix_ms":start}));
            edges.push(json!({"tx_hash":format!("out-{index}"),"from_address":"wallet",
                "to_address":"receiver","asset_id":"asset","amount":"950","block_timestamp_unix_ms":start+1000}));
        }
        json!({"address":"wallet","fingerprint":{"transfer_count":6},"graph":{"edges":edges}})
    }
    #[test]
    fn distinct_similar_amount_forwarding_is_a_low_weight_signal() {
        let data = forwarding_fixture();
        assert_eq!(pass_through(&data, false).len(), 3);
        assert_eq!(assess("ethereum", &data)["risk_score"], 5.0);
    }
    #[test]
    fn forwarding_does_not_mix_assets_or_reuse_transactions() {
        let mut data = forwarding_fixture();
        for edge in data["graph"]["edges"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .skip(1)
            .step_by(2)
        {
            edge["asset_id"] = json!("different-asset");
        }
        assert!(pass_through(&data, false).is_empty());
        let mut data = forwarding_fixture();
        for edge in data["graph"]["edges"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .skip(1)
            .step_by(2)
        {
            edge["tx_hash"] = json!("same-outgoing");
        }
        assert_eq!(pass_through(&data, false).len(), 1);
    }
    #[test]
    fn known_exchange_forwarding_not_scored() {
        let mut data = forwarding_fixture();
        let mut entity = label();
        entity["entity_type"] = json!("exchange");
        entity["is_exposure_seed"] = json!(false);
        data["entities"] = json!([entity]);
        assert_eq!(assess("ethereum", &data)["risk_score"], 0.0);
    }
    #[test]
    fn amount_comparison_does_not_overflow() {
        let mut data = forwarding_fixture();
        for edge in data["graph"]["edges"].as_array_mut().unwrap() {
            edge["amount"] = json!(u128::MAX.to_string());
        }
        assert_eq!(pass_through(&data, false).len(), 3);
    }
    #[test]
    fn empty_is_unknown_not_clean() {
        let risk = assess("tron", &json!({}));
        assert!(risk["risk_score"].is_null());
        assert_eq!(risk["risk_level"], "UNKNOWN");
        assert_eq!(risk["probability_claimed"], false);
    }
    #[test]
    fn pending_or_superseded_label_not_scored() {
        let data = json!({"intelligence":{"active_entity":{"source_label_id":"active"},
            "entity_claims":[{"label_id":"old","review_status":"APPROVED","risk_percent":100,"evidence_refs":["x"]},
            {"label_id":"active","review_status":"PENDING","risk_percent":100,"evidence_refs":["x"]}]}});
        assert!(
            assess("tron", &data)["signals"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn approved_seed_scored_without_transaction_history() {
        let risk = assess("ethereum", &json!({"entities":[label()]}));
        assert_eq!(risk["risk_score"], 70.0);
    }
    #[test]
    fn service_mediated_does_not_raise_score() {
        let mut path = exposure_path();
        path["service_mediated"] = json!(true);
        let data = json!({"exposure_paths":[path]});
        assert!(assess("ethereum", &data)["risk_score"].is_null());
    }
    #[test]
    fn duplicate_paths_not_double_counted() {
        let path = exposure_path();
        assert_eq!(
            assess("ethereum", &json!({"exposure_paths":[path.clone(),path]}))["risk_score"],
            55.0
        );
    }
    #[test]
    fn tron_outgoing_seed_propagation_means_received_exposure() {
        let data = json!({"exposure":{"top_sources":[{"hop_distance":2,
            "source_address":"seed","last_tx_hash":"tx-1","propagation_run_id":"run-1",
            "exposure_type":"DIRECTED_FUND_FLOW","effective_score":0.5}]}});
        assert_eq!(assess("tron", &data)["risk_score"], 12.5);
    }
    #[test]
    fn ordinary_exchange_and_bridge_use_not_scored() {
        let data = json!({"fingerprint":{"transfer_count":10},"entities":[{"entity_type":"exchange"}],
            "semantic_events":[{"event_type":"bridge_transfer","confidence":1}]});
        assert_eq!(assess("ethereum", &data)["risk_score"], 0.0);
    }

    #[test]
    fn full_uint256_forwarding_works_for_each_network() {
        for network in ["ethereum", "bsc", "tron"] {
            let mut data = forwarding_fixture();
            let large = ((BigUint::from(1u8) << 256usize) - BigUint::from(1u8)).to_string();
            for edge in data["graph"]["edges"].as_array_mut().unwrap() {
                edge["amount"] = json!(large);
                if network == "tron" {
                    edge["from"] = edge["from_address"].clone();
                    edge["to"] = edge["to_address"].clone();
                    edge["timestamp"] = edge["block_timestamp_unix_ms"].clone();
                    edge["token_address"] = edge["asset_id"].clone();
                }
            }
            assert_eq!(pass_through(&data, network == "tron").len(), 3);
        }
        assert!(raw_amount(&json!((BigUint::from(1u8) << 256usize).to_string())).is_none());
        assert!(raw_amount(&json!("1e18")).is_none());
        assert!(raw_amount(&json!("-1")).is_none());
    }

    #[test]
    fn unreviewed_and_sourceless_evidence_cannot_make_risk() {
        for network in ["ethereum", "bsc"] {
            for bad in [
                json!({"is_exposure_seed":true,"risk_level":100}),
                json!({"is_exposure_seed":true,"risk_level":100,"review_status":"approved","source_id":"s","evidence_refs":[" "]}),
            ] {
                assert!(assess(network, &json!({"entities":[bad]}))["risk_score"].is_null());
            }
            assert!(assess(network, &json!({"exposure_paths":[{"hop_count":1,"direction":"RECEIVED_FROM_SEED","exposure_score":1}]}))["risk_score"].is_null());
            assert_eq!(
                assess(network, &json!({"exposure_paths":[exposure_path()]}))["risk_score"],
                55.0
            );
        }
    }

    #[test]
    fn mixer_requires_a_real_log_and_registry_provenance() {
        let mut event = json!({"event_type":"mixer_deposit","confidence":1.0});
        assert!(!has_mixer_evidence(&event));
        event["tx_hash"] = json!("tx");
        event["protocol_contract"] = json!("contract");
        event["evidence_json"] =
            json!(json!({"registry_source":"official","log_index":0}).to_string());
        assert!(has_mixer_evidence(&event));
        event["evidence_json"] = json!({"registry":{"source_id":"s","review_status":"approved"},"signal":{"log_index":0}});
        assert!(has_mixer_evidence(&event));
    }
}
