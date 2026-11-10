use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use clickhouse::types::UInt256;
use futures::stream::{self, StreamExt};
use serde_json::Value;

use crate::models::tron::modules::TransactionRow;

use crate::models::tron::modules::TransactionFeatureRow;

use crate::services::loader::LoaderTron;
use crate::services::tron::ingestion_state::{
    FinalizedHashConflict, record_ingestion_failure, resolve_ingestion_failures,
    should_ingest_block,
};

// aml section
use crate::services::tron::aml::bridge_detector::detect_bridges;
use crate::services::tron::aml::liquidity_detector::detect_liquidity_events;
use crate::services::tron::aml::receipt_swap::decode_v2_swaps;
use crate::services::tron::aml::swap_detector::detect_swaps;
use crate::services::tron::aml::types::SimpleTransfer;

use crate::services::tron::tron_classifier::classifier::classify;
use crate::services::tron::tron_classifier::types::{ClassificationInput, ContractCategory};

use crate::services::tron::semantic_event_builder::build_semantic_event_rows;
use crate::services::tron::transaction_type::{
    TransactionSemanticsInput, classify_transaction_semantics,
};
use crate::services::tron::transfer_extractor::{
    extract_contract_transfers, extract_internal_transfers, extract_trc20_transfers,
    has_contract_call, primary_contract_summary, primary_method_data, standard_transfer_evidence,
};

use super::publication_store::{PreparedTransaction, publish_block};
use super::source_evidence::VerifiedTransaction;
use crate::services::tron::aml::mint_burn_detector::detect_mints_and_burns;

const ZERO_ADDRESS: &str = "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb";
const GENESIS_BLOCK_NUMBER: u64 = 0;
const MAX_REPLAY_BLOCKS: u64 = 10_000;

