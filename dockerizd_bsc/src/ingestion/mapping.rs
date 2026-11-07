//! Live BSC RPC evidence -> the same typed core contract as the other adapters.
use super::{
    model::{CanonicalBlock, SourceEvidence},
    transfers::ExtractedBlockEvidence,
};
use crate::{db::records::asset_id, semantic::SemanticBlockEvidence};
use aml_warehouse::{Network, publication::Block};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn optional(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    }
}
fn quantity(value: &str) -> Result<String> {
    Ok(super::model::EvmU256::parse("trace.value", value)?.decimal_string())
}
fn amount_column(value: &str) -> Value {
    let max = aml_warehouse::RawAmount::MAX;
    if value.is_empty() || value.len() > max.len() || (value.len() == max.len() && value > max) {
        Value::Null
    } else {
        json!(value)
    }
}
pub(crate) fn evidence(
    header: &Block,
    block: &CanonicalBlock,
    extracted: &ExtractedBlockEvidence,
    semantic: &SemanticBlockEvidence,
    source: &SourceEvidence,
) -> Result<BTreeMap<String, Vec<Value>>> {
    ensure!(
        header.network_id == Network::Bsc.id()
            && header.block_number == block.number
            && header.block_hash == block.hash
            && header.block_timestamp_unix_ms == block.timestamp_unix_ms,
        "BSC publication header differs from its source"
    );
    let common = json!({"network_id":header.network_id,"block_number":header.block_number,"block_hash":header.block_hash,"block_timestamp_unix_ms":header.block_timestamp_unix_ms,"commit_id":header.commit_id,"revision":header.revision,"created_at_unix_ms":header.created_at_unix_ms});
    let bind = |table: &str, mut data: Value| -> Result<Value> {
        data.as_object_mut()
            .context("fact object required")?
            .extend(common.as_object().unwrap().clone());
        let mut row = aml_warehouse::core_schema::extend(table, data)?;
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
    let references = |ids: &[String]| {
        ids.iter()
            .map(|id| {
                if aml_warehouse::values::evidence_ref(Network::Bsc, id).is_ok() {
                    id.clone()
                } else if block.transactions.iter().any(|t| t.hash == *id) {
                    format!("tx:{id}")
                } else if extracted
                    .relationships
                    .iter()
                    .any(|r| r.relationship_id == *id)
                {
                    format!("relationship:{id}")
                } else if block.logs.iter().any(|l| l.event_id == *id) {
                    format!("log:{id}")
                } else if semantic.events.iter().any(|e| e.event_id == *id) {
                    format!("event:{id}")
                } else {
                    format!("source:{id}")
                }
            })
            .collect::<Vec<_>>()
    };
    let mut facts = BTreeMap::new();
    for name in [
        "aml_transactions",
        "aml_address_relationships",
        "aml_evm_logs",
        "aml_evm_traces",
        "aml_metadata_discoveries",
        "aml_transaction_features",
        "aml_semantic_aml_events",
        "aml_raw_transactions",
    ] {
        facts.insert(name.to_owned(), vec![]);
    }
    let receipts = source
        .receipts
        .iter()
        .map(|r| {
            Ok((
                aml_warehouse::values::normalize_hash(
                    Network::Bsc,
                    r["transactionHash"]
                        .as_str()
                        .context("source receipt hash missing")?,
                )?,
                r,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for (index, t) in block.transactions.iter().enumerate() {
        let raw = source.block["transactions"]
            .get(index)
            .context("original transaction missing")?;
        ensure!(
            aml_warehouse::values::normalize_hash(
                Network::Bsc,
                raw["hash"]
                    .as_str()
                    .context("source transaction hash missing")?
            )? == t.hash,
            "raw transaction position mismatch"
        );
        facts.get_mut("aml_transactions").unwrap().push(bind("aml_transactions",json!({"tx_hash":t.hash,"transaction_index":t.transaction_index,"from_address":optional(&t.from_address),"to_address":optional(&t.to_address),"amount_raw":t.value.decimal_string(),"fee_raw":t.fee_paid.decimal_string(),"asset_id":"native:BNB","decimals":18,"status":if t.status==1{"success"}else{"failed"},"coinbase":0,"input_count":null,"output_count":null,"raw_evidence_json":serde_json::to_string(t)?}))?);
        facts.get_mut("aml_raw_transactions").unwrap().push(bind("aml_raw_transactions",json!({"tx_hash":t.hash,"transaction_index":t.transaction_index,"source_id":header.source_id,"payload_json":raw.to_string(),"receipt_json":receipts.get(&t.hash).context("source receipt missing")?.to_string(),"evidence_refs":[format!("tx:{}",t.hash)]}))?);
    }
    for r in &extracted.relationships {
        facts.get_mut("aml_address_relationships").unwrap().push(bind("aml_address_relationships",json!({"relationship_id":r.relationship_id,"tx_hash":r.tx_hash,"transaction_index":r.transaction_index,"event_index":r.event_index,"event_sub_index":r.event_sub_index,"trace_address":r.trace_address,"from_subject_id":r.from_address,"from_subject_type":"address","to_subject_id":r.to_address,"to_subject_type":"address","from_address":r.from_address,"to_address":r.to_address,"asset_id":asset_id(&r.asset_id)?,"token_id":optional(&r.token_id),"amount_raw":r.amount.decimal_string(),"decimals":if r.asset_id.ends_with("/native:bnb"){json!(18)}else{Value::Null},"transfer_type":r.transfer_type,"attribution_method":"observed_transfer","confidence":1.0,"prev_tx_hash":null,"prev_output_index":null,"output_index":null,"evidence_refs":[format!("tx:{}",r.tx_hash)],"raw_evidence_json":json!({"event_index":r.event_index,"event_sub_index":r.event_sub_index,"trace_address":r.trace_address}).to_string()}))?);
    }
    for l in &block.logs {
        let raw = receipts
            .get(&l.tx_hash)
            .and_then(|r| r["logs"].as_array())
            .into_iter()
            .flatten()
            .find(|r| {
                r["logIndex"]
                    .as_str()
                    .and_then(|n| u64::from_str_radix(n.trim_start_matches("0x"), 16).ok())
                    == Some(l.log_index as u64)
            })
            .context("original log position missing")?;
        facts.get_mut("aml_evm_logs").unwrap().push(bind("aml_evm_logs",json!({"tx_hash":l.tx_hash,"transaction_index":l.transaction_index,"log_index":l.log_index,"address":l.contract_address,"topics":l.topics,"data":l.data.trim_start_matches("0x"),"removed":raw["removed"].as_bool().map(u8::from),"raw_evidence_json":raw.to_string()}))?);
    }
    for d in &extracted.token_discoveries {
        let tx = block
            .transactions
            .iter()
            .find(|t| t.hash == d.tx_hash)
            .context("discovery transaction missing")?;
        ensure!(
            block
                .logs
                .iter()
                .any(|l| l.event_id == d.evidence_id && l.tx_hash == d.tx_hash),
            "discovery source log missing"
        );
        facts.get_mut("aml_metadata_discoveries").unwrap().push(bind("aml_metadata_discoveries",json!({"discovery_id":d.discovery_id,"tx_hash":d.tx_hash,"transaction_index":tx.transaction_index,"asset_id":crate::db::records::collection(d.standard_hint,&d.token_address)?,"standard_hint":d.standard_hint,"evidence_id":d.evidence_id,"source_id":header.source_id,"evidence_refs":[format!("log:{}",d.evidence_id)],"raw_evidence_json":json!({"token_address":d.token_address,"standard_hint":d.standard_hint}).to_string()}))?);
    }
    for f in &semantic.transaction_features {
        let tx = block
            .transactions
            .iter()
            .find(|t| t.hash == f.tx_hash)
            .context("feature transaction missing")?;
        facts.get_mut("aml_transaction_features").unwrap().push(bind("aml_transaction_features",json!({"feature_id":f.feature_id,"tx_hash":f.tx_hash,"subject_id":tx.from_address,"detector_version":f.detector_version,"features_json":serde_json::to_string(f)?,"evidence_refs":references(&f.evidence_refs),"raw_evidence_json":serde_json::to_string(f)?}))?);
    }
    for e in &semantic.events {
        let amount_in = amount_column(&e.amount_in);
        let amount_out = amount_column(&e.amount_out);
        facts.get_mut("aml_semantic_aml_events").unwrap().push(bind("aml_semantic_aml_events",json!({"event_id":e.event_id,"tx_hash":e.tx_hash,"subject_id":e.subject_address,"event_type":e.event_type,"protocol":e.protocol,"detector_version":e.detector_version,"confidence":e.confidence as f64,"asset_in":if e.asset_in.is_empty(){Value::Null}else{json!(asset_id(&e.asset_in)?)},"asset_out":if e.asset_out.is_empty(){Value::Null}else{json!(asset_id(&e.asset_out)?)},"amount_in":amount_in,"amount_out":amount_out,"remote_network_id":optional(&e.remote_network_id),"evidence_refs":references(&e.evidence_refs),"evidence_json":json!({"source_evidence":serde_json::from_str::<Value>(&e.evidence_json)?,"amount_in_exceeds_uint256":!e.amount_in.is_empty()&&amount_in.is_null(),"amount_out_exceeds_uint256":!e.amount_out.is_empty()&&amount_out.is_null()}).to_string(),"event_index":e.event_index,"event_sub_index":0,"detector":e.detector,"raw_evidence_json":serde_json::to_string(e)?}))?);
    }
    fn walk(
        frame: &Value,
        path: Vec<u32>,
        ancestor_failed: bool,
        t: &super::model::CanonicalTransaction,
        source: &str,
        rows: &mut Vec<Value>,
    ) -> Result<()> {
        let failed =
            ancestor_failed || frame["error"].is_string() || frame["revertReason"].is_string();
        rows.push(json!({"tx_hash":t.hash,"transaction_index":t.transaction_index,"trace_id":format!("{}:{}",t.hash,serde_json::to_string(&path)?),"trace_address":path,"trace_type":frame["type"],"from_address":frame["from"],"to_address":frame["to"],"value_raw":frame["value"].as_str().map(quantity).transpose()?,"success":u8::from(!failed),"error_message":frame.get("error").or_else(||frame.get("revertReason")).cloned().unwrap_or(Value::Null),"source_id":source,"evidence_refs":[format!("tx:{}",t.hash)],"raw_evidence_json":frame.to_string()}));
        for (i, child) in frame["calls"].as_array().into_iter().flatten().enumerate() {
            let mut next = path.clone();
            next.push(i.try_into()?);
            walk(child, next, failed, t, source, rows)?;
        }
        Ok(())
    }
    for trace in source.traces.as_deref().unwrap_or(&[]) {
        let hash = aml_warehouse::values::normalize_hash(
            Network::Bsc,
            trace["txHash"]
                .as_str()
                .context("trace source hash missing")?,
        )?;
        let tx = block
            .transactions
            .iter()
            .find(|t| t.hash == hash)
            .context("trace source transaction missing")?;
        let mut rows = vec![];
        walk(
            &trace["result"],
            vec![],
            tx.status != 1,
            tx,
            &header.source_id,
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
