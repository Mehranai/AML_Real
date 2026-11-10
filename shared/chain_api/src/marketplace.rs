use crate::{Network, response::Mode, seaport};
use aml_warehouse::{
    Connection, contract::identifier, publication::literal, values::normalize_hash,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "marketplace_tests.rs"]
mod tests;

const CONTRACT: &str = "nft_marketplace_evidence_v1";
const MAX_TRANSACTIONS: usize = 24;
const MAX_RECEIPTS: usize = 12;
const MAX_REFS: usize = 16;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    block_number: u64,
    block_hash: String,
    tx_hash: String,
    log_index: u32,
    emitter: String,
    topics: Vec<String>,
    data: String,
}

fn wallet_edges(data: &Value) -> impl Iterator<Item = &Value> {
    let wallet = &data["address"];
    data["graph"]["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(move |edge| {
            wallet.is_string()
                && (edge["from_subject_id"] == *wallet || edge["to_subject_id"] == *wallet)
                && edge["canonical_token_id"].is_string()
                && edge["canonical_asset_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("erc721:") || id.starts_with("erc1155:"))
        })
}

fn scope(data: &Value) -> (Vec<(u64, &str)>, bool) {
    let mut candidates = wallet_edges(data)
        .filter_map(|edge| {
            let block = edge["block_number"].as_u64()?;
            let hash = edge["tx_hash"].as_str()?;
            let snapshot = &data["source_snapshot"];
            let canonical_hash = normalize_hash(Network::Ethereum, hash).ok()? == hash;
            let indexed = (snapshot["start_block"].as_u64().unwrap_or(0)
                ..=snapshot["as_of_block"].as_u64()?)
                .contains(&block);
            (canonical_hash && indexed).then_some((block, hash))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .rev();
    let selected = candidates.by_ref().take(MAX_TRANSACTIONS).collect();
    (
        selected,
        candidates.next().is_some() || data["graph"]["truncated"] == true,
    )
}

fn project(receipt: &Receipt, order: &seaport::Order, data: &Value) -> Option<Value> {
    let offer: Vec<_> = order
        .offer
        .iter()
        .filter_map(|item| Some((item.nft_asset()?, item.amount_raw.as_str())))
        .collect();
    let consideration: Vec<_> = order
        .consideration
        .iter()
        .filter_map(|item| {
            Some((
                item.nft_asset()?,
                item.amount_raw.as_str(),
                item.recipient.as_deref()?,
            ))
        })
        .collect();
    let mut refs = wallet_edges(data)
        .filter_map(|edge| {
            if edge["tx_hash"] != receipt.tx_hash || edge["block_number"] != receipt.block_number {
                return None;
            }
            let related = offer.iter().any(|(asset, amount)| {
                order.recipient != seaport::ZERO
                    && edge["from_subject_id"] == order.offerer
                    && edge["to_subject_id"] == order.recipient
                    && edge["canonical_asset_id"] == asset.as_str()
                    && edge["amount_raw"] == *amount
            }) || consideration.iter().any(|(asset, amount, recipient)| {
                *recipient != seaport::ZERO
                    && edge["to_subject_id"] == *recipient
                    && edge["canonical_asset_id"] == asset.as_str()
                    && edge["amount_raw"] == *amount
            });
            related.then(|| edge["id"].as_str()).flatten()
        })
        .collect::<BTreeSet<_>>()
        .into_iter();
    let transfer_refs: Vec<_> = refs.by_ref().take(MAX_REFS).collect();
    (!transfer_refs.is_empty()).then(|| json!({
        "protocol":"seaport_1_6","event_type":"nft_order_fulfilled","claim":"protocol_order_event",
        "settlement_verified":false,"score_eligible":false,
        "receipt":receipt,"order":order,"related_nft_transfers":transfer_refs,
        "transfer_refs_truncated":refs.next().is_some()
    }))
}

fn valid_occurrence(receipt: &Receipt, data: &Value, selected: &[(u64, &str)]) -> bool {
    let snapshot = &data["source_snapshot"];
    receipt.emitter == seaport::ADDRESS
        && selected.contains(&(receipt.block_number, receipt.tx_hash.as_str()))
        && normalize_hash(Network::Ethereum, &receipt.block_hash)
            .is_ok_and(|hash| hash == receipt.block_hash)
        && (snapshot["as_of_block"] != receipt.block_number
            || snapshot["block_hash"] == receipt.block_hash)
}

fn envelope(data: &Value, status: &str, count: usize, items: Vec<Value>) -> Value {
    json!({"contract":CONTRACT,"network_id":Network::Ethereum.id(),"source_snapshot_id":data["source_snapshot"]["snapshot_id"],
        "status":status,"scope":"returned_wallet_nft_transfers","searched_transactions":count,"exhaustive":false,"items":items})
}

pub async fn observe(db: &impl Connection, database: &str, data: &Value) -> Value {
    match lookup(db, database, data).await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Canonical marketplace evidence unavailable: {error:#}");
            envelope(data, "unavailable", 0, Vec::new())
        }
    }
}

async fn lookup(db: &impl Connection, database: &str, data: &Value) -> Result<Value> {
    identifier(database)?;
    let (selected, mut partial) = scope(data);
    let mut items = Vec::new();
    if !selected.is_empty() {
        let blocks = selected
            .iter()
            .map(|(block, _)| block.to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(",");
        let pairs = selected
            .iter()
            .map(|(block, hash)| format!("({block},{})", literal(hash)))
            .collect::<Vec<_>>()
            .join(",");
        let emitter = seaport::ADDRESS;
        let topic = seaport::TOPIC;
        let rows = db.query_bounded(&format!(
            "SELECT toJSONString(tuple(toString(block_number),block_hash,tx_hash,log_index,address,topics,if(length(data)<={},data,'')))
             FROM {database}.aml_evm_logs FINAL
             WHERE network_id='eip155:1' AND block_number IN ({blocks}) AND (block_number,tx_hash) IN ({pairs})
               AND (block_number,block_hash,block_timestamp_unix_ms,commit_id,revision) IN (
                 SELECT block_number,block_hash,block_timestamp_unix_ms,commit_id,revision
                 FROM {database}.aml_ingested_blocks_canonical
                 WHERE network_id='eip155:1' AND block_number IN ({blocks})
               )
               AND address='{emitter}' AND ifNull(removed,0)=0 AND length(topics)=3
               AND topics[1] IN ('{topic}','0x{topic}')
             ORDER BY block_number DESC, tx_hash, log_index LIMIT {}
             SETTINGS max_threads=2,max_memory_usage=268435456,max_execution_time=5",
            seaport::MAX_HEX + 2, MAX_RECEIPTS + 1
        )).await?;
        partial |= rows.len() > MAX_RECEIPTS;
        let mut seen = BTreeSet::new();
        for row in rows.into_iter().take(MAX_RECEIPTS) {
            let (block, block_hash, tx_hash, log_index, emitter, topics, data_hex): (
                String,
                String,
                String,
                u32,
                String,
                Vec<String>,
                String,
            ) = serde_json::from_str(&row)?;
            let receipt = Receipt {
                block_number: block.parse()?,
                block_hash,
                tx_hash,
                log_index,
                emitter,
                topics,
                data: data_hex,
            };
            ensure!(
                valid_occurrence(&receipt, data, &selected),
                "unexpected marketplace occurrence"
            );
            if !seen.insert((
                receipt.block_number,
                receipt.tx_hash.clone(),
                receipt.log_index,
            )) {
                partial = true;
                continue;
            }
            let Some(order) = seaport::decode(&receipt.topics, &receipt.data) else {
                partial = true;
                continue;
            };
            if let Some(item) = project(&receipt, &order, data) {
                items.push(item);
            }
        }
    }
    Ok(envelope(
        data,
        if partial { "partial" } else { "available" },
        selected.len(),
        items,
    ))
}

pub(crate) fn validate(network: Network, mode: Mode, data: &Value) -> Result<()> {
    let Some(actual) = data.get("nft_marketplace_evidence") else {
        return Ok(());
    };
    ensure!(
        network == Network::Ethereum && mode == Mode::Wallet,
        "unsupported marketplace scope"
    );
    let (selected, partial) = scope(data);
    let status = actual["status"].as_str().unwrap_or_default();
    ensure!(
        matches!(status, "available" | "partial" | "unavailable")
            && (!partial || status != "available"),
        "invalid marketplace coverage"
    );
    let items = actual["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing marketplace items"))?;
    ensure!(
        items.len() <= MAX_RECEIPTS && (status != "unavailable" || items.is_empty()),
        "invalid marketplace item count"
    );
    let mut seen = BTreeSet::new();
    for item in items {
        let receipt: Receipt = serde_json::from_value(item["receipt"].clone())?;
        let order = seaport::decode(&receipt.topics, &receipt.data)
            .ok_or_else(|| anyhow::anyhow!("invalid marketplace receipt ABI"))?;
        ensure!(
            valid_occurrence(&receipt, data, &selected)
                && seen.insert((
                    receipt.block_number,
                    receipt.tx_hash.clone(),
                    receipt.log_index
                ))
                && project(&receipt, &order, data).as_ref() == Some(item),
            "marketplace item contradicts receipt or graph"
        );
    }
    let count = if status == "unavailable" {
        0
    } else {
        selected.len()
    };
    ensure!(
        *actual == envelope(data, status, count, items.clone()),
        "invalid marketplace evidence envelope"
    );
    Ok(())
}