async fn process_tx(
    loader: Arc<LoaderTron>,
    tx: Value,
    block_number: u64,
    block_timestamp: u64,
) -> Result<PreparedTransaction> {
    let txid = tx["txID"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing txID"))?
        .to_string();

    let (contract_type, mut initiator_address, mut target_address, mut contract_address, _) =
        primary_contract_summary(&tx);

    let receipt = {
        let _permit = loader.rpc_limiter.acquire().await?;

        loader.tron_client.get_tx_receipt(&txid).await?
    };

    // All source checks finish before any fact of this block is written.
    let source = VerifiedTransaction::verify(&tx, &receipt, block_number, block_timestamp)
        .context("TRON source validation failed")?;
    let txid = source.tx_hash().to_owned();
    let mut canonical_transfers = extract_contract_transfers(&tx, &txid);
    let primary_index = tx["raw_data"]["contract"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .position(|c| c["type"] == "TriggerSmartContract")
        })
        .unwrap_or(0);
    if !source.parameter_verified(primary_index) {
        initiator_address.clear();
        target_address.clear();
        contract_address.clear();
    }
    let timestamp = block_timestamp;

    let status = transaction_status(&tx, &receipt);

    let fee = UInt256::from(receipt["fee"].as_u64().unwrap_or(0));

    let energy_usage_total = receipt["receipt"]["energy_usage_total"]
        .as_u64()
        .unwrap_or(0);

    let net_usage = receipt["receipt"]["net_usage"].as_u64().unwrap_or(0);

    // TRC20 log و internal transfer اضافه می‌شوند
    if status == 1 {
        canonical_transfers.extend(extract_trc20_transfers(&receipt, &txid)?);
        canonical_transfers.extend(extract_internal_transfers(&receipt, &txid));
    } else {
        canonical_transfers.clear();
    }

    let asset_resolutions = resolve_legacy_assets(&loader, &tx, &mut canonical_transfers).await?;
    let semantic_transfers = canonical_transfers
        .iter()
        .filter(|transfer| transfer.token_id.is_none())
        .map(|transfer| transfer.as_simple_transfer())
        .collect::<Vec<SimpleTransfer>>();
    let simple_transfers = semantic_transfers
        .iter()
        .filter(|transfer| transfer.from != ZERO_ADDRESS && transfer.to != ZERO_ADDRESS)
        .cloned()
        .collect::<Vec<_>>();

    let transaction = TransactionRow {
        tx_hash: txid.clone(),
        block_number,
        timestamp,
        initiator_address: initiator_address.clone(),
        target_address: target_address.clone(),
        contract_address: contract_address.clone(),
        contract_type: contract_type.clone(),
        fee,
        energy_usage_total,
        net_usage,
        status,
    };
    let mut events = Vec::new();
    let mut features = Vec::new();

    // Classification combines an approved address registry, native TRON contract types,
    // method selectors, target-contract token events, and conservative flow evidence.
    let transfer_evidence = standard_transfer_evidence(&receipt, &contract_address);
    let classification = classify(
        &loader.protocol_registry,
        &ClassificationInput {
            contract_address: contract_address.clone(),
            target_address: target_address.clone(),
            contract_type: contract_type.clone(),
            method_data: primary_method_data(&tx),
            has_trc20_transfer: transfer_evidence.has_trc20_transfer,
            has_trc721_transfer: transfer_evidence.has_nft_transfer,
        },
        &semantic_transfers,
    );

    // This flag describes the actual TRON execution form. A labeled DEX/bridge
    // address receiving a plain transfer must not be rewritten as a contract call.
    let is_contract_call = u8::from(has_contract_call(&tx));

    // AML features
    if !semantic_transfers.is_empty() {
        let semantic_actor = (!initiator_address.is_empty()).then_some(initiator_address.as_str());
        let decoded_swaps = decode_v2_swaps(
            &txid,
            block_number,
            timestamp,
            &initiator_address,
            &receipt,
            &canonical_transfers,
        );
        let dex_context = classification.category == ContractCategory::Dex;
        let liquidity_events = if dex_context {
            detect_liquidity_events(&semantic_transfers, semantic_actor)
        } else {
            Vec::new()
        };
        let raw_swaps = if dex_context {
            detect_swaps(&semantic_transfers, semantic_actor)
        } else {
            Vec::new()
        };
        let swaps = if !decoded_swaps.is_empty() {
            decoded_swaps
                .iter()
                .map(|swap| swap.event.clone())
                .collect()
        } else if liquidity_events.is_empty() {
            raw_swaps
        } else {
            Vec::new()
        };
        let mint_burns = detect_mints_and_burns(&semantic_transfers);
        let bridge_protocol_hint = classification.category == ContractCategory::Bridge;
        let bridges = detect_bridges(&semantic_transfers, bridge_protocol_hint);

        let mut aml_events = Vec::new();
        if decoded_swaps.is_empty() {
            aml_events.extend(swaps.clone());
        }
        for swap in decoded_swaps {
            events.push(swap.row);
        }
        aml_events.extend(bridges.clone());
        aml_events.extend(mint_burns.clone());
        aml_events.extend(liquidity_events.clone());

        for event in build_semantic_event_rows(
            &txid,
            block_number,
            timestamp,
            &aml_events,
            &classification.protocol,
            &classification.detection_source,
            classification.confidence,
            &canonical_transfers,
        ) {
            events.push(event);
        }

        let semantics = classify_transaction_semantics(TransactionSemanticsInput {
            classification: &classification,
            contract_type: &contract_type,
            is_contract_call: is_contract_call == 1,
            transfers: &semantic_transfers,
            swaps: &swaps,
            bridges: &bridges,
            mint_burns: &mint_burns,
            liquidity_events: &liquidity_events,
        });

        let feature = TransactionFeatureRow {
            tx_hash: txid.clone(),
            block_number,
            timestamp,
            transaction_type: semantics.transaction_type.clone(),
            transaction_subtype: semantics.transaction_subtype.clone(),
            classification_confidence: semantics.confidence,
            classification_source: semantics.source.clone(),
            protocol: semantics.protocol.clone(),
            method_id: semantics.method_id.clone(),
            is_swap: semantics.is_swap,
            is_bridge: semantics.is_bridge,
            is_mint: semantics.is_mint,
            is_burn: semantics.is_burn,
            is_liquidity_add: semantics.is_liquidity_add,
            is_liquidity_remove: semantics.is_liquidity_remove,
            is_contract_call,
        };

        features.push(feature);
    } else {
        let semantics = classify_transaction_semantics(TransactionSemanticsInput {
            classification: &classification,
            contract_type: &contract_type,
            is_contract_call: is_contract_call == 1,
            transfers: &simple_transfers,
            swaps: &[],
            bridges: &[],
            mint_burns: &[],
            liquidity_events: &[],
        });
        features.push(TransactionFeatureRow {
            tx_hash: txid.clone(),
            block_number,
            timestamp,
            transaction_type: semantics.transaction_type.clone(),
            transaction_subtype: semantics.transaction_subtype.clone(),
            classification_confidence: semantics.confidence,
            classification_source: semantics.source.clone(),
            protocol: semantics.protocol.clone(),
            method_id: semantics.method_id.clone(),
            is_swap: semantics.is_swap,
            is_bridge: semantics.is_bridge,
            is_mint: semantics.is_mint,
            is_burn: semantics.is_burn,
            is_liquidity_add: semantics.is_liquidity_add,
            is_liquidity_remove: semantics.is_liquidity_remove,
            is_contract_call,
        });
    }

    Ok(PreparedTransaction {
        source,
        transaction,
        transfers: canonical_transfers,
        events,
        feature: features.pop(),
        asset_resolutions,
    })
}

