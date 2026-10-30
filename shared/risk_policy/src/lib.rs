use num_bigint::BigUint;
use serde_json::{Value, json};
use std::collections::HashSet;

pub const POLICY: &str = "aml_evidence_v2_verified_inputs";
pub const LABEL_WEIGHT: f64 = 70.0;
pub const INCOMING_WEIGHT: f64 = 55.0;
pub const OUTGOING_WEIGHT: f64 = 45.0;
pub const INDIRECT_WEIGHT: f64 = 25.0;
pub const MIXER_WEIGHT: f64 = 15.0;
pub const FORWARDING_WEIGHT: f64 = 5.0;
pub const MEDIUM: f64 = 25.0;
pub const HIGH: f64 = 60.0;

pub fn digest(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON serialization"))
    )
}

/// An auditable description of this exact implementation, not editable runtime rules.
pub fn definition() -> Value {
    let rules = [
        (
            "approved_label",
            "APPROVED_RISK_LABEL",
            LABEL_WEIGHT,
            "Approved active explicit risk designation",
            json!({"requires_review":true}),
        ),
        (
            "incoming_direct",
            "ILLICIT_SEED_EXPOSURE",
            INCOMING_WEIGHT,
            "Strongest direct incoming seed exposure",
            json!({"direction":"RECEIVED_FROM_SEED","hops":1}),
        ),
        (
            "outgoing_direct",
            "ILLICIT_SEED_EXPOSURE",
            OUTGOING_WEIGHT,
            "Strongest direct outgoing seed exposure",
            json!({"direction":"SENT_TO_SEED","hops":1}),
        ),
        (
            "indirect",
            "ILLICIT_SEED_EXPOSURE",
            INDIRECT_WEIGHT,
            "Strongest multi-hop seed exposure",
            json!({"minimum_hops":2}),
        ),
        (
            "mixer",
            "MIXER_INTERACTION",
            MIXER_WEIGHT,
            "Decoded mixer evidence with registry provenance",
            json!({"minimum_confidence":0.8}),
        ),
        (
            "forwarding",
            "RAPID_SAME_ASSET_FORWARDING",
            FORWARDING_WEIGHT,
            "Three distinct same-asset timed pairs outside known services",
            json!({"minimum_pairs":3,"maximum_delay_ms":600000,"maximum_amount_difference":0.1,"bitcoin_applicable":false}),
        ),
    ];
    json!({"policy_version":POLICY,"policy_name":"Verified evidence policy","enabled":true,
        "medium_threshold":MEDIUM,"high_threshold":HIGH,"maximum_score":100,
        "probability_claimed":false,"exposure_aggregation":"strongest_only",
        "implementation_sha256":digest(&json!(include_str!("lib.rs").replace("\r\n","\n"))),
        "rules":rules.into_iter().map(|(id,kind,weight,description,settings)|json!({"rule_id":id,"signal_type":kind,"max_contribution":weight,"description":description,"enabled":true,"settings":settings})).collect::<Vec<_>>()})
}

/// Only inputs used by the Bitcoin policy; excludes display graphs and prior results.
pub fn bitcoin_inputs(data: &Value) -> Value {
    let mut inputs = serde_json::Map::new();
    for name in [
        "network_id",
        "address",
        "source_snapshot",
        "intelligence_snapshot",
        "entities",
        "exposure_paths",
        "exposure_snapshot",
        "exposure_coverage",
        "fingerprint",
        "semantic_events",
    ] {
        inputs.insert(name.into(), data[name].clone());
    }
    inputs.insert(
        "graph".into(),
        json!({"truncated":data["graph"]["truncated"]}),
    );
    inputs.insert(
        "data_coverage".into(),
        json!({"history_from_genesis":data["data_coverage"]["history_from_genesis"]}),
    );
    Value::Object(inputs)
}

