use serde_json::json;

use crate::models::tron::modules::SemanticAmlEventRow;
use crate::services::tron::aml::types::AmlEvent;

pub fn build_semantic_event_rows(
    tx_hash: &str,
    block_number: u64,
    timestamp: u64,
    events: &[AmlEvent],
    protocol: &str,
    detector: &str,
    confidence: f32,
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
                detector_version: "tron_semantic_v1".to_string(),
                confidence: confidence.clamp(0.0, 1.0),
                evidence_json: json!({
                    "event_index": index,
                    "detector": detector,
                    "protocol": protocol,
                })
                .to_string(),
            }
        })
        .collect()
}