async fn resolve_legacy_assets(
    loader: &LoaderTron,
    tx: &Value,
    transfers: &mut [super::transfer_extractor::ExtractedTransfer],
) -> Result<BTreeMap<String, Value>> {
    let mut evidence = BTreeMap::new();
    for transfer in transfers {
        let contract = tx["raw_data"]["contract"]
            .as_array()
            .and_then(|a| a.get(transfer.source_index as usize));
        let name_hex = if transfer.source_kind == "contract"
            && contract.is_some_and(|c| c["type"] == "TransferAssetContract")
        {
            contract.unwrap()["parameter"]["value"]["asset_name"]
                .as_str()
                .context("verified TRC10 asset bytes missing")?
                .to_ascii_uppercase()
        } else if let Some(name) = transfer.asset_id.strip_prefix("TRC10:legacy:") {
            name.to_owned()
        } else {
            continue;
        };
        let mut cache = loader.legacy_assets.lock().await;
        let response = if let Some(value) = cache.get(&name_hex) {
            value.clone()
        } else {
            let _permit = loader.rpc_limiter.acquire().await?;
            let value = loader
                .tron_client
                .post(
                    "wallet/getassetissuebyname",
                    serde_json::json!({"value":name_hex}),
                )
                .await?;
            let name = value["name"].as_str().context(
                "TRON source validation failed: legacy TRC10 name is unresolved or ambiguous",
            )?;
            let id = value["id"]
                .as_str()
                .context("TRON source validation failed: legacy TRC10 numeric identity missing")?;
            let requested = hex::decode(&name_hex)?;
            let is_id = std::str::from_utf8(&requested).is_ok_and(|s| s == id);
            anyhow::ensure!(
                name.eq_ignore_ascii_case(&name_hex) || is_id,
                "TRON source validation failed: TRC10 catalog name/id mismatch"
            );
            let id = aml_warehouse::RawAmount::parse(id)?;
            anyhow::ensure!(
                id.as_str() != "0",
                "TRON source validation failed: invalid TRC10 catalog id"
            );
            aml_warehouse::values::tron_wallet(
                value["owner_address"]
                    .as_str()
                    .context("legacy TRC10 issuer missing")?,
            )?;
            let record = serde_json::json!({"rpc_method":"wallet/getassetissuebyname","observation":"current_identity_catalog_not_historical_state","observed_at_unix_ms":crate::db::now_ms(),"response":value});
            if cache.len() >= 10000 {
                cache.clear();
            }
            cache.insert(name_hex.clone(), record.clone());
            record
        };
        let id = response["response"]["id"]
            .as_str()
            .context("cached asset id missing")?;
        evidence.insert(name_hex, response.clone());
        transfer.asset_id = format!("TRC10:{id}");
    }
    Ok(evidence)
}

