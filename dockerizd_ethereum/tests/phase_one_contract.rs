use std::str::FromStr;

use ethereum_aml::domain::{AddressId, NetworkId};

const CANONICAL_SCHEMA: &str =
    include_str!("../sql/ethereum_migration_20260810_0001_canonical_schema.sql");

#[test]
fn schema_contains_replay_and_evidence_contracts() {
    for required in [
        "ingested_blocks",
        "transactions",
        "evm_logs",
        "address_relationships",
        "transaction_features",
        "semantic_aml_events",
        "ingestion_failures",
        "sync_state",
    ] {
        assert!(
            CANONICAL_SCHEMA.contains(required),
            "canonical schema is missing {required}"
        );
    }
}

#[test]
fn schema_does_not_restore_rejected_legacy_concepts() {
    for rejected in [
        "wallet_info",
        "owner_info",
        "sensivity",
        "address_token_balance",
        "address_token_delta",
        "risk_score",
        "hop_count",
    ] {
        assert!(
            !CANONICAL_SCHEMA.contains(rejected),
            "legacy concept {rejected} must not return"
        );
    }
}

#[test]
fn wallet_identity_is_network_scoped() {
    let address = "0x0000000000000000000000000000000000000001";
    let ethereum = AddressId::parse_evm(NetworkId::ethereum_mainnet(), address).unwrap();
    let another_evm =
        AddressId::parse_evm(NetworkId::from_str("eip155:56").unwrap(), address).unwrap();

    assert_ne!(ethereum.canonical_key(), another_evm.canonical_key());
}
