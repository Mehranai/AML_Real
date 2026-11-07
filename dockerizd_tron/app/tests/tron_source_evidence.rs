use aml_warehouse::{Network, publication::Block};
use anyhow::Result;
use arz_axum_for_services::services::tron::source_evidence::VerifiedTransaction;
use serde_json::json;
#[path = "support/source_evidence.rs"]
mod source;

fn block() -> Block {
    Block {
        network_id: Network::Tron.id().into(),
        block_number: source::HEIGHT,
        block_hash: "a".repeat(64),
        block_timestamp_unix_ms: source::TIME,
        commit_id: "source-check".into(),
        revision: 3,
        parent_hash: "b".repeat(64),
        canonical: 1,
        ingestion_status: "complete".into(),
        transaction_count: 1,
        receipt_coverage: Some(1),
        trace_coverage: None,
        prevout_coverage: None,
        source_id: "synthetic-rpc".into(),
        created_at_unix_ms: 1,
    }
}

#[test]
fn independent_official_unsigned_byte_vector_matches_json() -> Result<()> {
    // https://developers.tron.network/reference/buildtransaction
    // An unsigned documentation vector, not a confirmed chain transaction.
    let tx: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/tron/unsigned_trigger.json"))?;
    let receipt = json!({"id":tx["txID"],"blockNumber":source::HEIGHT});
    let verified = VerifiedTransaction::verify(&tx, &receipt, source::HEIGHT, source::TIME)?;
    assert_eq!(
        verified.tx_hash(),
        "1b8659471c3f359d9502ef2aca9a2bca73f8e80c0b47bbdc7ee2a85ce58ae8a8"
    );
    let facts = verified.facts(&block(), 0, 1)?;
    assert_eq!(
        facts["aml_tron_contracts"][0]["parameter_fields_verified"],
        1
    );
    assert!(facts["aml_tron_resources"][0]["fee_raw"].is_null());
    Ok(())
}
#[test]
fn genuine_bytes_match_json_and_keep_contract_log_internal_resource_positions() -> Result<()> {
    let tx = source::multi_transaction();
    let receipt = source::receipt(&tx);
    let verified = VerifiedTransaction::verify(&tx, &receipt, source::HEIGHT, source::TIME)?;
    let facts = verified.facts(&block(), 0, 99)?;
    let contracts = &facts["aml_tron_contracts"];
    assert_eq!(contracts.len(), 3);
    for (index, contract) in contracts.iter().enumerate() {
        assert_eq!(contract["contract_index"], index);
        assert_eq!(contract["transaction_index"], 0);
        assert_eq!(contract["raw_data_hash_verified"], 1);
        assert_eq!(contract["parameter_fields_verified"], 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(contract["payload_json"].as_str().unwrap())?,
            tx["raw_data"]["contract"][index]
        );
    }
    assert_eq!(facts["aml_tron_logs"][1]["log_index"], 1);
    assert_eq!(facts["aml_tron_logs"][1]["data_hex"], "f".repeat(64));
    assert_eq!(facts["aml_tron_logs"][2]["topics"][3], "f".repeat(64));
    assert!(facts["aml_tron_internals"][0]["rejected"].is_null());
    assert_eq!(facts["aml_tron_internals"][1]["rejected"], 1);
    assert_eq!(facts["aml_tron_internals"][2]["rejected"], 0);
    let values: serde_json::Value = serde_json::from_str(
        facts["aml_tron_internals"][2]["call_value_info_json"]
            .as_str()
            .unwrap(),
    )?;
    assert_eq!(values[0], json!({}));
    assert_eq!(values[1]["callValue"], 9_223_372_036_854_775_807u64);
    let resource = &facts["aml_tron_resources"][0];
    assert_eq!(resource["fee_raw"], "0");
    assert_eq!(resource["net_usage"], 0);
    assert!(resource["energy_fee"].is_null());
    assert_eq!(resource["energy_usage_total"], 123);
    let raw: serde_json::Value = serde_json::from_str(
        facts["aml_raw_transactions"][0]["payload_json"]
            .as_str()
            .unwrap(),
    )?;
    assert_eq!(raw, tx);
    assert!(verified.facts(&block(), 1, 99).is_err());
    Ok(())
}
#[test]
fn mismatched_hash_json_contract_order_type_and_each_financial_field_are_rejected() {
    let tx = source::multi_transaction();
    let receipt = source::receipt(&tx);
    let mut variants = Vec::new();
    let mut changed = tx.clone();
    changed["txID"] = json!("0".repeat(64));
    variants.push(changed);
    let mut changed = tx.clone();
    changed["raw_data"]["timestamp"] = json!(1);
    variants.push(changed);
    let mut changed = tx.clone();
    changed["raw_data"]["contract"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    variants.push(changed);
    let mut changed = tx.clone();
    changed["raw_data"]["contract"]
        .as_array_mut()
        .unwrap()
        .pop();
    variants.push(changed);
    for (contract, key) in [
        (0, "amount"),
        (1, "amount"),
        (2, "call_value"),
        (2, "call_token_value"),
        (2, "token_id"),
    ] {
        let mut changed = tx.clone();
        changed["raw_data"]["contract"][contract]["parameter"]["value"][key] = json!(999);
        variants.push(changed);
    }
    for (contract, key) in [
        (0, "owner_address"),
        (0, "to_address"),
        (1, "to_address"),
        (2, "contract_address"),
    ] {
        let mut changed = tx.clone();
        changed["raw_data"]["contract"][contract]["parameter"]["value"][key] = json!(
            if tx["raw_data"]["contract"][contract]["parameter"]["value"][key] == source::TARGET {
                source::OWNER
            } else {
                source::TARGET
            }
        );
        variants.push(changed);
    }
    let mut changed = tx.clone();
    changed["raw_data"]["contract"][2]["parameter"]["value"]["data"] = json!("abcdef00");
    variants.push(changed);
    let mut changed = tx.clone();
    changed["raw_data"]["contract"][1]["parameter"]["value"]["asset_name"] =
        json!(hex::encode("1000002"));
    variants.push(changed);
    let mut changed = tx.clone();
    changed["raw_data"]["contract"][0]["type"] = json!("TriggerSmartContract");
    variants.push(changed);
    let mut changed = tx.clone();
    changed["raw_data"]["contract"][0]["parameter"]["type_url"] = json!("wrong");
    variants.push(changed);
    let mut changed = tx.clone();
    changed["raw_data"]["contract"][0]["Permission_id"] = json!(2);
    variants.push(changed);
    for changed in variants {
        assert!(
            VerifiedTransaction::verify(&changed, &receipt, source::HEIGHT, source::TIME).is_err(),
            "accepted changed source: {changed}"
        );
    }
}
#[test]
fn receipts_from_other_transactions_or_blocks_and_malformed_resources_are_rejected() {
    let tx = source::multi_transaction();
    let receipt = source::receipt(&tx);
    for (key, value) in [
        ("id", json!("f".repeat(64))),
        ("blockNumber", json!(101)),
        ("blockTimeStamp", json!(source::TIME + 1)),
        ("receipt", json!("bad")),
        ("fee", json!(-1)),
        ("log", json!({})),
        ("internal_transactions", json!([null])),
    ] {
        let mut changed = receipt.clone();
        changed[key] = value;
        assert!(
            VerifiedTransaction::verify(&tx, &changed, source::HEIGHT, source::TIME).is_err(),
            "accepted invalid receipt field {key}"
        );
    }
    for value in [
        json!(-1),
        json!(1.1),
        json!("123"),
        json!(u64::MAX),
        json!(null),
    ] {
        let mut changed = receipt.clone();
        changed["receipt"]["net_usage"] = value;
        assert!(VerifiedTransaction::verify(&tx, &changed, source::HEIGHT, source::TIME).is_err());
    }
}
#[test]
fn unsupported_payload_is_retained_without_claiming_parameter_verification() -> Result<()> {
    let tx = source::transaction(vec![source::contract(
        11,
        "FreezeBalanceContract",
        &[
            source::bytes(1, &hex::decode(source::OWNER)?),
            source::integer(2, 1),
        ]
        .concat(),
        json!({"owner_address":source::OWNER,"frozen_balance":999}),
    )]);
    let receipt = source::receipt(&tx);
    let facts = VerifiedTransaction::verify(&tx, &receipt, source::HEIGHT, source::TIME)?.facts(
        &block(),
        0,
        1,
    )?;
    assert_eq!(facts["aml_tron_contracts"][0]["raw_data_hash_verified"], 1);
    assert_eq!(
        facts["aml_tron_contracts"][0]["parameter_fields_verified"],
        0
    );
    Ok(())
}
#[test]
fn proto3_omitted_zero_fields_match_but_ambiguous_or_malformed_bytes_fail() -> Result<()> {
    let owner = hex::decode(source::OWNER)?;
    let to = hex::decode(source::TARGET)?;
    let tx = source::transaction(vec![source::contract(
        1,
        "TransferContract",
        &[source::bytes(1, &owner), source::bytes(2, &to)].concat(),
        json!({"owner_address":source::OWNER,"to_address":source::TARGET}),
    )]);
    VerifiedTransaction::verify(&tx, &source::receipt(&tx), source::HEIGHT, source::TIME)?;
    let duplicate = source::transaction(vec![source::contract(
        1,
        "TransferContract",
        &[
            source::bytes(1, &owner),
            source::bytes(2, &to),
            source::integer(3, 1),
            source::integer(3, 2),
        ]
        .concat(),
        json!({"owner_address":source::OWNER,"to_address":source::TARGET,"amount":2}),
    )]);
    assert!(
        VerifiedTransaction::verify(
            &duplicate,
            &source::receipt(&duplicate),
            source::HEIGHT,
            source::TIME
        )
        .is_err()
    );
    for raw in [
        vec![0x5a, 0xff],
        vec![0],
        vec![0x80; 12],
        vec![0x5b],
        vec![0x59, 0],
    ] {
        let mut bad = tx.clone();
        bad["raw_data_hex"] = json!(hex::encode(&raw));
        bad["txID"] = json!(format!("{:x}", sha2::Sha256::digest(&raw)));
        assert!(
            VerifiedTransaction::verify(&bad, &source::receipt(&bad), source::HEIGHT, source::TIME)
                .is_err()
        );
    }
    Ok(())
}
#[test]
fn genesis_has_no_invented_receipt_logs_resources_or_successful_flow() -> Result<()> {
    let tx = source::transaction(vec![source::transfer()]);
    let verified = VerifiedTransaction::genesis(&tx, source::TIME)?;
    assert!(!verified.execution_succeeded());
    let mut context = block();
    context.block_number = 0;
    context.parent_hash = String::new();
    context.receipt_coverage = None;
    let facts = verified.facts(&context, 0, 1)?;
    assert!(facts["aml_raw_transactions"][0]["receipt_json"].is_null());
    for key in ["aml_tron_logs", "aml_tron_internals", "aml_tron_resources"] {
        assert!(facts[key].is_empty());
    }
    assert!(!facts.contains_key("aml_address_relationships"));
    assert!(VerifiedTransaction::verify(&tx, &source::receipt(&tx), 0, source::TIME).is_err());
    Ok(())
}

#[test]
fn explicit_transaction_or_receipt_failure_wins_and_original_evidence_survives() -> Result<()> {
    let tx = source::multi_transaction();
    let receipt = source::receipt(&tx);
    assert!(
        VerifiedTransaction::verify(&tx, &receipt, source::HEIGHT, source::TIME)?
            .execution_succeeded()
    );
    let mut receipt_failed = receipt.clone();
    receipt_failed["result"] = json!("FAILED");
    let mut vm_failed = receipt.clone();
    vm_failed["receipt"]["result"] = json!("REVERT");
    let mut ret_failed = tx.clone();
    ret_failed["ret"][0]["contractRet"] = json!("REVERT");
    for (transaction, info) in [
        (&tx, &receipt_failed),
        (&tx, &vm_failed),
        (&ret_failed, &receipt),
    ] {
        let verified =
            VerifiedTransaction::verify(transaction, info, source::HEIGHT, source::TIME)?;
        assert!(!verified.execution_succeeded());
        let facts = verified.facts(&block(), 0, 1)?;
        assert_eq!(
            facts["aml_tron_logs"].len(),
            3,
            "raw logs of a failed receipt remain evidence, never successful flows"
        );
        assert!(!facts.contains_key("aml_address_relationships"));
    }
    Ok(())
}

use sha2::Digest;