pub fn bitcoin_binding_matches(frame: &Value, data: &Value) -> bool {
    frame["contract"] == "bitcoin_risk_v1"
        && frame["network_id"] == data["network_id"]
        && frame["address"] == data["address"]
        && frame["policy_version"] == POLICY
        && frame["policy_hash"] == digest(&definition())
        && frame["source_snapshot_id"] == data["source_snapshot"]["snapshot_id"]
        && frame["evidence_hash"] == digest(&bitcoin_inputs(data))
        && frame["risk_engine_hash"] == digest(&assess("bitcoin", data))
}

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
                identity_score =
                    (number(&claim["risk_percent"]) / 100.0).clamp(0.0, 1.0) * LABEL_WEIGHT;
                identity_evidence = claim.clone();
            }
        }
    } else {
        for entity in array(data.get("entities")) {
            if entity["is_exposure_seed"] == true
                && approved_label(entity)
                && (network != "bitcoin" || bitcoin_label(entity, data))
            {
                let score = (number(&entity["risk_level"]) / 100.0).clamp(0.0, 1.0) * LABEL_WEIGHT;
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
        if network == "bitcoin" && bitcoin_exposure(path, data).is_none() {
            limitations.push("Bitcoin possible-only, unverified or unsupported UTXO allocations are shown as context and are not scored.");
            continue;
        }
        if network != "bitcoin" && !has_path_evidence(path, tron) {
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
            INDIRECT_WEIGHT
        } else if incoming {
            INCOMING_WEIGHT
        } else {
            OUTGOING_WEIGHT
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
        signals.push(signal("MIXER_INTERACTION", MIXER_WEIGHT,
            "High-confidence decoded mixer interaction; mixer use alone is not evidence of laundering.", json!(mixer)));
    }

    let known_service = if tron {
        data.pointer("/intelligence/active_exchange")
            .is_some_and(|v| !v.is_null())
    } else {
        array(data.get("entities")).iter().any(|e| {
            approved_label(e)
                && (network != "bitcoin" || bitcoin_label(e, data))
                && matches!(
                    text(&e["entity_type"]).to_ascii_lowercase().as_str(),
                    "exchange" | "dex" | "bridge" | "custodian"
                )
        })
    };
    // UTXO inputs and outputs are distinct legs, not allocated account transfers.
    let pairs = if network == "bitcoin" {
        Vec::new()
    } else {
        pass_through(data, tron)
    };
    if pairs.len() >= 3 && !known_service {
        signals.push(signal("RAPID_SAME_ASSET_FORWARDING", FORWARDING_WEIGHT,
            "At least three distinct receipts closely followed by a similar-sized outgoing transfer of the same asset. This is a timing pattern, not proven fund continuity.", json!(pairs)));
    }
    let truncated = data.pointer("/graph/truncated") == Some(&json!(true))
        || data.pointer("/fingerprint/is_truncated") == Some(&json!(true));
    if truncated {
        limitations.push("Graph or fingerprint was truncated by query safety limits.");
    }
    if matches!(network, "ethereum" | "bsc")
        && data.pointer("/data_coverage/internal_transfer_coverage") != Some(&json!(true))
    {
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
    if network == "bitcoin" {
        limitations.push("Bitcoin transaction connectivity does not allocate particular inputs to outputs or prove fund continuity.");
        limitations.push("Bitcoin ownership, change and CoinJoin classifications are not inferred. Exposure uses conservative value bounds over exact spent outpoints; specific satoshi continuity is not established.");
        if data.pointer("/exposure_coverage/truncated") == Some(&json!(true)) {
            limitations
                .push("Bitcoin exposure reached query limits; partial candidates are not scored.");
        }
        if data.pointer("/data_coverage/history_from_genesis") != Some(&json!(true)) {
            limitations.push("Bitcoin history is partial or empty; observed UTXOs are not a complete wallet balance.");
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
    let available = !signals.is_empty() || (network != "bitcoin" && transfers >= 3);
    let level = if !available {
        "UNKNOWN"
    } else if score >= HIGH {
        "HIGH"
    } else if score >= MEDIUM {
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
        "assessment_scope":"returned_wallet_evidence", "requires_analyst_review":score >= MEDIUM
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
fn bitcoin_label(claim: &Value, data: &Value) -> bool {
    let snapshot = &data["intelligence_snapshot"];
    snapshot["contract"] == "bitcoin_intelligence_v1"
        && snapshot["network_id"] == "bip122:000000000019d6689c085ae165831e93"
        && snapshot["address"] == data["address"]
        && snapshot["snapshot_id"]
            .as_str()
            .is_some_and(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        && claim["network_id"] == snapshot["network_id"]
        && claim["address"] == data["address"]
        && claim["active"] == true
        && !text(&claim["reviewed_by"]).trim().is_empty()
        && claim["reviewed_at_unix_ms"].as_u64().is_some()
}
fn bitcoin_exposure(path: &Value, data: &Value) -> Option<()> {
    const METHOD: &str = "bitcoin_utxo_bounds_v1";
    const SUPPLY: u64 = 2_100_000_000_000_000;
    let frame = &data["exposure_snapshot"];
    if frame["contract"] != "bitcoin_exposure_v1"
        || frame["current"] != true
        || frame["network_id"] != data["network_id"]
        || frame["address"] != data["address"]
        || path["network_id"] != frame["network_id"]
        || path["asset_id"] != "native:BTC"
        || frame["source_snapshot_id"] != data["source_snapshot"]["snapshot_id"]
        || path["run_id"] != frame["run_id"]
        || path["subject_id"] != data["address"]
        || path["attribution_method"] != METHOD
        || path["score_eligible"] != true
        || path["fund_continuity_claimed"] != false
        || path["ownership_claimed"] != false
        || data.pointer("/exposure_coverage/truncated") == Some(&json!(true))
        || !array(frame.get("path_ids")).contains(&path["path_id"])
    {
        return None;
    }
    let seed = &path["seed_claim"];
    if !approved_label(seed)
        || seed["is_exposure_seed"] != true
        || seed["address"] != path["seed_address"]
        || !bitcoin_label(
            seed,
            &json!({"address":path["seed_address"],"intelligence_snapshot":path["seed_snapshot"]}),
        )
    {
        return None;
    }
    let exact = |v: &Value| -> Option<u64> {
        let s = v.as_str()?;
        let n = s.parse::<u64>().ok()?;
        (n <= SUPPLY && n.to_string() == s).then_some(n)
    };
    let hops = path["hops"].as_array()?;
    let steps = path["allocation"]["steps"].as_array()?;
    if !(1..=5).contains(&hops.len())
        || steps.len() != hops.len()
        || path["hop_count"].as_u64() != Some(hops.len() as u64)
        || path["allocation"]["method"] != METHOD
    {
        return None;
    }
    let mut low = 0u64;
    let mut high = 0u64;
    let mut ids = Vec::new();
    let mut txs = Vec::new();
    for (index, (hop, step)) in hops.iter().zip(steps).enumerate() {
        let inputs = hop["inputs"].as_array()?;
        if inputs.is_empty() || inputs.len() > 2000 {
            return None;
        }
        let mut selected = 0u64;
        let mut seen = HashSet::new();
        for input in inputs {
            selected = selected.checked_add(input["value"].as_u64()?)?;
            if !seen.insert((
                text(&input["prev_tx_hash"]),
                input["prev_output_index"].as_u64()?,
            )) {
                return None;
            }
            ids.push(input["relationship_id"].clone());
        }
        if index == 0 {
            if inputs.iter().any(|i| i["subject"] != inputs[0]["subject"]) {
                return None;
            }
            low = selected;
            high = selected;
        } else {
            let previous = &hops[index - 1];
            let input = &inputs[0];
            if inputs.len() != 1
                || input["prev_tx_hash"] != previous["tx_hash"]
                || input["prev_output_index"] != previous["output"]["index"]
                || input["value"] != previous["output"]["value"]
                || input["subject"] != previous["output"]["subject"]
                || (
                    previous["block_number"].as_u64()?,
                    previous["transaction_index"].as_u64()?,
                ) >= (
                    hop["block_number"].as_u64()?,
                    hop["transaction_index"].as_u64()?,
                )
            {
                return None;
            }
        }
        let total = exact(&step["total_input_sats"])?;
        let output = hop["output"]["value"].as_u64()?;
        if selected > total
            || high > selected
            || output > total
            || exact(&step["fee_sats"])? > total - output
            || exact(&step["lower_input_sats"])? != low
            || exact(&step["upper_input_sats"])? != high
            || exact(&step["output_sats"])? != output
        {
            return None;
        }
        low = low.saturating_sub(total - output);
        high = high.min(output);
        if exact(&step["lower_output_sats"])? != low || exact(&step["upper_output_sats"])? != high {
            return None;
        }
        ids.push(hop["output"]["relationship_id"].clone());
        txs.push(hop["tx_hash"].clone());
    }
    if low == 0
        || path["relationship_ids"] != json!(ids)
        || path["tx_hashes"] != json!(txs)
        || exact(&path["amount_raw"])? != low
        || exact(&path["allocation"]["minimum_sats"])? != low
        || exact(&path["allocation"]["maximum_sats"])? != high
    {
        return None;
    }
    let incoming = path["direction"] == "RECEIVED_FROM_SEED";
    if !incoming && path["direction"] != "SENT_TO_SEED" {
        return None;
    }
    let first = hops.first()?;
    let last = hops.last()?;
    let from = text(&first["inputs"][0]["subject"]);
    let to = text(&last["output"]["subject"]);
    if incoming {
        if from != text(&path["seed_address"]) || to != text(&data["address"]) {
            return None;
        }
    } else if from != text(&data["address"]) || to != text(&path["seed_address"]) {
        return None;
    }
    let at = if incoming { first } else { last }["timestamp_unix_ms"].as_u64()?;
    if seed["valid_from_unix_ms"].as_u64()? > at
        || (!seed["valid_to_unix_ms"].is_null() && seed["valid_to_unix_ms"].as_u64()? <= at)
    {
        return None;
    }
    let share = low as f64 / last["output"]["value"].as_u64()?.max(1) as f64;
    let time = path["time_weight"].as_f64()?;
    let confidence = seed["confidence"].as_f64()?;
    let risk = seed["risk_level"].as_u64()?;
    if !(0.0..=1.0).contains(&time)
        || !(0.0..=1.0).contains(&confidence)
        || risk == 0
        || risk > 100
        || (number(&path["amount_share"]) - share).abs() > 1e-12
    {
        return None;
    }
    let maximum =
        confidence * risk as f64 / 100.0 * share * 0.65f64.powi(hops.len() as i32 - 1) * time;
    (number(&path["exposure_score"]) <= maximum + 1e-12).then_some(())
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
    #[test]
    fn bitcoin_utxo_legs_do_not_become_evm_risk_or_a_zero_risk_claim() {
        let data = serde_json::json!({"fingerprint":{"transfer_count":100},"graph":{"edges":[]},
            "data_coverage":{"history_from_genesis":true}});
        let risk = super::assess("bitcoin", &data);
        assert!(risk["risk_score"].is_null());
        assert_eq!(risk["risk_level"], "UNKNOWN");
        assert!(!risk["limitations"].to_string().contains("EVM"));
    }
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
