use crate::{Network, response::Mode};
use aml_warehouse::{RawAmount, receipt_abi, relationships::Subject, values};
use serde_json::{Value, json};
use std::collections::HashSet;

const VERSION: &str = "chain_semantic_evidence_v1";
const MAX_EVENTS: usize = 100;
const MAX_NFT: usize = 100;

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn evidence(event: &Value) -> Value {
    if event["evidence_json"].is_object() {
        return event["evidence_json"].clone();
    }
    let raw = text(&event["evidence_json"]);
    if raw.len() <= 65_536 {
        return serde_json::from_str(raw).unwrap_or(Value::Null);
    }
    Value::Null
}

fn event(network: Network, event: &Value) -> Option<Value> {
    let kind = event["event_type"].as_str()?;
    let tx = event["tx_hash"].as_str()?;
    let id = event["event_id"].as_str().filter(|id| !id.is_empty())?;
    if values::normalize_hash(network, tx).ok()?.as_str() != tx {
        return None;
    }
    if network == Network::Bitcoin {
        if !matches!(
            kind,
            "bitcoin_consolidation_candidate"
                | "bitcoin_fan_out_candidate"
                | "bitcoin_possible_coinjoin"
                | "bitcoin_peel_step_candidate"
                | "bitcoin_peel_chain_candidate"
        ) || event["score_eligible"] != false
            || event["fund_continuity_claimed"] != false
            || event["ownership_claimed"] != false
            || items(&event["evidence_refs"]).is_empty()
        {
            return None;
        }
        return Some(
            json!({"id":id,"category":"obfuscation","event_type":kind,"grade":"behavioral_candidate",
            "tx_hash":tx,"block_number":event["evidence"]["transactions"][0]["block_number"],
            "detector_version":event["detector_version"],"protocol_identity":{"status":"not_applicable"},
            "evidence_refs":items(&event["evidence_refs"]).iter().take(16).collect::<Vec<_>>(),
            "score_eligible":false}),
        );
    }
    let category = if kind.starts_with("mixer_") {
        "mixer"
    } else if kind == "swap"
        || kind.starts_with("liquidity_")
        || kind.starts_with("lending_")
        || kind.starts_with("staking_")
        || kind.starts_with("bridge_")
    {
        "defi"
    } else {
        return None;
    };
    let body = evidence(event);
    let proof = &body["receipt_abi"];
    let emitter = text(&body["emitter"]);
    let log_index = body["log_index"]
        .as_u64()
        .or_else(|| body["signal"]["log_index"].as_u64());
    let registry = &body["registry"];
    let source = if network == Network::Bsc && registry["review_status"] == "approved" {
        text(&registry["source_id"])
    } else if network == Network::Ethereum
        && (body["registered_protocol"] == true || category == "mixer")
    {
        text(&body["registry_source"])
    } else {
        ""
    };
    let emitter_bound = !emitter.is_empty()
        && Subject::address(network, emitter)
            .ok()
            .is_some_and(|address| address.id() == emitter)
        && (network == Network::Tron || event["protocol_contract"] == emitter);
    let abi = receipt_abi::verify(proof).filter(|abi| abi.operation() == kind);
    let verified = abi.is_some() && emitter_bound && log_index.is_some();
    let registry_address = text(&event["protocol_contract"]);
    let registered_address = if emitter_bound {
        Some(emitter)
    } else if network == Network::Bsc
        && emitter.is_empty()
        && Subject::address(network, registry_address)
            .ok()
            .is_some_and(|address| address.id() == registry_address)
    {
        Some(registry_address)
    } else {
        None
    };
    let identity = if let Some(address) = registered_address.filter(|_| !source.is_empty()) {
        json!({"status":"registry_matched","source_id":source,"protocol":event["protocol"],"address":address})
    } else {
        json!({"status":"unattributed"})
    };
    let flow_basis = if verified
        && network == Network::Tron
        && body["amount_basis"] == "matched_canonical_transfers"
        && items(&body["relationship_ids"]).len() >= 2
    {
        "matched_canonical_transfers"
    } else if verified
        && network == Network::Ethereum
        && abi == Some(receipt_abi::Event::BalancerSwap)
        && body["amount_basis"] == "receipt_words"
    {
        "receipt_words"
    } else {
        "not_attested"
    };
    let basis = text(&body["classification_source"]);
    let candidate_basis = if basis.ends_with(":method") {
        "method_selector"
    } else if basis.ends_with(":fund_flow") {
        "fund_flow"
    } else if log_index.is_some() {
        "unverified_log"
    } else {
        "classifier_context"
    };
    let amounts = if flow_basis == "not_attested" {
        Value::Null
    } else {
        let source = if network == Network::Tron {
            &body
        } else {
            event
        };
        let raw = |key: &str| {
            source[key]
                .as_str()
                .and_then(|amount| RawAmount::parse(amount).ok())
                .map(|amount| amount.as_str().to_owned())
        };
        json!({"asset_in":event["asset_in"],"asset_out":event["asset_out"],
            "amount_in_raw":raw("amount_in"),"amount_out_raw":raw("amount_out")})
    };
    Some(json!({"id":id,"category":category,"event_type":kind,
        "grade":if verified{"decoded_receipt"}else{"candidate"},
        "basis":if verified{"validated_abi"}else{candidate_basis},"tx_hash":tx,"block_number":event["block_number"],
        "detector_version":event["detector_version"],"protocol_identity":identity,
        "receipt":if verified {json!({"log_index":log_index,"emitter":emitter,"abi":proof})}else{Value::Null},
        "amount_basis":flow_basis,
        "amounts":amounts,
        "evidence_refs":items(&event["evidence_refs"]).iter().chain(items(&body["relationship_ids"]))
            .take(16).collect::<Vec<_>>() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swap_proof() -> Value {
        let address = format!("{}{}", "0".repeat(24), "1".repeat(40));
        receipt_abi::proof(
            &[
                "d78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822",
                &address,
                &address,
            ],
            &"0".repeat(256),
        )
        .unwrap()
        .1
    }

    #[test]
    fn receipt_layout_registry_identity_and_amount_attestation_are_separate() {
        let pool = format!("0x{}", "2".repeat(40));
        let mut raw = json!({"event_id":"log1","event_type":"swap","tx_hash":format!("0x{}","a".repeat(64)),
            "protocol_contract":pool,"detector_version":"fixture","amount_in":"10",
            "evidence_json":{"emitter":pool,"log_index":8,"receipt_abi":swap_proof(),
                "registered_protocol":false,"amount_basis":"transaction_aggregate"}});
        let observed = event(Network::Ethereum, &raw).unwrap();
        assert_eq!(observed["grade"], "decoded_receipt");
        assert_eq!(observed["protocol_identity"]["status"], "unattributed");
        assert!(observed["amounts"].is_null());
        raw["evidence_json"]["registered_protocol"] = json!(true);
        raw["evidence_json"]["registry_source"] = json!("approved_fixture");
        raw["evidence_json"]["receipt_abi"]["data"] = json!("0x00");
        let candidate = event(Network::Ethereum, &raw).unwrap();
        assert_eq!(candidate["grade"], "candidate");
        assert_eq!(candidate["protocol_identity"]["status"], "registry_matched");
        assert!(candidate["receipt"].is_null());
        raw["evidence_json"] = json!({"registry":{"source_id":"reviewed","review_status":"approved"},
            "classification_source":"reviewed_registry:method"});
        let candidate = event(Network::Bsc, &raw).unwrap();
        assert_eq!(candidate["basis"], "method_selector");
        assert_eq!(candidate["protocol_identity"]["status"], "registry_matched");
    }

    #[test]
    fn tron_flow_witness_and_bitcoin_patterns_stay_in_their_native_scope() {
        let tron = values::tron_wallet(&format!("41{}", "2".repeat(40))).unwrap();
        let tron = event(
            Network::Tron,
            &json!({"event_id":"log1","event_type":"swap",
            "tx_hash":"a".repeat(64),"asset_in":"A","asset_out":"B",
            "evidence_json":{"emitter":tron,"log_index":8,"receipt_abi":swap_proof(),
                "amount_basis":"matched_canonical_transfers","relationship_ids":["in","out"],
                "amount_in":"10","amount_out":"9"}}),
        )
        .unwrap();
        assert_eq!(tron["amounts"]["amount_in_raw"], "10");
        assert_eq!(tron["protocol_identity"]["status"], "unattributed");
        let mut btc = json!({"event_id":"coinjoin","event_type":"bitcoin_possible_coinjoin","tx_hash":"a".repeat(64),
            "score_eligible":false,"ownership_claimed":false,"fund_continuity_claimed":false,"evidence_refs":["tx:fixture"]});
        let candidate = event(Network::Bitcoin, &btc).unwrap();
        assert_eq!(candidate["grade"], "behavioral_candidate");
        btc["score_eligible"] = json!(true);
        assert!(event(Network::Bitcoin, &btc).is_none());
    }

    #[test]
    fn nft_identity_and_amount_are_exact_and_paths_do_not_request_semantics() {
        let asset = format!("erc1155:0x{}/9007199254740993", "2".repeat(40));
        let data = json!({"graph":{"truncated":true,"edges":[{"id":"nft","canonical_asset_id":asset,
            "canonical_token_id":"9007199254740993","amount_raw":"9007199254740995","event_sub_index":9}]}});
        let wallet = project(Network::Bsc, Mode::Wallet, &data);
        assert_eq!(wallet["items"][0]["token_id"], "9007199254740993");
        assert_eq!(wallet["items"][0]["amount_raw"], "9007199254740995");
        assert_eq!(wallet["coverage"]["nft_transfers_truncated"], true);
        assert_eq!(
            project(Network::Bsc, Mode::Paths, &data)["items"],
            json!([])
        );
    }
}

fn nft(edge: &Value) -> Option<Value> {
    let asset = edge["canonical_asset_id"].as_str()?;
    let token = edge["canonical_token_id"].as_str()?;
    let (standard, _) = asset.split_once(':')?;
    if !matches!(standard, "erc721" | "erc1155" | "trc721") {
        return None;
    }
    Some(
        json!({"id":edge["id"],"category":"nft","event_type":"nft_transfer","grade":"observed_transfer",
        "tx_hash":edge["tx_hash"],"block_number":edge["block_number"],"event_index":edge["event_index"],
        "event_sub_index":edge["event_sub_index"],"asset_id":asset,"token_id":token,
        "amount_raw":edge["amount_raw"],"from_address":edge["from_address"],"to_address":edge["to_address"],
        "protocol_identity":{"status":"unattributed"}}),
    )
}

pub(crate) fn project(network: Network, mode: Mode, data: &Value) -> Value {
    let wallet = mode == Mode::Wallet;
    let native = if network == Network::Tron {
        &data["activity"]["recent_semantic_events"]
    } else {
        &data["semantic_events"]
    };
    let mut rows = Vec::new();
    let mut ids = HashSet::new();
    let mut events_truncated = false;
    let mut nft_truncated = false;
    if wallet {
        let mut events = items(native)
            .iter()
            .filter_map(|row| event(network, row))
            .filter(|row| ids.insert(text(&row["id"]).to_owned()));
        rows.extend(events.by_ref().take(MAX_EVENTS));
        events_truncated = events.next().is_some()
            || data["limits"]["events_truncated"] == true
            || data["activity"]["semantic_events_truncated"] == true
            || data["semantic_coverage"]["status"] == "partial"
            || (network == Network::Ethereum && items(native).len() >= 100);
        if network != Network::Bitcoin {
            let mut assets = items(&data["graph"]["edges"]).iter().filter_map(nft);
            rows.extend(assets.by_ref().take(MAX_NFT));
            nft_truncated = assets.next().is_some() || data["graph"]["truncated"] == true;
        }
    }
    json!({"contract":VERSION,"network_id":network.id(),"source_snapshot_id":data["source_snapshot"]["snapshot_id"],
        "scope":if wallet{"returned_wallet_evidence"}else{"not_requested"},
        "coverage":{"protocol_receipts":if !wallet{"not_requested"}else if network==Network::Bitcoin{"not_applicable"}else{"bounded"},
            "mixer_receipts":if !wallet{"not_requested"}else if matches!(network,Network::Ethereum|Network::Bsc){"registered_abi_only"}else{"unsupported"},
            "nft_transfers":if !wallet{"not_requested"}else if network==Network::Bitcoin{"not_applicable"}else{"returned_graph"},
            "behavioral_patterns":if wallet && network==Network::Bitcoin{"bounded_utxo_candidates"}else{"not_requested"},
            "semantic_events_truncated":events_truncated,"nft_transfers_truncated":nft_truncated,
            "exhaustive":false},"items":rows})
}