pub async fn fetch_tron(loader: Arc<LoaderTron>, start: u64, total_txs: u64) -> Result<()> {
    fetch_tron_with_options(loader, start, None, total_txs, false, None).await
}
pub async fn replay_tron_range(loader: Arc<LoaderTron>, start: u64, end: u64) -> Result<()> {
    validate_replay_range(start, end)?;
    fetch_tron_with_options(loader, start, Some(end), 0, true, None).await
}
pub async fn ingest_tron_range(loader: Arc<LoaderTron>, start: u64, end: u64) -> Result<()> {
    validate_replay_range(start, end)?;
    fetch_tron_with_options(loader, start, Some(end), 0, false, None).await
}
pub(crate) async fn maintenance_block(
    loader: Arc<LoaderTron>,
    height: u64,
    historical: bool,
    owner: &std::fs::File,
) -> Result<()> {
    fetch_tron_with_options(loader, height, Some(height), 0, historical, Some(owner)).await
}
async fn fetch_tron_with_options(
    loader: Arc<LoaderTron>,
    start: u64,
    end: Option<u64>,
    total_txs: u64,
    force_replay: bool,
    owner: Option<&std::fs::File>,
) -> Result<()> {
    let _writer = if owner.is_none() {
        Some(super::publication_store::writer_lock(
            loader.clickhouse.database(),
        )?)
    } else {
        None
    };
    loader.tron_client.validate_mainnet().await?;
    let latest = loader.tron_client.get_solid_block_number().await?;
    let end = end.unwrap_or(latest);
    anyhow::ensure!(
        end <= latest,
        "requested TRON end block {end} is above latest solid block {latest}"
    );
    let state = aml_warehouse::publication::state(
        &*loader.clickhouse,
        loader.clickhouse.database(),
        aml_warehouse::Network::Tron,
    )
    .await?;
    if force_replay {
        anyhow::ensure!(
            state.initialized == 1 && start >= state.start_block && end <= state.last_block_number,
            "TRON source validation failed: replay refresh requires an already published range; use ingestion for new heights"
        );
        loader.protocol_registry.refresh(&loader.clickhouse).await?;
    } else if state.initialized == 1 {
        anyhow::ensure!(
            start >= state.start_block && start <= state.last_block_number.saturating_add(1),
            "TRON source validation failed: requested range leaves a checkpoint gap or precedes the published start"
        );
    }
    // The follow loop can poll with start=tip+1 and no new solid blocks. Verify
    // its checkpoint even when the range is empty, before treating it as a no-op.
    if state.initialized == 1 {
        let checked = async {
            anyhow::ensure!(
                latest >= state.last_block_number,
                "TRON source validation failed: solid head is behind the published checkpoint"
            );
            let source = {
                let _permit = loader.rpc_limiter.acquire().await?;
                loader
                    .tron_client
                    .get_block(state.last_block_number)
                    .await?
            };
            let header =
                super::publication_store::block_header(&source, state.last_block_number, 3)?;
            should_ingest_block(
                &loader.clickhouse,
                state.last_block_number,
                &header.block_hash,
                false,
            )
            .await?;
            anyhow::Ok(())
        }
        .await;
        if let Err(error) = checked {
            record_error(
                &loader.clickhouse,
                state.last_block_number,
                &state.last_block_hash,
                "",
                "VERIFY_CHECKPOINT",
                &error,
            )
            .await?;
            return Err(error);
        }
        resolve_ingestion_failures(
            &loader.clickhouse,
            state.last_block_number,
            &["VERIFY_CHECKPOINT"],
        )
        .await?;
    }
    let mut count = 0u64;
    for height in start..=end {
        if !force_replay && owner.is_none() {
            anyhow::ensure!(
                !aml_warehouse::maintenance::blocked(
                    &*loader.clickhouse,
                    loader.clickhouse.database(),
                    aml_warehouse::Network::Tron,
                    height
                )
                .await?,
                "TRON source validation failed: ingestion blocked by a dead letter; use maintenance repair --include-dead"
            );
        }
        if total_txs > 0 && count >= total_txs {
            break;
        }
        let fetched = {
            let _permit = loader.rpc_limiter.acquire().await?;
            loader.tron_client.get_block(height).await
        };
        let source = match fetched {
            Ok(v) => v,
            Err(error) => {
                record_error(&loader.clickhouse, height, "", "", "FETCH_BLOCK", &error).await?;
                return Err(error);
            }
        };
        let header = match super::publication_store::block_header(&source, height, 3) {
            Ok(v) => v,
            Err(error) => {
                record_error(
                    &loader.clickhouse,
                    height,
                    source["blockID"].as_str().unwrap_or(""),
                    "",
                    "VALIDATE_BLOCK",
                    &error,
                )
                .await?;
                return Err(error);
            }
        };
        let ingest =
            match should_ingest_block(&loader.clickhouse, height, &header.block_hash, force_replay)
                .await
            {
                Ok(v) => v,
                Err(error) => {
                    record_error(
                        &loader.clickhouse,
                        height,
                        &header.block_hash,
                        "",
                        "CANONICALITY",
                        &error,
                    )
                    .await?;
                    return Err(error);
                }
            };
        if !ingest {
            resolve_ingestion_failures(
                &loader.clickhouse,
                height,
                &[
                    "VERIFY_CHECKPOINT",
                    "FETCH_BLOCK",
                    "VALIDATE_BLOCK",
                    "CANONICALITY",
                ],
            )
            .await?;
            continue;
        }
        let txs = source
            .get("transactions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if total_txs > 0 && count > 0 && count.saturating_add(txs.len() as u64) > total_txs {
            break;
        }
        loader
            .protocol_registry
            .refresh_if_due(&loader.clickhouse, height)
            .await?;
        let prepared = if is_genesis_block(height) {
            let values = txs
                .iter()
                .map(|tx| PreparedTransaction::genesis(tx, header.block_timestamp_unix_ms))
                .collect::<Result<Vec<_>>>();
            match values {
                Ok(v) => v,
                Err(error) => {
                    record_error(
                        &loader.clickhouse,
                        height,
                        &header.block_hash,
                        "",
                        "GENESIS_SOURCE",
                        &error,
                    )
                    .await?;
                    return Err(error);
                }
            }
        } else {
            let results = stream::iter(txs.into_iter().enumerate())
                .map(|(position, tx)| {
                    let current = loader.clone();
                    let hash = tx["txID"].as_str().unwrap_or("").to_owned();
                    async move {
                        (
                            position,
                            hash,
                            process_tx(current, tx, height, header.block_timestamp_unix_ms).await,
                        )
                    }
                })
                .buffer_unordered(loader.config.tx_worker_concurrency.max(1))
                .collect::<Vec<_>>()
                .await;
            let mut ordered = BTreeMap::new();
            let mut first = None;
            for (position, hash, result) in results {
                match result {
                    Ok(value) => {
                        ordered.insert(position, value);
                    }
                    Err(error) => {
                        record_error(
                            &loader.clickhouse,
                            height,
                            &header.block_hash,
                            &hash,
                            "PROCESS_TX",
                            &error,
                        )
                        .await?;
                        if first.is_none() {
                            first = Some(error);
                        }
                    }
                }
            }
            if let Some(error) = first {
                return Err(error)
                    .with_context(|| format!("failed to prepare complete TRON block {height}"));
            }
            ordered.into_values().collect()
        };
        if let Err(error) = publish_block(
            &loader.clickhouse,
            &source,
            height,
            &prepared,
            latest,
            force_replay,
        )
        .await
        {
            record_error(
                &loader.clickhouse,
                height,
                &header.block_hash,
                "",
                "PUBLISH_BLOCK",
                &error,
            )
            .await?;
            return Err(error)
                .with_context(|| format!("failed to publish complete TRON block {height}"));
        }
        resolve_ingestion_failures(&loader.clickhouse, height, &[]).await?;
        count = count.saturating_add(prepared.len() as u64);
        println!("[TRON] published block {height}; {count} transaction(s) in this pass");
    }
    Ok(())
}

