use serde_json::json;

use crate::models::tron::modules::SemanticAmlEventRow;
use crate::services::tron::aml::types::AmlEvent;
use crate::services::tron::transfer_extractor::ExtractedTransfer;

pub fn build_semantic_event_rows(
    tx_hash: &str,
    block_number: u64,
    timestamp: u64,
    events: &[AmlEvent],
    protocol: &str,
    detector: &str,
    confidence: f32,
    transfers: &[ExtractedTransfer],
) -> Vec<SemanticAmlEventRow> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let (event_type, subject_address, asset_in, asset_out) = match event {
                AmlEvent::Swap {
                    user,
                    token_in,
                    token_out,
                } => ("swap", user.as_str(), token_in.as_str(), token_out.as_str()),
                AmlEvent::BridgeIn { user, token } => {
                    ("bridge_in", user.as_str(), "", token.as_str())
                }
                AmlEvent::BridgeOut { user, token } => {
                    ("bridge_out", user.as_str(), token.as_str(), "")
                }
                AmlEvent::Mint { user, token } => ("mint", user.as_str(), "", token.as_str()),
                AmlEvent::Burn { user, token } => ("burn", user.as_str(), token.as_str(), ""),
                AmlEvent::LiquidityAdd {
                    user,
                    lp_token,
                    sent_tokens,
                } => (
                    "liquidity_add",
                    user.as_str(),
                    sent_tokens.first().map(String::as_str).unwrap_or_default(),
                    lp_token.as_str(),
                ),
                AmlEvent::LiquidityRemove {
                    user,
                    lp_token,
                    received_tokens,
                } => (
                    "liquidity_remove",
                    user.as_str(),
                    lp_token.as_str(),
                    received_tokens
                        .first()
                        .map(String::as_str)
                        .unwrap_or_default(),
                ),
            };

            // Store the transfer IDs and raw legs, not merely the classifier's name.
            let relevant = transfers.iter().filter(|t|
                (t.from_address == subject_address || t.to_address == subject_address)
                && (t.asset_id == asset_in || t.asset_id == asset_out
                    || matches!(event, AmlEvent::LiquidityAdd { .. } | AmlEvent::LiquidityRemove { .. }))
            ).collect::<Vec<_>>();
            let observed = matches!(event, AmlEvent::Mint { .. } | AmlEvent::Burn { .. });
            let event_confidence = if observed { 1.0 } else { confidence.clamp(0.0, 0.75) };
            SemanticAmlEventRow {
                event_id: format!("{tx_hash}:{event_type}:{index}:{subject_address}"),
                chain: "tron".to_string(),
                tx_hash: tx_hash.to_string(),
                block_number,
                timestamp,
                event_type: event_type.to_string(),
                subject_address: subject_address.to_string(),
                protocol: protocol.to_string(),
                asset_in: asset_in.to_string(),
                asset_out: asset_out.to_string(),
                detector: detector.to_string(),
                detector_version: "tron_semantic_v2_evidence".to_string(),
                confidence: event_confidence,
                evidence_json: json!({
                    "event_index": index,
                    "detector": detector,
                    "protocol": protocol,
                    "classification_confidence": confidence,
                    "evidence_type": if observed { "observed_zero_address_transfer" } else { "flow_pattern_candidate" },
                    "protocol_operation_verified": false,
                    "cross_chain_match_verified": false,
                    "transfer_count": relevant.len(),
                    "transfers_truncated": relevant.len() > 64,
                    "transfers": relevant.iter().take(64).map(|t| json!({
                        "relationship_id": t.transfer_id, "from_address": t.from_address,
                        "to_address": t.to_address, "asset_id": t.asset_id,
                        "amount": t.amount.to_string()
                    })).collect::<Vec<_>>(),
                })
                .to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tron::transfer_extractor::TransferKind;
    use clickhouse::types::UInt256;

    #[test]
    fn event_contains_canonical_raw_evidence_and_does_not_claim_verified_swap() {
        let transfer = ExtractedTransfer {
            transfer_id: "tx:log:1".into(),
            asset_id: "asset-in".into(),
            from_address: "wallet".into(),
            to_address: "pool".into(),
            amount: UInt256::from_le_bytes([255; 32]),
            kind: TransferKind::Trc20,
        };
        let event = AmlEvent::Swap {
            user: "wallet".into(),
            token_in: "asset-in".into(),
            token_out: "asset-out".into(),
        };
        let rows = build_semantic_event_rows(
            "tx",
            10,
            100,
            &[event],
            "protocol",
            "registry",
            0.99,
            &[transfer.clone()],
        );
        let evidence: serde_json::Value = serde_json::from_str(&rows[0].evidence_json).unwrap();
        assert_eq!(
            evidence["transfers"][0]["relationship_id"],
            transfer.transfer_id
        );
        assert_eq!(
            evidence["transfers"][0]["amount"],
            transfer.amount.to_string()
        );
        assert_eq!(evidence["protocol_operation_verified"], false);
        assert!(rows[0].confidence <= 0.75);
    }
}
