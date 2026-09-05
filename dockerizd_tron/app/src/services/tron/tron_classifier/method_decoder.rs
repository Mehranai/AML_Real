use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::types::ContractCategory;

#[derive(Debug, Clone)]
pub struct OperationHint {
    pub protocol: &'static str,
    pub category: ContractCategory,
    pub operation: &'static str,
    pub confidence: f32,
}

pub static METHOD_SIGNATURES: Lazy<HashMap<&'static str, OperationHint>> = Lazy::new(|| {
    let mut methods = HashMap::new();

    for selector in [
        "38ed1739", "7ff36ab5", "18cbafe5", "8803dbee", "fb3bdb41", "4a25d94a", "5c11d795",
        "414bf389", "c04b8d59", "db3e2198", "f28c0498",
    ] {
        insert(
            &mut methods,
            selector,
            "Generic DEX",
            ContractCategory::Dex,
            "swap",
            0.84,
        );
    }

    for selector in ["e8e33700", "f305d719"] {
        insert(
            &mut methods,
            selector,
            "Generic DEX",
            ContractCategory::Dex,
            "liquidity_add",
            0.80,
        );
    }

    for selector in ["baa2abde", "02751cec", "2195995c", "af2979eb"] {
        insert(
            &mut methods,
            selector,
            "Generic DEX",
            ContractCategory::Dex,
            "liquidity_remove",
            0.80,
        );
    }

    for (selector, operation) in [
        ("a0712d68", "supply"),
        ("c5ebeaec", "borrow"),
        ("db006a75", "redeem"),
        ("852a12e3", "redeem_underlying"),
        ("0e752702", "repay"),
        ("4e4d9fea", "repay"),
        ("f5e3c462", "liquidate"),
        ("c2998238", "enter_market"),
        ("ede4edd0", "exit_market"),
    ] {
        insert(
            &mut methods,
            selector,
            "Generic Lending",
            ContractCategory::Lending,
            operation,
            0.86,
        );
    }

    for (selector, operation) in [("a694fc3a", "stake"), ("2e17de78", "unstake")] {
        insert(
            &mut methods,
            selector,
            "Generic Staking",
            ContractCategory::Staking,
            operation,
            0.78,
        );
    }

    for (selector, operation) in [
        ("a9059cbb", "transfer"),
        ("40c10f19", "mint"),
        ("1249c58b", "mint"),
        ("42966c68", "burn"),
        ("79cc6790", "burn_from"),
    ] {
        insert(
            &mut methods,
            selector,
            "Generic TRC20",
            ContractCategory::Token,
            operation,
            0.72,
        );
    }

    for (selector, operation) in [
        ("42842e0e", "nft_transfer"),
        ("b88d4fde", "nft_transfer"),
        ("f242432a", "nft_transfer"),
        ("2eb2c2d6", "nft_batch_transfer"),
        ("a22cb465", "nft_approval"),
        ("6352211e", "nft_owner_query"),
        ("c87b56dd", "nft_metadata_query"),
    ] {
        insert(
            &mut methods,
            selector,
            "Generic NFT",
            ContractCategory::Nft,
            operation,
            0.82,
        );
    }

    methods
});

pub fn extract_method_id(method_data: &str) -> Option<String> {
    let trimmed = method_data.trim();
    let normalized = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    if normalized.len() < 8
        || !normalized[..8]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return None;
    }

    Some(normalized[..8].to_ascii_lowercase())
}

pub fn detect_method(method_data: &str) -> Option<(String, OperationHint)> {
    let method_id = extract_method_id(method_data)?;
    METHOD_SIGNATURES
        .get(method_id.as_str())
        .map(|info| (method_id, info.clone()))
}

pub fn detect_contract_type(contract_type: &str) -> Option<OperationHint> {
    let (category, operation, confidence) = match contract_type.trim() {
        "FreezeBalanceContract" | "FreezeBalanceV2Contract" => {
            (ContractCategory::Staking, "stake", 0.99)
        }
        "UnfreezeBalanceContract" | "UnfreezeBalanceV2Contract" => {
            (ContractCategory::Staking, "unstake", 0.99)
        }
        "WithdrawExpireUnfreezeContract" | "WithdrawBalanceContract" => {
            (ContractCategory::Staking, "withdraw", 0.99)
        }
        "DelegateResourceContract" => (ContractCategory::Staking, "delegate_resource", 0.99),
        "UnDelegateResourceContract" => (ContractCategory::Staking, "undelegate_resource", 0.99),
        "VoteWitnessContract" => (ContractCategory::Staking, "vote", 0.99),
        "TransferAssetContract" => (ContractCategory::Token, "transfer", 0.99),
        "AssetIssueContract" => (ContractCategory::Token, "issue", 0.99),
        "ParticipateAssetIssueContract" => (ContractCategory::Token, "participate_issue", 0.99),
        "UpdateAssetContract" => (ContractCategory::Token, "update", 0.99),
        "UnfreezeAssetContract" => (ContractCategory::Token, "unfreeze", 0.99),
        "AccountCreateContract" => (ContractCategory::Wallet, "account_create", 0.99),
        "AccountUpdateContract" => (ContractCategory::Wallet, "account_update", 0.99),
        "AccountPermissionUpdateContract" => (ContractCategory::Wallet, "permission_update", 0.99),
        "SetAccountIdContract" => (ContractCategory::Wallet, "account_id_update", 0.99),
        _ => return None,
    };

    Some(OperationHint {
        protocol: "TRON Native",
        category,
        operation,
        confidence,
    })
}

fn insert(
    methods: &mut HashMap<&'static str, OperationHint>,
    selector: &'static str,
    protocol: &'static str,
    category: ContractCategory,
    operation: &'static str,
    confidence: f32,
) {
    methods.insert(
        selector,
        OperationHint {
            protocol,
            category,
            operation,
            confidence,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_decoder_accepts_prefixed_uppercase_call_data() {
        let (method_id, hint) =
            detect_method("0x38ED17390000").expect("known DEX method must decode");

        assert_eq!(method_id, "38ed1739");
        assert_eq!(hint.category, ContractCategory::Dex);
        assert_eq!(hint.operation, "swap");
    }

    #[test]
    fn native_stake_two_contracts_are_staking() {
        let hint = detect_contract_type("DelegateResourceContract")
            .expect("native delegation must be classified");

        assert_eq!(hint.category, ContractCategory::Staking);
        assert_eq!(hint.operation, "delegate_resource");
    }

    #[test]
    fn shared_transfer_from_selector_is_not_guessed_as_token_or_nft() {
        assert!(detect_method("23b872dd").is_none());
    }
}
