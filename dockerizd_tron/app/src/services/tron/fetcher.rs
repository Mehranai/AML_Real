use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow};
use clickhouse::types::UInt256;
use futures::stream::{self, StreamExt};
use serde_json::Value;

use crate::models::tron::modules::TransactionRow;

use crate::progress::core::save_sync_state;

use crate::models::tron::modules::TransactionFeatureRow;

use crate::services::loader::LoaderTron;
use crate::services::tron::ingestion_state::{
    FinalizedHashConflict, record_failed_block, record_ingested_block, record_ingestion_failure,
    record_processing_block, resolve_ingestion_failures, should_ingest_block,
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
    TransferKind, extract_contract_transfers, extract_internal_transfers, extract_trc20_transfers,
    has_contract_call, primary_contract_summary, primary_method_data, standard_transfer_evidence,
};
use chrono::Utc;

use crate::services::tron::aml::mint_burn_detector::detect_mints_and_burns;
use crate::services::tron::relationship_builder::build_relationships;

const ZERO_ADDRESS: &str = "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb";
const GENESIS_BLOCK_NUMBER: u64 = 0;
const MAX_REPLAY_BLOCKS: u64 = 10_000;

async fn process_tx(loader: Arc<LoaderTron>, tx: Value, block_number: u64) -> Result<()> {
    let txid = tx["txID"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing txID"))?
        .to_string();

    let (contract_type, initiator_address, target_address, contract_address, _) =
        primary_contract_summary(&tx);
    let mut canonical_transfers = extract_contract_transfers(&tx, &txid);

    let receipt = {
        let _permit = loader.rpc_limiter.acquire().await?;

        loader.tron_client.get_tx_receipt(&txid).await?
    };

    if receipt["id"].as_str().is_none() {
        return Err(anyhow!("transaction receipt is not available for {txid}"));
    }

    let timestamp = tx["raw_data"]["timestamp"]
        .as_u64()
        .filter(|timestamp| *timestamp > 0)
        .ok_or_else(|| anyhow!("transaction {txid} has no valid timestamp"))?;

    let status = transaction_status(&receipt);

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

    let semantic_transfers = canonical_transfers
        .iter()
        .map(|transfer| transfer.as_simple_transfer())
        .collect::<Vec<SimpleTransfer>>();
    let simple_transfers = semantic_transfers
        .iter()
        .filter(|transfer| transfer.from != ZERO_ADDRESS && transfer.to != ZERO_ADDRESS)
        .cloned()
        .collect::<Vec<_>>();

    // tx_hash: شناسه تراکنش.
    // block_number: شماره بلاک.
    // timestamp: زمان شبکه.
    // initiator_address: امضاکننده یا شروع‌کننده.
    // target_address: مقصد اصلی contract.
    // contract_address: قرارداد هوشمند درگیر.
    // contract_type: نوع contract در TRON.
    // fee: هزینه تراکنش.
    // energy_usage_total: مصرف Energy.
    // net_usage: مصرف Bandwidth.
    // status: موفق یا ناموفق بودن اجرا.

    loader
        .transaction_batcher
        .push(TransactionRow {
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
        })
        .await?;

    let mut discovered_tokens = HashSet::<String>::new();

    for transfer in &canonical_transfers {
        if transfer.kind == TransferKind::Trc20 {
            discovered_tokens.insert(transfer.asset_id.clone());
        }
    }

    // token metadata worker
    if !discovered_tokens.is_empty() {
        let updated_at_unix_ms = Utc::now().timestamp_millis().max(0) as u64;
        for token_address in discovered_tokens {
            loader
                .token_metadata_discovery_batcher
                .push(crate::models::tron::modules::TokenMetadataDiscoveryRow {
                    token_address,
                    discovered_block: block_number,
                    discovered_at_unix_ms: updated_at_unix_ms,
                })
                .await?;
        }
    }

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
            loader.semantic_event_batcher.push(swap.row).await?;
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
            loader.semantic_event_batcher.push(event).await?;
        }

        //     relationship_id: شناسه یکتای edge.
        //     from_address: فرستنده واقعی دارایی.
        //     to_address: گیرنده واقعی دارایی.
        //     token_address: TRX، TRC10 یا قرارداد TRC20.
        //     tx_hash: تراکنش منبع.
        //     block_number: بلاک منبع.
        //     timestamp: زمان انتقال.
        //     amount: مقدار خام و بدون از دست دادن precision.
        //     transfer_type: native، trc10، trc20 یا internal.
        // از این استفاده میکنیم که جریان واقعی fund رو رسم کنیم

        let relationships =
            build_relationships(&txid, block_number, timestamp, &canonical_transfers);

        for row in relationships {
            loader.relationship_batcher.push(row).await?;
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

        loader.transaction_feature_batcher.push(feature).await?;
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
        loader
            .transaction_feature_batcher
            .push(TransactionFeatureRow {
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
            })
            .await?;
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct FetchOptions {
    end_block: Option<u64>,
    force_replay: bool,
    advance_checkpoint: bool,
}

pub async fn fetch_tron(loader: Arc<LoaderTron>, start_block: u64, total_txs: u64) -> Result<()> {
    fetch_tron_with_options(
        loader,
        start_block,
        total_txs,
        FetchOptions {
            end_block: None,
            force_replay: false,
            advance_checkpoint: true,
        },
    )
    .await
}

pub async fn replay_tron_range(
    loader: Arc<LoaderTron>,
    start_block: u64,
    end_block: u64,
) -> Result<()> {
    validate_replay_range(start_block, end_block)?;

    fetch_tron_with_options(
        loader,
        start_block,
        0,
        FetchOptions {
            end_block: Some(end_block),
            force_replay: true,
            advance_checkpoint: false,
        },
    )
    .await
}

pub async fn ingest_tron_range(
    loader: Arc<LoaderTron>,
    start_block: u64,
    end_block: u64,
) -> Result<()> {
    validate_replay_range(start_block, end_block)?;

    fetch_tron_with_options(
        loader,
        start_block,
        0,
        FetchOptions {
            end_block: Some(end_block),
            force_replay: false,
            advance_checkpoint: false,
        },
    )
    .await
}

async fn fetch_tron_with_options(
    loader: Arc<LoaderTron>,
    start_block: u64,
    total_txs: u64,
    options: FetchOptions,
) -> Result<()> {
    let latest_block = loader.tron_client.get_solid_block_number().await?;
    let end_block = options.end_block.unwrap_or(latest_block);

    if end_block > latest_block {
        return Err(anyhow!(
            "requested TRON end block {end_block} is above latest solid block {latest_block}"
        ));
    }

    println!(
        "TRON Latest Solid Block: {} | processing range {}..={}",
        latest_block, start_block, end_block
    );

    let mut tx_count = 0u64;

    let mut current_block = start_block;

    while current_block <= end_block {
        if total_txs > 0 && tx_count >= total_txs {
            break;
        }

        let block_result = {
            let _permit = loader.rpc_limiter.acquire().await?;

            loader.tron_client.get_block(current_block).await
        };
        let block = match block_result {
            Ok(block) => block,
            Err(error) => {
                record_error(
                    &loader.clickhouse,
                    current_block,
                    "",
                    "",
                    "FETCH_BLOCK",
                    &error,
                )
                .await?;
                return Err(error)
                    .with_context(|| format!("failed to fetch TRON block {current_block}"));
            }
        };

        let empty_txs = Vec::new();

        let txs = block["transactions"].as_array().unwrap_or(&empty_txs);
        let block_hash_result = block["blockID"]
            .as_str()
            .filter(|hash| !hash.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("TRON block {current_block} has no blockID"));
        let block_hash = match block_hash_result {
            Ok(block_hash) => block_hash,
            Err(error) => {
                record_error(
                    &loader.clickhouse,
                    current_block,
                    "",
                    "",
                    "VALIDATE_BLOCK",
                    &error,
                )
                .await?;
                return Err(error);
            }
        };
        let parent_hash = block["block_header"]["raw_data"]["parentHash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let block_timestamp = block["block_header"]["raw_data"]["timestamp"]
            .as_u64()
            .unwrap_or_default();

        let should_ingest = match should_ingest_block(
            &loader.clickhouse,
            current_block,
            &block_hash,
            options.force_replay,
        )
        .await
        {
            Ok(should_ingest) => should_ingest,
            Err(error) => {
                let (error_class, retryable) =
                    if error.downcast_ref::<FinalizedHashConflict>().is_some() {
                        ("HASH_CONFLICT", false)
                    } else {
                        classify_ingestion_error(&error)
                    };
                record_ingestion_failure(
                    &loader.clickhouse,
                    current_block,
                    &block_hash,
                    "",
                    "CANONICALITY",
                    error_class,
                    &format!("{error:#}"),
                    retryable,
                )
                .await?;
                return Err(error);
            }
        };

        if !should_ingest {
            resolve_ingestion_failures(&loader.clickhouse, current_block).await?;

            if options.advance_checkpoint {
                save_sync_state(loader.clickhouse.clone(), current_block).await?;
            }
            current_block += 1;
            continue;
        }

        record_processing_block(
            &loader.clickhouse,
            current_block,
            block_hash.clone(),
            parent_hash.clone(),
            block_timestamp,
            txs.len() as u32,
        )
        .await?;

        if is_genesis_block(current_block) {
            println!(
                "[TRON] block 0 is the genesis block; recording {} allocation transaction(s) without runtime receipts or fund-flow edges",
                txs.len()
            );

            record_ingested_block(
                &loader.clickhouse,
                current_block,
                block_hash,
                parent_hash,
                block_timestamp,
                txs.len() as u32,
            )
            .await?;

            if options.advance_checkpoint {
                save_sync_state(loader.clickhouse.clone(), current_block).await?;
            }

            current_block += 1;
            continue;
        }

        if txs.is_empty() {
            println!("[TRON] block {} has 0 transaction(s)", current_block);

            record_ingested_block(
                &loader.clickhouse,
                current_block,
                block_hash,
                parent_hash,
                block_timestamp,
                0,
            )
            .await?;

            if options.advance_checkpoint {
                save_sync_state(loader.clickhouse.clone(), current_block).await?;
            }

            current_block += 1;

            continue;
        }

        if total_txs > 0 && tx_count > 0 && tx_count.saturating_add(txs.len() as u64) > total_txs {
            println!(
                "[TRON] stopping before block {} to preserve block-level checkpoint integrity",
                current_block
            );
            break;
        }

        if loader
            .protocol_registry
            .refresh_if_due(&loader.clickhouse, current_block)
            .await?
        {
            println!(
                "[TRON] refreshed approved protocol registry at block {}",
                current_block
            );
        }

        let tx_vec = txs.to_vec();
        let block_tx_total = tx_vec.len() as u64;

        println!(
            "[TRON] block {} fetched {} transaction(s); processing {} transaction(s)",
            current_block,
            txs.len(),
            block_tx_total
        );

        tx_count += block_tx_total;

        let processed_in_block = Arc::new(AtomicU64::new(0));

        // شروع بخش پردازش مواردی که از بلاک گرفتیم
        // همه تراکنش های بلاک جمع میشوند بعد ذخیره میشوند
        // حتی اگه یکی ذخیره نشه نتیجه خطا میده
        let tx_errors = stream::iter(tx_vec)
            .map(|tx| {
                let loader_clone = loader.clone();
                let tx_hash = tx["txID"].as_str().unwrap_or_default().to_string();

                async move { (tx_hash, process_tx(loader_clone, tx, current_block).await) }
            })
            .buffer_unordered(loader.config.tx_worker_concurrency)
            .filter_map(|(tx_hash, res)| {
                let processed_in_block = processed_in_block.clone();

                async move {
                    let processed = processed_in_block.fetch_add(1, Ordering::Relaxed) + 1;

                    match res {
                        Ok(()) => {
                            if processed == 1
                                || processed.is_multiple_of(10)
                                || processed == block_tx_total
                            {
                                println!(
                                    "[TRON] block {} processed {}/{} transaction(s)",
                                    current_block, processed, block_tx_total
                                );
                            }

                            None
                        }
                        Err(err) => {
                            eprintln!(
                                "[TRON TX ERROR] block {} processed {}/{} transaction(s): {:?}",
                                current_block, processed, block_tx_total, err
                            );

                            Some((tx_hash, err))
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
            .await;

        if !tx_errors.is_empty() {
            let mut first_error = None;

            for (tx_hash, error) in tx_errors {
                record_error(
                    &loader.clickhouse,
                    current_block,
                    &block_hash,
                    &tx_hash,
                    "PROCESS_TX",
                    &error,
                )
                .await?;

                if first_error.is_none() {
                    first_error = Some(error);
                }
            }

            if let Err(flush_error) = loader.flush_batches().await {
                record_error(
                    &loader.clickhouse,
                    current_block,
                    &block_hash,
                    "",
                    "FLUSH_AFTER_TX_FAILURE",
                    &flush_error,
                )
                .await?;
            }

            let first_error = first_error.expect("transaction errors are not empty");
            record_failed_block(
                &loader.clickhouse,
                current_block,
                block_hash,
                parent_hash,
                block_timestamp,
                block_tx_total as u32,
                format!("{first_error:#}"),
            )
            .await?;

            return Err(first_error)
                .with_context(|| format!("failed to process TRON block {current_block}"));
        }

        if let Err(error) = loader.flush_batches().await {
            record_error(
                &loader.clickhouse,
                current_block,
                &block_hash,
                "",
                "FLUSH_BLOCK",
                &error,
            )
            .await?;
            record_failed_block(
                &loader.clickhouse,
                current_block,
                block_hash,
                parent_hash,
                block_timestamp,
                block_tx_total as u32,
                format!("{error:#}"),
            )
            .await?;
            return Err(error)
                .with_context(|| format!("failed to flush TRON block {current_block}"));
        }

        record_ingested_block(
            &loader.clickhouse,
            current_block,
            block_hash,
            parent_hash,
            block_timestamp,
            block_tx_total as u32,
        )
        .await?;

        if options.advance_checkpoint {
            save_sync_state(loader.clickhouse.clone(), current_block).await?;
        }

        println!(
            "TRON synced block {} | total tx {}",
            current_block, tx_count
        );

        current_block += 1;
    }

    loader.flush_batches().await?;

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
    clickhouse: &clickhouse::Client,
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

fn classify_ingestion_error(error: &anyhow::Error) -> (&'static str, bool) {
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
        "missing",
        "no valid",
        "invalid",
        "decode",
        "deserialize",
        "malformed",
    ]
    .iter()
    .any(|needle| message.contains(needle))
    {
        return ("DATA_VALIDATION", false);
    }

    ("PROCESSING", true)
}

fn transaction_status(receipt: &Value) -> u8 {
    // TRON's protobuf JSON omits default enum values. For TransactionInfo::Receipt,
    // an omitted result is SUCCESS; failed executions include an explicit result.
    match receipt
        .get("receipt")
        .and_then(|receipt| receipt.get("result"))
    {
        None => 1,
        Some(Value::String(result)) if result == "SUCCESS" => 1,
        Some(_) => 0,
    }
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
        let error = anyhow!("transaction receipt is not available");

        assert_eq!(classify_ingestion_error(&error), ("RPC_TRANSIENT", true));
    }

    #[test]
    fn classifies_invalid_block_data_as_non_retryable() {
        let error = anyhow!("TRON block 10 has no valid timestamp");

        assert_eq!(classify_ingestion_error(&error), ("DATA_VALIDATION", false));
    }

    #[test]
    fn treats_omitted_receipt_result_as_success() {
        let receipt = serde_json::json!({
            "id": "tx-id",
            "receipt": { "net_fee": 100_000 }
        });

        assert_eq!(transaction_status(&receipt), 1);
    }

    #[test]
    fn treats_explicit_success_as_success() {
        let receipt = serde_json::json!({
            "id": "tx-id",
            "receipt": { "result": "SUCCESS" }
        });

        assert_eq!(transaction_status(&receipt), 1);
    }

    #[test]
    fn treats_explicit_failure_as_failure() {
        let receipt = serde_json::json!({
            "id": "tx-id",
            "receipt": { "result": "FAILED" }
        });

        assert_eq!(transaction_status(&receipt), 0);
    }
}
