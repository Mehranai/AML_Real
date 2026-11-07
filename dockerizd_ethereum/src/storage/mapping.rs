//! Direct live RPC/extractor -> shared evidence mapping. No database replay.
use crate::{db::records::asset_id, ethereum::ExtractedBlock};
use alloy::primitives::U256;
use aml_warehouse::{Network, publication::Block};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub fn amount(value: clickhouse::types::UInt256) -> String {
    value.to_string()
}
fn optional(s: &str) -> Value {
    if s.is_empty() { Value::Null } else { json!(s) }
}
pub fn evidence(block: &Block, extracted: &ExtractedBlock) -> Result<BTreeMap<String, Vec<Value>>> {
    ensure!(
        extracted.block.network_id == Network::Ethereum.id()
            && extracted.block.block_number == block.block_number
            && extracted.block.block_hash == block.block_hash
            && extracted.block.block_timestamp_unix_ms == block.block_timestamp_unix_ms
            && extracted.block.transaction_count == block.transaction_count,
        "source block differs from the shared publication header"
    );
    let header = json!({"network_id":Network::Ethereum.id(),"block_number":block.block_number,"block_hash":block.block_hash,"block_timestamp_unix_ms":block.block_timestamp_unix_ms,"commit_id":block.commit_id,"revision":block.revision,"created_at_unix_ms":block.created_at_unix_ms});
    let bind = |table: &str, mut row: Value| -> Result<Value> {
        row.as_object_mut()
            .context("evidence object required")?
            .extend(header.as_object().unwrap().clone());
        let mut row = aml_warehouse::core_schema::extend(table, row)?;
        for defaults in [
            aml_warehouse::analytics_schema::defaults(table)?,
            aml_warehouse::visibility_schema::defaults(table)?,
        ] {
            for (key, value) in defaults {
                row.as_object_mut().unwrap().entry(key).or_insert(value);
            }
        }
        Ok(row)
    };
    let mut facts = BTreeMap::<String, Vec<Value>>::new();
    for name in [
        "aml_transactions",
        "aml_address_relationships",
        "aml_evm_logs",
        "aml_evm_traces",
        "aml_transaction_features",
        "aml_semantic_aml_events",
        "aml_metadata_discoveries",
        "aml_raw_transactions",
    ] {
        facts.insert(name.into(), Vec::new());
    }
    let receipts = extracted
        .source_receipts
        .iter()
        .filter_map(|r| r["transactionHash"].as_str().map(|h| (h, r)))
        .collect::<BTreeMap<_, _>>();
    for (i, tx) in extracted.transactions.iter().enumerate() {
        ensure!(
            tx.transaction_index as usize == i,
            "original transaction order mismatch"
        );
        ensure!(
            tx.network_id == block.network_id
                && tx.block_number == block.block_number
                && tx.block_hash == block.block_hash
                && tx.block_timestamp_unix_ms == block.block_timestamp_unix_ms,
            "transaction source header differs from its block"
        );
        let mut raw = serde_json::to_value(tx)?;
        for (key, value) in [
            ("value", tx.value),
            ("effective_gas_price", tx.effective_gas_price),
            ("fee_paid", tx.fee_paid),
            ("max_fee_per_gas", tx.max_fee_per_gas),
            ("max_priority_fee_per_gas", tx.max_priority_fee_per_gas),
            ("blob_gas_price", tx.blob_gas_price),
        ] {
            raw[key] = json!(amount(value));
        }
        facts.get_mut("aml_transactions").unwrap().push(bind("aml_transactions",json!({"tx_hash":tx.tx_hash,"transaction_index":tx.transaction_index,"from_address":optional(&tx.from_address),"to_address":optional(&tx.to_address),"amount_raw":amount(tx.value),"fee_raw":amount(tx.fee_paid),"asset_id":"native:ETH","decimals":18,"status":if tx.status_known==0{"unknown"}else if tx.status==1{"success"}else{"failed"},"coinbase":0,"input_count":null,"output_count":null,"raw_evidence_json":raw.to_string()}))?);
        facts.get_mut("aml_raw_transactions").unwrap().push(bind("aml_raw_transactions",json!({"tx_hash":tx.tx_hash,"transaction_index":tx.transaction_index,"source_id":block.source_id,"payload_json":extracted.source_transactions.get(i).context("raw transaction missing")?.to_string(),"receipt_json":receipts.get(tx.tx_hash.as_str()).context("raw receipt missing")?.to_string(),"evidence_refs":[format!("tx:{}",tx.tx_hash)]}))?);
    }
    for r in &extracted.relationships {
        ensure!(
            r.transfer_type != "validator_withdrawal",
            "block credits must not be forged into transaction facts"
        );
        ensure!(
            r.network_id == block.network_id
                && r.block_number == block.block_number
                && r.block_hash == block.block_hash
                && r.block_timestamp_unix_ms == block.block_timestamp_unix_ms,
            "relationship source header differs from its block"
        );
        let mut raw = serde_json::to_value(r)?;
        raw["amount"] = json!(amount(r.amount));
        facts.get_mut("aml_address_relationships").unwrap().push(bind("aml_address_relationships",json!({"relationship_id":r.relationship_id,"tx_hash":r.tx_hash,"transaction_index":r.transaction_index,"event_index":r.event_index,"event_sub_index":r.event_sub_index,"trace_address":r.trace_address,"from_subject_id":r.from_address,"from_subject_type":"address","to_subject_id":r.to_address,"to_subject_type":"address","from_address":r.from_address,"to_address":r.to_address,"asset_id":asset_id(&r.asset_id,&r.token_id)?,"token_id":optional(&r.token_id),"amount_raw":amount(r.amount),"decimals":if r.asset_id.ends_with("/native:eth"){json!(18)}else{Value::Null},"transfer_type":r.transfer_type,"attribution_method":"observed_transfer","confidence":1.0,"prev_tx_hash":null,"prev_output_index":null,"output_index":null,"evidence_refs":[format!("tx:{}",r.tx_hash)],"raw_evidence_json":raw.to_string()}))?);
    }
    for log in &extracted.logs {
        let raw = receipts
            .get(log.tx_hash.as_str())
            .and_then(|r| r["logs"].as_array())
            .into_iter()
            .flatten()
            .find(|r| {
                r["logIndex"]
                    .as_str()
                    .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                    == Some(log.log_index as u64)
            })
            .context("original receipt log missing")?;
        facts.get_mut("aml_evm_logs").unwrap().push(bind("aml_evm_logs",json!({"tx_hash":log.tx_hash,"transaction_index":log.transaction_index,"log_index":log.log_index,"address":log.contract_address,"topics":log.topics,"data":log.data.trim_start_matches("0x"),"removed":0,"raw_evidence_json":raw.to_string()}))?);
    }
    for feature in &extracted.transaction_features {
        let subject = extracted
            .transactions
            .iter()
            .find(|t| t.tx_hash == feature.tx_hash)
            .context("feature transaction missing")?
            .from_address
            .clone();
        facts.get_mut("aml_transaction_features").unwrap().push(bind("aml_transaction_features",json!({"feature_id":feature.feature_id,"tx_hash":feature.tx_hash,"subject_id":subject,"detector_version":feature.detector_version,"features_json":serde_json::to_string(feature)?,"evidence_refs":feature.evidence_refs.iter().map(|id|format!("event:{id}")).collect::<Vec<_>>(),"raw_evidence_json":serde_json::to_string(feature)?}))?);
    }
    for event in &extracted.semantic_events {
        facts.get_mut("aml_semantic_aml_events").unwrap().push(bind("aml_semantic_aml_events",json!({"event_id":event.event_id,"tx_hash":event.tx_hash,"subject_id":event.subject_address,"event_type":event.event_type,"protocol":event.protocol,"detector_version":event.detector_version,"confidence":event.confidence as f64,"asset_in":if event.asset_in.is_empty(){Value::Null}else{json!(asset_id(&event.asset_in,"")?)},"asset_out":if event.asset_out.is_empty(){Value::Null}else{json!(asset_id(&event.asset_out,"")?)},"amount_in":optional(&event.amount_in),"amount_out":optional(&event.amount_out),"remote_network_id":optional(&event.remote_network_id),"evidence_refs":[format!("tx:{}",event.tx_hash)],"evidence_json":event.evidence_json,"event_index":event.event_index,"event_sub_index":0,"detector":event.detector,"raw_evidence_json":serde_json::to_string(event)?}))?);
    }
    for d in &extracted.token_discoveries {
        let log = extracted
            .logs
            .iter()
            .find(|l| l.contract_address == d.token_address)
            .context("metadata discovery lacks an observed source log")?;
        let asset = crate::db::records::collection(&d.token_standard, &d.token_address)?;
        facts.get_mut("aml_metadata_discoveries").unwrap().push(bind("aml_metadata_discoveries",json!({"discovery_id":format!("{}:{asset}",block.block_hash),"tx_hash":log.tx_hash,"transaction_index":log.transaction_index,"asset_id":asset,"standard_hint":d.token_standard,"evidence_id":log.event_id,"source_id":block.source_id,"evidence_refs":[format!("tx:{}",log.tx_hash)],"raw_evidence_json":serde_json::to_string(d)?}))?);
    }
    for trace in &extracted.source_traces {
        fn walk(
            frame: &Value,
            path: Vec<u32>,
            failed: bool,
            tx: &str,
            index: u32,
            source: &str,
            out: &mut Vec<Value>,
        ) -> Result<()> {
            let failed = failed
                || frame.get("error").is_some_and(|v| !v.is_null())
                || frame.get("revertReason").is_some_and(|v| !v.is_null());
            let value = frame["value"]
                .as_str()
                .map(|s| {
                    U256::from_str_radix(s.trim_start_matches("0x"), 16).map(|v| v.to_string())
                })
                .transpose()?;
            out.push(json!({"tx_hash":tx,"trace_id":format!("{tx}:{}",serde_json::to_string(&path)?),"transaction_index":index,"trace_address":path,"trace_type":frame["type"],"from_address":frame.get("from").cloned().unwrap_or(Value::Null),"to_address":frame.get("to").cloned().unwrap_or(Value::Null),"value_raw":value,"success":u8::from(!failed),"error_message":frame.get("error").or_else(||frame.get("revertReason")).cloned().unwrap_or(Value::Null),"source_id":source,"evidence_refs":[format!("tx:{tx}")],"raw_evidence_json":frame.to_string()}));
            for (i, child) in frame["calls"].as_array().into_iter().flatten().enumerate() {
                let mut p = path.clone();
                p.push(i.try_into()?);
                walk(child, p, failed, tx, index, source, out)?;
            }
            Ok(())
        }
        let mut rows = Vec::new();
        walk(
            &trace["root"],
            vec![],
            false,
            trace["tx_hash"].as_str().context("trace hash missing")?,
            trace["transaction_index"]
                .as_u64()
                .context("trace transaction position missing")?
                .try_into()?,
            &block.source_id,
            &mut rows,
        )?;
        for r in rows {
            facts
                .get_mut("aml_evm_traces")
                .unwrap()
                .push(bind("aml_evm_traces", r)?);
        }
    }
    Ok(facts)
}
