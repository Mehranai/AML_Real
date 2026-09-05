use crate::services::tron::aml::types::SimpleTransfer;

use super::confidence::combine_confidence;
use super::flow_analyzer::analyze_flows;
use super::method_decoder::{detect_contract_type, detect_method, extract_method_id};
use super::protocol_detector::detect_protocol;
use super::registry::ProtocolRegistry;
use super::types::{ClassificationInput, ClassificationResult, ContractCategory, ProtocolInfo};

#[derive(Debug, Clone)]
struct Candidate {
    protocol: String,
    category: ContractCategory,
    confidence: f32,
    source: String,
    operation: Option<String>,
    priority: u8,
}

pub fn classify(
    registry: &ProtocolRegistry,
    input: &ClassificationInput,
    transfers: &[SimpleTransfer],
) -> ClassificationResult {
    let method_id = input.method_data.as_deref().and_then(extract_method_id);
    let mut candidates = Vec::new();

    let registry_match = [&input.contract_address, &input.target_address]
        .into_iter()
        .filter(|address| !address.trim().is_empty())
        .find_map(|address| detect_protocol(registry, address));
    if let Some(protocol) = registry_match {
        candidates.push(registry_candidate(protocol));
    }

    if let Some(native) = detect_contract_type(&input.contract_type) {
        candidates.push(Candidate {
            protocol: native.protocol.to_string(),
            category: native.category,
            confidence: native.confidence,
            source: "native_contract_type".to_string(),
            operation: Some(native.operation.to_string()),
            priority: 90,
        });
    }

    if let Some(data) = input.method_data.as_deref()
        && let Some((_selector, method)) = detect_method(data)
    {
        candidates.push(Candidate {
            protocol: method.protocol.to_string(),
            category: method.category,
            confidence: method.confidence,
            source: "method_signature".to_string(),
            operation: Some(method.operation.to_string()),
            priority: 80,
        });
    }

    if let Some(flow) = analyze_flows(transfers) {
        candidates.push(Candidate {
            protocol: flow.protocol,
            category: flow.category,
            confidence: flow.confidence,
            source: flow.source,
            operation: Some("swap".to_string()),
            priority: 70,
        });
    }

    if input.has_trc721_transfer {
        candidates.push(Candidate {
            protocol: "TRC721/TRC1155".to_string(),
            category: ContractCategory::Nft,
            confidence: 0.97,
            source: "standard_transfer_event".to_string(),
            operation: Some("nft_transfer".to_string()),
            priority: 65,
        });
    }

    if input.has_trc20_transfer {
        candidates.push(Candidate {
            protocol: "TRC20".to_string(),
            category: ContractCategory::Token,
            confidence: 0.96,
            source: "standard_transfer_event".to_string(),
            operation: Some("transfer".to_string()),
            priority: 60,
        });
    }

    let Some((winner_index, winner)) =
        candidates
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.priority
                    .cmp(&right.priority)
                    .then_with(|| left.confidence.total_cmp(&right.confidence))
            })
    else {
        return ClassificationResult {
            protocol: "Unknown".to_string(),
            category: ContractCategory::Unknown,
            confidence: 0.0,
            detection_source: "none".to_string(),
            method_id,
            operation: None,
        };
    };

    let supporting_confidence = candidates
        .iter()
        .enumerate()
        .filter(|(index, candidate)| {
            *index != winner_index && candidate.category == winner.category
        })
        .map(|(_, candidate)| candidate.confidence);
    let confidence = combine_confidence(winner.confidence, supporting_confidence);

    let mut sources = vec![winner.source.clone()];
    for candidate in &candidates {
        let source = if candidate.category == winner.category {
            candidate.source.clone()
        } else {
            format!("conflict_{}", candidate.source)
        };
        if !sources.contains(&source) {
            sources.push(source);
        }
    }

    let operation = winner.operation.clone().or_else(|| {
        candidates
            .iter()
            .find(|candidate| candidate.source == "method_signature")
            .and_then(|candidate| candidate.operation.clone())
    });

    ClassificationResult {
        protocol: winner.protocol.clone(),
        category: winner.category.clone(),
        confidence,
        detection_source: sources.join("+"),
        method_id,
        operation,
    }
}

