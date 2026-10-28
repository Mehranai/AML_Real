use aml_warehouse::{Network, publication::Block};
use anyhow::Result;
use bitcoin_aml::{
    model,
    warehouse::{self, LegacyBlock},
};
use serde_json::json;
use std::collections::HashMap;

fn evidence() -> Result<(LegacyBlock, model::Evidence)> {
    let block = json!({"height":7,"hash":"a".repeat(64),"previousblockhash":"b".repeat(64),"time":123,
    "tx":[
        {"txid":"1".repeat(64),"vin":[{"coinbase":"00"}],"vout":[{"n":0,"value":"50","scriptPubKey":{"hex":"6a01ff","type":"nulldata"}}]},
        {"txid":"2".repeat(64),"vin":[{"txid":"3".repeat(64),"vout":2,"prevout":{"value":"0.00000002","scriptPubKey":{"hex":"6a01ff","type":"nulldata"}}}],"vout":[{"n":0,"value":"0.00000001","scriptPubKey":{"hex":"6a01ee","type":"nulldata"}}]}
    ]});
    let source = LegacyBlock {
        block_height: 7,
        block_hash: "a".repeat(64),
        parent_hash: "b".repeat(64),
        block_time: 123,
        commit_id: "legacy-attempt".into(),
        transaction_count: 2,
    };
    Ok((
        source,
        model::extract(&block, "legacy-attempt", &HashMap::new())?,
    ))
}

#[test]
fn utxo_legs_preserve_one_satoshi_and_unknown_addresses() -> Result<()> {
    let (source, evidence) = evidence()?;
    warehouse::validate_evidence(&source, &evidence)?;
    let block = Block {
        network_id: Network::Bitcoin.id().into(),
        block_number: 7,
        block_hash: source.block_hash,
        parent_hash: source.parent_hash,
        block_timestamp_unix_ms: 123000,
        commit_id: "warehouse-attempt".into(),
        revision: 20,
        canonical: 1,
        ingestion_status: "complete".into(),
        transaction_count: 2,
        receipt_coverage: None,
        trace_coverage: None,
        prevout_coverage: Some(1),
        source_id: "test".into(),
        created_at_unix_ms: 1,
    };
    let rows = warehouse::map_evidence(&block, &evidence)?;
    let tx = &rows["aml_transactions"][1];
    assert!(
        tx["from_address"].is_null() && tx["to_address"].is_null() && tx["amount_raw"].is_null()
    );
    assert_eq!(tx["fee_raw"], "1");
    let input = &rows["aml_bitcoin_inputs"][0];
    assert_eq!(input["value_sats"], 2);
    assert!(input["address"].is_null());
    assert_eq!(input["prev_output_index"], 2);
    let legs = &rows["aml_address_relationships"];
    assert_eq!(legs.len(), 3); // 1 vin + 2 vouts; never an address Cartesian product.
    assert_eq!(legs[0]["from_subject_type"], "script");
    assert_eq!(legs[0]["to_subject_type"], "transaction");
    assert_eq!(legs[2]["amount_raw"], "1");
    assert_eq!(legs[2]["to_subject_type"], "script");
    assert_eq!(legs[0]["trace_address"], json!([]));
    assert!(legs[0]["token_id"].is_null());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(legs[0]["raw_evidence_json"].as_str().unwrap())?,
        serde_json::to_value(&evidence.inputs[0])?
    );
    let contract = aml_warehouse::current_contract()?;
    for (name, rows) in rows {
        let table = contract.tables.iter().find(|t| t.name == name).unwrap();
        for row in rows {
            assert_eq!(row.as_object().unwrap().len(), table.columns.len());
            for column in &table.columns {
                assert!(
                    row.get(&column.name).is_some(),
                    "missing {name}.{}",
                    column.name
                );
            }
        }
    }
    Ok(())
}

#[test]
fn incomplete_or_mismatched_legacy_evidence_cannot_be_published() -> Result<()> {
    let (mut source, mut evidence) = evidence()?;
    source.transaction_count = 3;
    assert!(warehouse::validate_evidence(&source, &evidence).is_err());
    source.transaction_count = 2;
    evidence.inputs[0].output.value_sats = 1;
    assert!(warehouse::validate_evidence(&source, &evidence).is_err());
    evidence.inputs[0].output.value_sats = 2;
    evidence.inputs[0].output.address = "1BoatSLRHtKNngkdXEeobR76b53LETtpyT".into();
    assert!(warehouse::validate_evidence(&source, &evidence).is_err());
    evidence.inputs[0].output.address = String::new();
    evidence.outputs[1].block_hash = "c".repeat(64);
    assert!(warehouse::validate_evidence(&source, &evidence).is_err());
    Ok(())
}
