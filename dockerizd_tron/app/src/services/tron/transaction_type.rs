use crate::services::tron::aml::types::{AmlEvent, SimpleTransfer};
use crate::services::tron::tron_classifier::types::{ClassificationResult, ContractCategory};

#[derive(Debug, Clone)]
pub struct TransactionSemantics {
    pub transaction_type: String,
    pub transaction_subtype: String,
    pub confidence: f32,
    pub source: String,
    pub protocol: String,
    pub method_id: String,
    pub is_swap: u8,
    pub is_bridge: u8,
    pub is_mint: u8,
    pub is_burn: u8,
    pub is_liquidity_add: u8,
    pub is_liquidity_remove: u8,
}

pub struct TransactionSemanticsInput<'a> {
    pub classification: &'a ClassificationResult,
    pub contract_type: &'a str,
    pub is_contract_call: bool,
    pub transfers: &'a [SimpleTransfer],
    pub swaps: &'a [AmlEvent],
    pub bridges: &'a [AmlEvent],
    pub mint_burns: &'a [AmlEvent],
    pub liquidity_events: &'a [AmlEvent],
}

pub fn classify_transaction_semantics(
    input: TransactionSemanticsInput<'_>,
) -> TransactionSemantics {
    let classification = input.classification;
    let method_operation = classification.operation.as_deref().or_else(|| {
        classification
            .method_id
            .as_deref()
            .and_then(operation_from_method)
    });
    let has_liquidity_add = input
        .liquidity_events
        .iter()
        .any(|event| matches!(event, AmlEvent::LiquidityAdd { .. }))
        || method_operation == Some("liquidity_add");
    let has_liquidity_remove = input
        .liquidity_events
        .iter()
        .any(|event| matches!(event, AmlEvent::LiquidityRemove { .. }))
        || method_operation == Some("liquidity_remove");
    let has_mint = input
        .mint_burns
        .iter()
        .any(|event| matches!(event, AmlEvent::Mint { .. }))
        || method_operation == Some("mint");
    let has_burn = input
        .mint_burns
        .iter()
        .any(|event| matches!(event, AmlEvent::Burn { .. }))
        || method_operation == Some("burn");
    let has_bridge =
        !input.bridges.is_empty() || classification.category == ContractCategory::Bridge;
    let has_swap = (!input.swaps.is_empty() || method_operation == Some("swap"))
        && !has_liquidity_add
        && !has_liquidity_remove;

    let (transaction_type, transaction_subtype, source, confidence) = if has_bridge {
        (
            "bridge".to_string(),
            bridge_subtype(input.bridges).to_string(),
            best_source(classification, "bridge_evidence"),
            confidence_at_least(classification.confidence, 0.88),
        )
    } else if has_liquidity_add {
        (
            "liquidity".to_string(),
            "add".to_string(),
            best_source(classification, "liquidity_flow"),
            confidence_at_least(classification.confidence, 0.82),
        )
    } else if has_liquidity_remove {
        (
            "liquidity".to_string(),
            "remove".to_string(),
            best_source(classification, "liquidity_flow"),
            confidence_at_least(classification.confidence, 0.82),
        )
    } else if has_swap {
        (
            "swap".to_string(),
            "token_swap".to_string(),
            best_source(classification, "swap_flow"),
            confidence_at_least(classification.confidence, 0.78),
        )
    } else if has_mint && has_burn {
        (
            "token_lifecycle".to_string(),
            "mint_and_burn".to_string(),
            "transfer_event".to_string(),
            0.85,
        )
    } else if has_mint {
        (
            "mint".to_string(),
            "token_mint".to_string(),
            "transfer_event".to_string(),
            0.82,
        )
    } else if has_burn {
        (
            "burn".to_string(),
            "token_burn".to_string(),
            "transfer_event".to_string(),
            0.82,
        )
    } else if classification.category == ContractCategory::Lending {
        (
            "lending".to_string(),
            method_operation.unwrap_or("contract_call").to_string(),
            best_source(classification, "known_protocol"),
            confidence_at_least(classification.confidence, 0.70),
        )
    } else if classification.category == ContractCategory::Staking {
        (
            "staking".to_string(),
            method_operation.unwrap_or("contract_call").to_string(),
            best_source(classification, "known_protocol"),
            confidence_at_least(classification.confidence, 0.70),
        )
    } else if classification.category == ContractCategory::Mixer {
        (
            "mixer_interaction".to_string(),
            method_operation
                .unwrap_or("labeled_counterparty")
                .to_string(),
            best_source(classification, "approved_registry"),
            confidence_at_least(classification.confidence, 0.80),
        )
    } else if classification.category == ContractCategory::Scam {
        (
            "scam_interaction".to_string(),
            method_operation
                .unwrap_or("labeled_counterparty")
                .to_string(),
            best_source(classification, "approved_registry"),
            confidence_at_least(classification.confidence, 0.80),
        )
    } else if classification.category == ContractCategory::Nft {
        (
            "nft_interaction".to_string(),
            method_operation.unwrap_or("contract_call").to_string(),
            best_source(classification, "standard_or_registry"),
            confidence_at_least(classification.confidence, 0.75),
        )
    } else if classification.category == ContractCategory::Dex {
        (
            "dex_interaction".to_string(),
            method_operation.unwrap_or("contract_call").to_string(),
            best_source(classification, "known_protocol"),
            confidence_at_least(classification.confidence, 0.70),
        )
    } else if classification.category == ContractCategory::Token {
        (
            "token_interaction".to_string(),
            method_operation.unwrap_or("contract_call").to_string(),
            best_source(classification, "standard_or_registry"),
            confidence_at_least(classification.confidence, 0.70),
        )
    } else if classification.category == ContractCategory::Wallet {
        (
            "wallet_interaction".to_string(),
            method_operation
                .unwrap_or("labeled_counterparty")
                .to_string(),
            best_source(classification, "approved_registry"),
            confidence_at_least(classification.confidence, 0.70),
        )
    } else if input.is_contract_call {
        (
            "contract_call".to_string(),
            input.contract_type.to_string(),
            best_source(classification, "contract_call"),
            confidence_at_least(classification.confidence, 0.50),
        )
    } else if input
        .transfers
        .iter()
        .any(|transfer| transfer.token != "TRX")
    {
        (
            "token_transfer".to_string(),
            "trc20_transfer".to_string(),
            "transfer_event".to_string(),
            0.95,
        )
    } else if input
        .transfers
        .iter()
        .any(|transfer| transfer.token == "TRX")
    {
        (
            "native_transfer".to_string(),
            "trx_transfer".to_string(),
            "transfer_contract".to_string(),
            0.95,
        )
    } else {
        (
            "unknown".to_string(),
            input.contract_type.to_string(),
            classification.detection_source.clone(),
            classification.confidence,
        )
    };

    TransactionSemantics {
        transaction_type,
        transaction_subtype,
        confidence,
        source,
        protocol: classification.protocol.clone(),
        method_id: classification.method_id.clone().unwrap_or_default(),
        is_swap: has_swap as u8,
        is_bridge: has_bridge as u8,
        is_mint: has_mint as u8,
        is_burn: has_burn as u8,
        is_liquidity_add: has_liquidity_add as u8,
        is_liquidity_remove: has_liquidity_remove as u8,
    }
}