fn is_genesis_block(block_number: u64) -> bool {
    block_number == GENESIS_BLOCK_NUMBER
}

fn validate_replay_range(start_block: u64, end_block: u64) -> Result<()> {
    if end_block < start_block {
        return Err(anyhow!(
            "TRON replay end block {end_block} is below start block {start_block}"
        ));
    }

    let block_count = end_block
        .checked_sub(start_block)
        .and_then(|difference| difference.checked_add(1))
        .ok_or_else(|| anyhow!("TRON replay range overflowed"))?;

    if block_count > MAX_REPLAY_BLOCKS {
        return Err(anyhow!(
            "TRON replay range contains {block_count} blocks; maximum is {MAX_REPLAY_BLOCKS}"
        ));
    }

    Ok(())
}

async fn record_error(
    clickhouse: &crate::db::Client,
    block_number: u64,
    block_hash: &str,
    tx_hash: &str,
    stage: &str,
    error: &anyhow::Error,
) -> Result<()> {
    let (error_class, retryable) = classify_ingestion_error(error);

    record_ingestion_failure(
        clickhouse,
        block_number,
        block_hash,
        tx_hash,
        stage,
        error_class,
        &format!("{error:#}"),
        retryable,
    )
    .await
}

pub(crate) fn classify_ingestion_error(error: &anyhow::Error) -> (&'static str, bool) {
    if error
        .downcast_ref::<crate::helper::tron::RpcHttpError>()
        .is_some()
        || error.downcast_ref::<reqwest::Error>().is_some()
    {
        let transient = crate::helper::tron::transient_rpc_error(error);
        return (
            if transient {
                "RPC_TRANSIENT"
            } else {
                "RPC_PERMANENT"
            },
            transient,
        );
    }
    if error.downcast_ref::<FinalizedHashConflict>().is_some() {
        return ("HASH_CONFLICT", false);
    }

    let message = format!("{error:#}").to_ascii_lowercase();

    if [
        "timeout",
        "timed out",
        "http request",
        "connection",
        "429",
        "frequency",
        "receipt is not available",
    ]
    .iter()
    .any(|needle| message.contains(needle))
    {
        return ("RPC_TRANSIENT", true);
    }

    if [
        "tron source validation",
        "core parent mismatch",
        "noncontiguous core batch",
        "duplicate transaction",
        "duplicate stored fact",
        "refresh cannot",
        "missing",
        "no valid",
        "invalid",
        "decode",
        "deserialize",
        "failed to parse",
        "malformed",
    ]
    .iter()
    .any(|needle| message.contains(needle))
    {
        return ("DATA_VALIDATION", false);
    }

    ("PROCESSING", true)
}