fn registry_candidate(protocol: ProtocolInfo) -> Candidate {
    Candidate {
        protocol: protocol.protocol,
        category: protocol.category,
        confidence: protocol.confidence,
        source: protocol.source,
        operation: None,
        priority: 100,
    }
}

#[cfg(test)]
mod tests {
    use clickhouse::types::UInt256;

    use super::*;

    fn input(address: &str, contract_type: &str, method_data: Option<&str>) -> ClassificationInput {
        ClassificationInput {
            contract_address: address.to_string(),
            target_address: address.to_string(),
            contract_type: contract_type.to_string(),
            method_data: method_data.map(str::to_string),
            has_trc20_transfer: false,
            has_trc721_transfer: false,
        }
    }

    fn registry(
        address: &str,
        protocol: &str,
        category: ContractCategory,
        source: &str,
    ) -> ProtocolRegistry {
        ProtocolRegistry::from_entries([(
            address.to_string(),
            ProtocolInfo {
                protocol: protocol.to_string(),
                category,
                confidence: 0.98,
                source: source.to_string(),
            },
        )])
    }

    #[test]
    fn approved_sensitive_label_wins_over_conflicting_selector() {
        let registry = registry(
            "TScamAddress",
            "Reviewed phishing service",
            ContractCategory::Scam,
            "approved_entity_registry",
        );
        let result = classify(
            &registry,
            &input("TScamAddress", "TriggerSmartContract", Some("38ed1739")),
            &[],
        );

        assert_eq!(result.category, ContractCategory::Scam);
        assert_eq!(result.protocol, "Reviewed phishing service");
        assert_eq!(result.operation.as_deref(), Some("swap"));
        assert_eq!(
            result.detection_source,
            "approved_entity_registry+conflict_method_signature"
        );
    }

    #[test]
    fn nft_transfer_event_resolves_ambiguous_transfer_from() {
        let registry = ProtocolRegistry::from_entries([]);
        let mut classification_input =
            input("TNftAddress", "TriggerSmartContract", Some("23b872dd"));
        classification_input.has_trc721_transfer = true;

        let result = classify(&registry, &classification_input, &[]);

        assert_eq!(result.category, ContractCategory::Nft);
        assert_eq!(result.method_id.as_deref(), Some("23b872dd"));
        assert_eq!(result.operation.as_deref(), Some("nft_transfer"));
    }

    #[test]
    fn registry_and_method_evidence_are_combined() {
        let registry = registry(
            "TDexAddress",
            "Reviewed DEX",
            ContractCategory::Dex,
            "approved_entity_registry",
        );
        let result = classify(
            &registry,
            &input("TDexAddress", "TriggerSmartContract", Some("38ed1739")),
            &[],
        );

        assert_eq!(result.category, ContractCategory::Dex);
        assert_eq!(result.operation.as_deref(), Some("swap"));
        assert!(result.confidence > 0.98);
        assert_eq!(
            result.detection_source,
            "approved_entity_registry+method_signature"
        );
    }

    #[test]
    fn native_resource_delegation_is_staking_without_a_contract_address() {
        let registry = ProtocolRegistry::from_entries([]);
        let result = classify(&registry, &input("", "DelegateResourceContract", None), &[]);

        assert_eq!(result.category, ContractCategory::Staking);
        assert_eq!(result.operation.as_deref(), Some("delegate_resource"));
    }

    #[test]
    fn multi_asset_bidirectional_flow_is_only_a_low_confidence_fallback() {
        let registry = ProtocolRegistry::from_entries([]);
        let transfers = vec![
            SimpleTransfer {
                token: "TRX".to_string(),
                from: "wallet".to_string(),
                to: "pool".to_string(),
                amount: 10,
                raw_amount: UInt256::from(10_u64),
            },
            SimpleTransfer {
                token: "USDT".to_string(),
                from: "pool".to_string(),
                to: "wallet".to_string(),
                amount: 5,
                raw_amount: UInt256::from(5_u64),
            },
        ];

        let result = classify(
            &registry,
            &input("", "TriggerSmartContract", None),
            &transfers,
        );

        assert_eq!(result.category, ContractCategory::Dex);
        assert_eq!(result.confidence, 0.55);
        assert_eq!(result.detection_source, "flow_analysis");
    }
}