fn operation_from_method(method_id: &str) -> Option<&'static str> {
    match method_id {
        "38ed1739" | "7ff36ab5" | "18cbafe5" | "8803dbee" | "fb3bdb41" | "4a25d94a"
        | "5c11d795" => Some("swap"),
        "e8e33700" | "f305d719" => Some("liquidity_add"),
        "baa2abde" | "02751cec" | "2195995c" | "af2979eb" => Some("liquidity_remove"),
        "40c10f19" | "1249c58b" => Some("mint"),
        "42966c68" | "89afcb44" => Some("burn"),
        "c5ebeaec" => Some("borrow"),
        _ => None,
    }
}

fn bridge_subtype(bridges: &[AmlEvent]) -> &'static str {
    if bridges
        .iter()
        .any(|event| matches!(event, AmlEvent::BridgeIn { .. }))
    {
        return "bridge_in";
    }

    if bridges
        .iter()
        .any(|event| matches!(event, AmlEvent::BridgeOut { .. }))
    {
        return "bridge_out";
    }

    "bridge_transfer"
}

fn best_source(classification: &ClassificationResult, fallback: &str) -> String {
    if classification.detection_source == "none" {
        fallback.to_string()
    } else {
        classification.detection_source.clone()
    }
}

fn confidence_at_least(current: f32, minimum: f32) -> f32 {
    current.max(minimum).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classification(category: ContractCategory, method_id: Option<&str>) -> ClassificationResult {
        ClassificationResult {
            protocol: "test".to_string(),
            category,
            confidence: 0.7,
            detection_source: "test".to_string(),
            method_id: method_id.map(str::to_string),
            operation: method_id
                .and_then(operation_from_method)
                .map(str::to_string),
        }
    }

    #[test]
    fn liquidity_add_wins_over_generic_swap_flow() {
        let liquidity = vec![AmlEvent::LiquidityAdd {
            user: "wallet".to_string(),
            lp_token: "LP".to_string(),
            sent_tokens: vec!["TRX".to_string(), "USDT".to_string()],
        }];
        let swaps = vec![AmlEvent::Swap {
            user: "wallet".to_string(),
            token_in: "TRX".to_string(),
            token_out: "LP".to_string(),
        }];

        let classification = classification(ContractCategory::Dex, None);
        let result = classify_transaction_semantics(TransactionSemanticsInput {
            classification: &classification,
            contract_type: "TriggerSmartContract",
            is_contract_call: true,
            transfers: &[],
            swaps: &swaps,
            bridges: &[],
            mint_burns: &[],
            liquidity_events: &liquidity,
        });

        assert_eq!(result.transaction_type, "liquidity");
        assert_eq!(result.transaction_subtype, "add");
        assert_eq!(result.is_swap, 0);
    }

    #[test]
    fn every_registry_category_has_a_persistable_transaction_type() {
        for (category, expected_type) in [
            (ContractCategory::Dex, "dex_interaction"),
            (ContractCategory::Lending, "lending"),
            (ContractCategory::Staking, "staking"),
            (ContractCategory::Mixer, "mixer_interaction"),
            (ContractCategory::Token, "token_interaction"),
            (ContractCategory::Nft, "nft_interaction"),
            (ContractCategory::Scam, "scam_interaction"),
            (ContractCategory::Wallet, "wallet_interaction"),
        ] {
            let classification = classification(category, None);
            let result = classify_transaction_semantics(TransactionSemanticsInput {
                classification: &classification,
                contract_type: "TriggerSmartContract",
                is_contract_call: true,
                transfers: &[],
                swaps: &[],
                bridges: &[],
                mint_burns: &[],
                liquidity_events: &[],
            });

            assert_eq!(result.transaction_type, expected_type);
        }
    }

    #[test]
    fn known_bridge_protocol_becomes_bridge() {
        let classification = classification(ContractCategory::Bridge, None);
        let result = classify_transaction_semantics(TransactionSemanticsInput {
            classification: &classification,
            contract_type: "TriggerSmartContract",
            is_contract_call: true,
            transfers: &[],
            swaps: &[],
            bridges: &[],
            mint_burns: &[],
            liquidity_events: &[],
        });

        assert_eq!(result.transaction_type, "bridge");
        assert_eq!(result.is_bridge, 1);
    }
}
