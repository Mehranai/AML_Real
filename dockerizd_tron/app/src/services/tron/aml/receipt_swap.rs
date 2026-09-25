use std::collections::BTreeMap;

use num_bigint::BigUint;
use serde_json::{Value, json};

use super::types::AmlEvent;
use crate::{
    models::tron::modules::SemanticAmlEventRow,
    services::tron::transfer_extractor::ExtractedTransfer,
    utils::tron_address::normalize_tron_address,
};

const SWAP_V2: &str = "d78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822";

pub struct ReceiptSwap {
    pub event: AmlEvent,
    pub row: SemanticAmlEventRow,
}

fn unprefixed(value: &str) -> &str {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
}

fn is_swap(log: &Value) -> bool {
    log["topics"][0]
        .as_str()
        .is_some_and(|s| unprefixed(s).eq_ignore_ascii_case(SWAP_V2))
}

fn indexed_address(topic: &Value) -> Option<String> {
    let bytes = hex::decode(unprefixed(topic.as_str()?)).ok()?;
    if bytes.len() != 32 || bytes[..12].iter().any(|b| *b != 0) {
        return None;
    }
    normalize_tron_address(&hex::encode(&bytes[12..]))
}

/// Verify a single V2 swap at a pool against that transaction's canonical transfer legs.
/// Multiple swaps at the same pool are left to a future log-order-aware decoder.
pub fn decode_v2_swaps(
    tx_hash: &str,
    block_number: u64,
    timestamp: u64,
    actor: &str,
    receipt: &Value,
    transfers: &[ExtractedTransfer],
) -> Vec<ReceiptSwap> {
    let Some(logs) = receipt["log"].as_array() else {
        return Vec::new();
    };
    if actor.is_empty() {
        return Vec::new();
    }
    let mut pool_counts = BTreeMap::<String, usize>::new();
    for log in logs.iter().filter(|log| is_swap(log)) {
        if let Some(pool) = log["address"].as_str().and_then(normalize_tron_address) {
            *pool_counts.entry(pool).or_default() += 1;
        }
    }
    logs.iter()
        .enumerate()
        .filter_map(|(index, log)| {
            if !is_swap(log) || log["topics"].as_array()?.len() != 3 {
                return None;
            }
            let pool = normalize_tron_address(log["address"].as_str()?)?;
            if pool_counts.get(&pool) != Some(&1) {
                return None;
            }
            let sender = indexed_address(&log["topics"][1])?;
            let recipient = indexed_address(&log["topics"][2])?;
            let data = hex::decode(unprefixed(log["data"].as_str()?)).ok()?;
            if data.len() != 128 {
                return None;
            }
            let words = data
                .chunks_exact(32)
                .map(BigUint::from_bytes_be)
                .collect::<Vec<_>>();
            let zero = BigUint::default();
            let (amount_in, amount_out) =
                if words[0] > zero && words[3] > zero && words[1] == zero && words[2] == zero {
                    (&words[0], &words[3])
                } else if words[1] > zero && words[2] > zero && words[0] == zero && words[3] == zero
                {
                    (&words[1], &words[2])
                } else {
                    return None;
                };
            let mut incoming = BTreeMap::<&str, BigUint>::new();
            let mut outgoing = BTreeMap::<&str, BigUint>::new();
            let mut ids = Vec::new();
            for transfer in transfers {
                if transfer.from_address == transfer.to_address {
                    continue;
                }
                let amount = BigUint::from_bytes_le(&transfer.amount.to_le_bytes());
                if amount == zero {
                    continue;
                }
                if transfer.to_address == pool {
                    *incoming.entry(&transfer.asset_id).or_default() += &amount;
                    ids.push(transfer.transfer_id.clone());
                }
                if transfer.from_address == pool {
                    *outgoing.entry(&transfer.asset_id).or_default() += &amount;
                    ids.push(transfer.transfer_id.clone());
                }
            }
            if incoming.len() != 1 || outgoing.len() != 1 {
                return None;
            }
            let (asset_in, observed_in) = incoming.into_iter().next()?;
            let (asset_out, observed_out) = outgoing.into_iter().next()?;
            if asset_in == asset_out || &observed_in != amount_in || &observed_out != amount_out {
                return None;
            }
            ids.sort();
            ids.dedup();
            Some(ReceiptSwap {
                event: AmlEvent::Swap {
                    user: actor.into(),
                    token_in: asset_in.into(),
                    token_out: asset_out.into(),
                },
                row: SemanticAmlEventRow {
                    event_id: format!("{tx_hash}:swap:receipt:{index}"),
                    chain: "tron".into(),
                    tx_hash: tx_hash.into(),
                    block_number,
                    timestamp,
                    event_type: "swap".into(),
                    subject_address: actor.into(),
                    protocol: "amm_v2_compatible".into(),
                    asset_in: asset_in.into(),
                    asset_out: asset_out.into(),
                    detector: "receipt_log_and_canonical_transfers".into(),
                    detector_version: "tron_amm_v2_receipt_v1".into(),
                    confidence: 0.85,
                    evidence_json: json!({"evidence_type":"decoded_swap_with_matching_transfers",
                    "log_index":index,"event_signature":SWAP_V2,"emitter":pool,
                    "sender":sender,"recipient":recipient,"subject_role":"transaction_initiator",
                    "amount_in":amount_in.to_string(),"amount_out":amount_out.to_string(),
                    "protocol_identity_verified":false,"protocol_operation_verified":true,
                    "relationship_ids":ids.iter().take(64).collect::<Vec<_>>(),
                    "references_truncated":ids.len()>64})
                    .to_string(),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tron::transfer_extractor::TransferKind;

    fn fixture() -> (Value, Vec<ExtractedTransfer>) {
        let pool_hex = "11".repeat(20);
        let pool = normalize_tron_address(&pool_hex).unwrap();
        let topic = format!("{}{}", "00".repeat(12), "22".repeat(20));
        let receipt = json!({"log":[{"address":pool_hex,"topics":[SWAP_V2,topic,topic],
            "data":format!("{:064x}{:064x}{:064x}{:064x}",10,0,0,9)}]});
        let transfers = vec![
            ExtractedTransfer {
                transfer_id: "in".into(),
                asset_id: "token-a".into(),
                from_address: "wallet".into(),
                to_address: pool.clone(),
                amount: 10u64.into(),
                kind: TransferKind::Trc20,
            },
            ExtractedTransfer {
                transfer_id: "out".into(),
                asset_id: "token-b".into(),
                from_address: pool,
                to_address: "wallet".into(),
                amount: 9u64.into(),
                kind: TransferKind::Trc20,
            },
        ];
        (receipt, transfers)
    }

    #[test]
    fn decodes_only_when_receipt_and_canonical_amounts_agree() {
        let (receipt, mut transfers) = fixture();
        let result = decode_v2_swaps("tx", 1, 1000, "wallet", &receipt, &transfers);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].row.asset_in, "token-a");
        assert_eq!(result[0].row.asset_out, "token-b");
        let evidence: Value = serde_json::from_str(&result[0].row.evidence_json).unwrap();
        assert_eq!(evidence["amount_in"], "10");
        assert_eq!(evidence["relationship_ids"], json!(["in", "out"]));
        transfers[1].amount = 8u64.into();
        assert!(decode_v2_swaps("tx", 1, 1000, "wallet", &receipt, &transfers).is_empty());
    }

    #[test]
    fn refuses_malformed_and_ambiguous_pool_logs() {
        let (mut receipt, transfers) = fixture();
        let duplicate = receipt["log"][0].clone();
        receipt["log"].as_array_mut().unwrap().push(duplicate);
        assert!(decode_v2_swaps("tx", 1, 1000, "wallet", &receipt, &transfers).is_empty());
        receipt["log"].as_array_mut().unwrap().pop();
        receipt["log"][0]["data"] = json!("00");
        assert!(decode_v2_swaps("tx", 1, 1000, "wallet", &receipt, &transfers).is_empty());
    }
}