fn transaction_status(tx: &Value, receipt: &Value) -> u8 {
    u8::from(super::source_evidence::transaction_succeeded(tx, receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_block_zero_as_genesis() {
        assert!(is_genesis_block(0));
        assert!(!is_genesis_block(1));
    }
    #[test]
    fn replay_range_is_inclusive_and_bounded() {
        assert!(validate_replay_range(1, 10_000).is_ok());
        assert!(validate_replay_range(1, 10_001).is_err());
        assert!(validate_replay_range(2, 1).is_err());
    }

    #[test]
    fn classifies_missing_receipt_as_retryable_rpc_failure() {
        let error = anyhow!("transaction receipt is not available")
            .context("TRON source validation failed");

        assert_eq!(classify_ingestion_error(&error), ("RPC_TRANSIENT", true));
    }

    #[test]
    fn classifies_invalid_block_data_as_non_retryable() {
        let error = anyhow!("TRON block 10 has no valid timestamp");

        assert_eq!(classify_ingestion_error(&error), ("DATA_VALIDATION", false));
        let mismatch = anyhow!("protobuf/JSON contract type mismatch")
            .context("TRON source validation failed");
        assert_eq!(
            classify_ingestion_error(&mismatch),
            ("DATA_VALIDATION", false)
        );
    }

    #[test]
    fn treats_omitted_receipt_result_as_success() {
        let receipt = serde_json::json!({
            "id": "tx-id",
            "receipt": { "net_fee": 100_000 }
        });

        assert_eq!(transaction_status(&serde_json::json!({}), &receipt), 1);
    }

    #[test]
    fn treats_explicit_success_as_success() {
        let receipt = serde_json::json!({
            "id": "tx-id",
            "receipt": { "result": "SUCCESS" }
        });

        assert_eq!(transaction_status(&serde_json::json!({}), &receipt), 1);
    }

    #[test]
    fn treats_explicit_failure_as_failure() {
        let receipt = serde_json::json!({
            "id": "tx-id",
            "receipt": { "result": "FAILED" }
        });

        assert_eq!(transaction_status(&serde_json::json!({}), &receipt), 0);
    }
}
