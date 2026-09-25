use crate::services::tron::aml::flow_engine::compute_net_flows;
use crate::services::tron::aml::types::SimpleTransfer;

use super::types::{ContractCategory, ProtocolInfo};

pub fn analyze_flows(transfers: &[SimpleTransfer]) -> Option<ProtocolInfo> {
    if transfers.len() < 2 {
        return None;
    }

    let flows = compute_net_flows(transfers);

    for token_map in flows.into_values() {
        let sent_asset_count = token_map
            .values()
            .filter(|delta| delta.sign() == num_bigint::Sign::Minus)
            .count();
        let received_asset_count = token_map
            .values()
            .filter(|delta| delta.sign() == num_bigint::Sign::Plus)
            .count();

        if sent_asset_count >= 1 && received_asset_count >= 1 && token_map.len() >= 2 {
            return Some(ProtocolInfo {
                protocol: "Flow-based DEX candidate".to_string(),
                category: ContractCategory::Dex,
                confidence: 0.55,
                source: "flow_analysis".to_string(),
            });
        }
    }

    None
}
