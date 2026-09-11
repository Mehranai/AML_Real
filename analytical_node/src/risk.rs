use serde_json::{Value, json};
use std::collections::HashSet;

pub const POLICY: &str = "aml_evidence_v1";

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
            if active.is_some()
                && claim["label_id"].as_str() == active
                && claim["review_status"] == "APPROVED"
                && !array(Some(&claim["evidence_refs"])).is_empty()
            {
                identity_score = (number(&claim["risk_percent"]) / 100.0).clamp(0.0, 1.0) * 70.0;
                identity_evidence = claim.clone();
            }
        }
    } else {
        for entity in array(data.get("entities")) {
            if entity["is_exposure_seed"] == true {
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
            text(&event["event_type"]).starts_with("mixer_") && number(&event["confidence"]) >= 0.8
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
            matches!(
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
    if network == "bsc" {
        if data.pointer("/data_coverage/history_from_genesis") != Some(&json!(true)) {
            limitations.push("BSC history is not complete from genesis; missing periods are not evidence of low risk.");
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

fn pass_through(data: &Value, tron: bool) -> Vec<Value> {
    let address = text(&data["address"]);
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
        let Ok(amount) = text(&incoming["amount"]).parse::<u128>() else {
            continue;
        };
        if amount == 0
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
                || text(&incoming[asset]) != text(&outgoing[asset])
                || text(&incoming["token_id"]) != text(&outgoing["token_id"])
                || text(&incoming["tx_hash"]) == text(&outgoing["tx_hash"])
                || used.contains(text(&outgoing["tx_hash"]))
                || end <= start
                || end - start > 600_000
            {
                continue;
            }
            let Ok(out_amount) = text(&outgoing["amount"]).parse::<u128>() else {
                continue;
            };
            if amount.abs_diff(out_amount) > amount / 10 {
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
        data["entities"] = json!([{"entity_type":"exchange"}]);
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
        let risk = assess(
            "ethereum",
            &json!({"entities":[{"is_exposure_seed":true,"risk_level":100}]}),
        );
        assert_eq!(risk["risk_score"], 70.0);
    }
    #[test]
    fn service_mediated_does_not_raise_score() {
        let data = json!({"exposure_paths":[{"service_mediated":true,"hop_count":1,"direction":"RECEIVED_FROM_SEED","exposure_score":1}]});
        assert!(assess("ethereum", &data)["risk_score"].is_null());
    }
    #[test]
    fn duplicate_paths_not_double_counted() {
        let path = json!({"hop_count":1,"direction":"RECEIVED_FROM_SEED","exposure_score":1});
        assert_eq!(
            assess("ethereum", &json!({"exposure_paths":[path.clone(),path]}))["risk_score"],
            55.0
        );
    }
    #[test]
    fn tron_outgoing_seed_propagation_means_received_exposure() {
        let data = json!({"exposure":{"top_sources":[{"hop_distance":2,
            "exposure_type":"DIRECTED_FUND_FLOW","effective_score":0.5}]}});
        assert_eq!(assess("tron", &data)["risk_score"], 12.5);
    }
    #[test]
    fn ordinary_exchange_and_bridge_use_not_scored() {
        let data = json!({"fingerprint":{"transfer_count":10},"entities":[{"entity_type":"exchange"}],
            "semantic_events":[{"event_type":"bridge_transfer","confidence":1}]});
        assert_eq!(assess("ethereum", &data)["risk_score"], 0.0);
    }
}
