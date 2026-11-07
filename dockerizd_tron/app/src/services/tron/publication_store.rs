//! One complete, verified TRON block -> shared facts -> publication fence.
use super::{
    source_evidence::VerifiedTransaction,
    transfer_extractor::{ExtractedTransfer, TransferKind},
};
use crate::{
    db::{Client, now_ms, row},
    models::tron::modules::{SemanticAmlEventRow, TransactionFeatureRow, TransactionRow},
};
use aml_warehouse::{
    Network, core,
    publication::{self, Block},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
};

pub(crate) struct PreparedTransaction {
    pub source: VerifiedTransaction,
    pub transaction: TransactionRow,
    pub transfers: Vec<ExtractedTransfer>,
    pub events: Vec<SemanticAmlEventRow>,
    pub feature: Option<TransactionFeatureRow>,
    pub asset_resolutions: BTreeMap<String, Value>,
}
impl PreparedTransaction {
    pub fn genesis(tx: &Value, timestamp: u64) -> Result<Self> {
        let source = VerifiedTransaction::genesis(tx, timestamp)?;
        let transaction = TransactionRow {
            tx_hash: source.tx_hash().into(),
            block_number: 0,
            timestamp,
            initiator_address: String::new(),
            target_address: String::new(),
            contract_address: String::new(),
            contract_type: "GenesisAllocation".into(),
            fee: 0u64.into(),
            energy_usage_total: 0,
            net_usage: 0,
            status: 0,
        };
        Ok(Self {
            source,
            transaction,
            transfers: vec![],
            events: vec![],
            feature: None,
            asset_resolutions: BTreeMap::new(),
        })
    }
}
pub(crate) fn writer_lock(database: &str) -> Result<File> {
    aml_warehouse::contract::identifier(database)?;
    let root = std::env::var_os("TRON_WRITER_LOCK_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("aml-tron-writer"));
    std::fs::create_dir_all(&root).context("create TRON writer lock directory")?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!("{database}.lock")))?;
    lock.try_lock()
        .context("another TRON ingestor owns this database cursor")?;
    Ok(lock)
}
pub(crate) fn block_header(source: &Value, height: u64, revision: u64) -> Result<Block> {
    let raw = &source["block_header"]["raw_data"];
    // Proto3 may omit number=0 on the genesis header, never another height.
    ensure!(
        raw["number"]
            .as_u64()
            .or_else(|| (height == 0 && raw.get("number").is_none()).then_some(0))
            == Some(height),
        "TRON source validation failed: requested/header block number mismatch"
    );
    let timestamp = raw["timestamp"]
        .as_u64()
        .filter(|v| *v > 0)
        .context("TRON source validation failed: invalid block timestamp")?;
    let hash = aml_warehouse::values::normalize_hash(
        Network::Tron,
        source["blockID"].as_str().context("missing blockID")?,
    )?;
    let parent = if height == 0 && raw.get("parentHash").is_none_or(|v| v.as_str() == Some("")) {
        String::new()
    } else {
        aml_warehouse::values::normalize_hash(
            Network::Tron,
            raw["parentHash"].as_str().context("missing parentHash")?,
        )?
    };
    let count = match source.get("transactions") {
        None => 0,
        Some(v) => u32::try_from(
            v.as_array()
                .context("invalid block transaction array")?
                .len(),
        )?,
    };
    Ok(Block {
        network_id: Network::Tron.id().into(),
        block_number: height,
        block_hash: hash.clone(),
        parent_hash: parent,
        block_timestamp_unix_ms: timestamp,
        commit_id: format!("{revision}-{hash}"),
        revision,
        canonical: 1,
        ingestion_status: "complete".into(),
        transaction_count: count,
        receipt_coverage: (height != 0).then_some(1),
        trace_coverage: None,
        prevout_coverage: None,
        source_id: "tron_rpc".into(),
        created_at_unix_ms: now_ms(),
    })
}
fn asset(transfer: &ExtractedTransfer) -> Result<String> {
    let legacy = &transfer.asset_id;
    let value = if legacy == "TRX" {
        "native:TRX".into()
    } else if let Some(id) = legacy.strip_prefix("TRC10:") {
        ensure!(
            !id.starts_with("legacy:"),
            "TRON source validation failed: pre-ID TRC10 asset name needs an unambiguous verified token-ID mapping; no transfer is discarded or guessed"
        );
        format!("trc10:{}", aml_warehouse::RawAmount::parse(id)?.as_str())
    } else {
        let address = contract_hex(legacy)?;
        match transfer.kind {
            TransferKind::Trc721 => format!(
                "trc721:{address}/{}",
                transfer.token_id.context("NFT token id missing")?
            ),
            _ => format!("trc20:{address}"),
        }
    };
    aml_warehouse::values::AssetId::parse(Network::Tron, &value)?;
    Ok(value)
}
fn contract_hex(value: &str) -> Result<String> {
    let address = aml_warehouse::values::tron_wallet(value)?;
    Ok(crate::utils::tron_address::base58_to_hex(&address)
        .context("validated TRON address conversion failed")?
        .to_ascii_lowercase())
}
fn event_asset(legacy: &str) -> Result<Value> {
    if legacy.is_empty() {
        return Ok(Value::Null);
    }
    let value = if legacy == "TRX" {
        "native:TRX".into()
    } else if let Some(id) = legacy.strip_prefix("TRC10:") {
        format!("trc10:{id}")
    } else {
        format!("trc20:{}", contract_hex(legacy)?)
    };
    aml_warehouse::values::AssetId::parse(Network::Tron, &value)?;
    Ok(json!(value))
}
fn fact(header: &Block, table: &str, mut fields: Value) -> Result<Value> {
    fields.as_object_mut().context("fact object required")?.extend(json!({"network_id":header.network_id,"block_number":header.block_number,"block_hash":header.block_hash,"block_timestamp_unix_ms":header.block_timestamp_unix_ms,"commit_id":header.commit_id,"revision":header.revision,"created_at_unix_ms":header.created_at_unix_ms}).as_object().unwrap().clone());
    row(table, fields, header.revision, header.created_at_unix_ms)
}
pub(crate) fn map_block(
    header: &Block,
    prepared: &[PreparedTransaction],
) -> Result<BTreeMap<String, Vec<Value>>> {
    ensure!(
        prepared.len() == header.transaction_count as usize,
        "prepared transaction coverage mismatch"
    );
    let mut all = BTreeMap::<String, Vec<Value>>::new();
    for name in [
        "aml_transactions",
        "aml_address_relationships",
        "aml_transaction_features",
        "aml_semantic_aml_events",
        "aml_metadata_discoveries",
        "aml_raw_transactions",
        "aml_tron_contracts",
        "aml_tron_logs",
        "aml_tron_internals",
        "aml_tron_resources",
    ] {
        all.insert(name.into(), vec![]);
    }
    let mut discovered = BTreeSet::new();
    for (position, tx) in prepared.iter().enumerate() {
        let index = u32::try_from(position)?;
        let t = &tx.transaction;
        ensure!(
            t.tx_hash == tx.source.tx_hash(),
            "prepared/source transaction identity mismatch"
        );
        for (table, rows) in tx.source.facts(header, index, header.created_at_unix_ms)? {
            all.get_mut(&table)
                .context("unsupported source fact family")?
                .extend(rows);
        }
        let mut raw = serde_json::to_value(t)?;
        raw["asset_resolutions"] = json!(tx.asset_resolutions);
        raw["fee"] = json!(t.fee.to_string());
        for key in ["energy_usage_total", "net_usage"] {
            raw[key] = tx
                .source
                .receipt()
                .and_then(|r| r["receipt"].get(key))
                .cloned()
                .unwrap_or(Value::Null);
        }
        raw["fee_basis"] = json!(if header.block_number == 0 {
            "no_runtime_execution"
        } else if tx.source.receipt().is_some_and(|r| r.get("fee").is_some()) {
            "explicit_rpc_fee"
        } else {
            "transaction_info_proto3_default_zero"
        });
        // A generic transaction is not one arbitrary leg of a multi-contract call.
        // Only a single verified TransferContract has an addressed native amount.
        let contracts = tx.source.raw_transaction()["raw_data"]["contract"]
            .as_array()
            .context("source contracts missing")?;
        let plain = header.block_number > 0
            && contracts.len() == 1
            && contracts[0]["type"] == "TransferContract"
            && tx.source.parameter_verified(0);
        let value = &contracts[0]["parameter"]["value"];
        let wallet = |key: &str| -> Result<Value> {
            Ok(json!(aml_warehouse::values::tron_wallet(
                value[key].as_str().context("verified address missing")?
            )?))
        };
        all.get_mut("aml_transactions").unwrap().push(fact(header,"aml_transactions",json!({"tx_hash":tx.source.tx_hash(),"transaction_index":index,
            "from_address":if plain{wallet("owner_address")?}else{Value::Null},"to_address":if plain{wallet("to_address")?}else{Value::Null},
            "amount_raw":if plain{json!(value["amount"].as_u64().unwrap_or(0).to_string())}else{Value::Null},"fee_raw":t.fee.to_string(),"asset_id":"native:TRX","decimals":6,
            "status":if header.block_number==0{"unknown"}else if tx.source.execution_succeeded(){"success"}else{"failed"},"coinbase":0,"input_count":null,"output_count":null,"raw_evidence_json":raw.to_string()}))?);
        for transfer in &tx.transfers {
            ensure!(
                tx.source.execution_succeeded(),
                "failed/genesis execution cannot publish a successful transfer"
            );
            let asset_id = asset(transfer)?;
            let source_reference = format!(
                "{}:{}:{}",
                tx.source.tx_hash(),
                transfer.source_kind,
                transfer.source_index
            );
            let (source, contract_checked) = match transfer.source_kind {
                "contract" => (
                    contracts
                        .get(transfer.source_index as usize)
                        .context("contract position outside source")?,
                    tx.source.parameter_verified(transfer.source_index as usize),
                ),
                "log" => (
                    tx.source
                        .receipt()
                        .and_then(|r| r["log"].as_array())
                        .and_then(|r| r.get(transfer.source_index as usize))
                        .context("log position outside source")?,
                    true,
                ),
                "internal" => (
                    tx.source
                        .receipt()
                        .and_then(|r| r["internal_transactions"].as_array())
                        .and_then(|r| r.get(transfer.source_index as usize))
                        .context("internal position outside source")?,
                    true,
                ),
                _ => anyhow::bail!("unknown TRON transfer source"),
            };
            ensure!(
                contract_checked,
                "unsupported contract parameters cannot become financial provenance"
            );
            let legacy = if transfer.kind == TransferKind::Trc721 {
                asset_id.clone()
            } else {
                transfer.asset_id.clone()
            };
            all.get_mut("aml_address_relationships").unwrap().push(fact(header,"aml_address_relationships",json!({"relationship_id":transfer.transfer_id,"tx_hash":tx.source.tx_hash(),"transaction_index":index,"event_index":transfer.source_index,"event_sub_index":transfer.source_sub_index,"trace_address":[],
                "from_subject_id":transfer.from_address,"from_subject_type":"address","to_subject_id":transfer.to_address,"to_subject_type":"address","from_address":transfer.from_address,"to_address":transfer.to_address,"asset_id":asset_id,"token_id":transfer.token_id.map(|n|n.to_string()),"amount_raw":transfer.amount.to_string(),"decimals":if transfer.asset_id=="TRX"{json!(6)}else{Value::Null},"transfer_type":transfer.kind.relationship_type(),"attribution_method":"observed_transfer","confidence":1.0,"prev_tx_hash":null,"prev_output_index":null,"output_index":null,
                "evidence_refs":[format!("tx:{}",tx.source.tx_hash()),format!("tron_{}:{source_reference}",transfer.source_kind)],"raw_evidence_json":json!({"source_kind":transfer.source_kind,"source_index":transfer.source_index,"source_sub_index":transfer.source_sub_index,"legacy_asset_id":legacy,"original":source,"asset_resolutions":tx.asset_resolutions}).to_string()}))?);
            if matches!(transfer.kind, TransferKind::Trc20 | TransferKind::Trc721) {
                let collection = asset_id.split('/').next().unwrap();
                if discovered.insert(collection.to_owned()) {
                    all.get_mut("aml_metadata_discoveries").unwrap().push(fact(header,"aml_metadata_discoveries",json!({"discovery_id":format!("{}:{collection}",header.block_hash),"tx_hash":tx.source.tx_hash(),"transaction_index":index,"asset_id":collection,"standard_hint":if transfer.kind==TransferKind::Trc721{"trc721"}else{"trc20"},"evidence_id":source_reference,"source_id":header.source_id,"evidence_refs":[format!("tx:{}",tx.source.tx_hash())],"raw_evidence_json":json!({"token_address":transfer.asset_id,"log_index":transfer.source_index}).to_string()}))?);
                }
            }
        }
        if let Some(feature) = &tx.feature {
            all.get_mut("aml_transaction_features").unwrap().push(fact(header,"aml_transaction_features",json!({"feature_id":format!("{}:tron_semantics_v2",tx.source.tx_hash()),"tx_hash":tx.source.tx_hash(),"subject_id":if t.initiator_address.is_empty(){tx.source.tx_hash()}else{&t.initiator_address},"detector_version":"tron_semantics_v2","features_json":serde_json::to_string(feature)?,"evidence_refs":[format!("tx:{}",tx.source.tx_hash())],"raw_evidence_json":serde_json::to_string(feature)?}))?);
        }
        for (event_position, event) in tx.events.iter().enumerate() {
            let mut evidence: Value = serde_json::from_str(&event.evidence_json)?;
            evidence["publication_event_position"] = json!(event_position);
            let event_index = evidence["log_index"]
                .as_u64()
                .unwrap_or(event_position as u64);
            all.get_mut("aml_semantic_aml_events").unwrap().push(fact(header,"aml_semantic_aml_events",json!({"event_id":event.event_id,"tx_hash":tx.source.tx_hash(),"subject_id":event.subject_address,"event_type":event.event_type,"protocol":event.protocol,"detector_version":event.detector_version,"confidence":f64::from(event.confidence),"asset_in":event_asset(&event.asset_in)?,"asset_out":event_asset(&event.asset_out)?,"amount_in":null,"amount_out":null,"remote_network_id":null,"evidence_refs":[format!("tx:{}",tx.source.tx_hash())],"evidence_json":evidence.to_string(),"event_index":u32::try_from(event_index)?,"event_sub_index":u32::try_from(event_position)?,"detector":event.detector,"raw_evidence_json":serde_json::to_string(event)?}))?);
        }
    }
    Ok(all)
}
pub(crate) async fn publish_block(
    client: &Client,
    source: &Value,
    height: u64,
    prepared: &[PreparedTransaction],
    observed: u64,
    refresh: bool,
) -> Result<()> {
    let previous = publication::state(client, client.database(), Network::Tron).await?;
    let revision =
        publication::next_revision(client, client.database(), Network::Tron, previous.revision)
            .await?;
    let header = block_header(source, height, revision)?;
    let facts = map_block(&header, prepared).context("TRON source validation failed")?;
    if refresh && previous.initialized == 1 && height <= previous.last_block_number {
        core::refresh_observed_batch(
            client,
            client.database(),
            &previous,
            &header,
            &facts,
            &source.to_string(),
            observed,
        )
        .await?;
    } else {
        core::publish_observed_batch(
            client,
            client.database(),
            &previous,
            &header,
            &facts,
            &source.to_string(),
            observed,
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/support/live_ingestion.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::sync_state::get_last_synced_block,
        services::{loader::LoaderTron, tron::fetcher::ingest_tron_range},
    };
    use std::sync::Arc;
    #[tokio::test]
    #[ignore = "requires a fresh disposable WAREHOUSE_TEST_URL"]
    async fn shared_writer_recovers_at_six_physical_write_boundaries() -> Result<()> {
        let rpc = fixture::Rpc::start().await?;
        let (config, client) = fixture::setup("faults", &rpc).await?;
        for phase in 1..=6 {
            let height = 99 + u64::from(phase);
            let tx = fixture::simple(height, height);
            rpc.block(height, vec![tx.clone()]);
            rpc.receipt(height, &tx, false);
            let before = publication::state(&*client, client.database(), Network::Tron).await?;
            let interrupted = Arc::new(LoaderTron::new(&config).await?);
            interrupted.clickhouse.fail_once(phase);
            let error = ingest_tron_range(interrupted, height, height)
                .await
                .unwrap_err();
            ensure!(
                format!("{error:#}").contains("injected TRON publication interruption"),
                "unexpected failure before injected phase {phase}: {error:#}"
            );
            assert_eq!(
                publication::state(&*client, client.database(), Network::Tron).await?,
                before,
                "cursor changed at interrupted phase {phase}"
            );
            assert_eq!(fixture::scalar(&client,&format!("SELECT toString(count()) FROM aml_transactions_canonical WHERE block_number={height}")).await?,0);
            let fresh = Arc::new(LoaderTron::new(&config).await?);
            ingest_tron_range(fresh, height, height).await?;
            assert_eq!(get_last_synced_block(&client).await?, Some(height));
            assert_eq!(fixture::scalar(&client,&format!("SELECT toString(count()) FROM aml_transactions_canonical WHERE block_number={height}")).await?,1);
            assert_eq!(fixture::scalar(&client,&format!("SELECT toString(count()) FROM aml_ingestion_failures_current WHERE block_number={height} AND status!='resolved'")).await?,0);
        }
        let guard = writer_lock(client.database())?;
        assert!(
            ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 106, 106)
                .await
                .unwrap_err()
                .to_string()
                .contains("another TRON ingestor")
        );
        drop(guard);
        Ok(())
    }
    #[test]
    fn writer_lock_child() {
        if let Ok(database) = std::env::var("TRON_TEST_LOCK_DATABASE") {
            assert!(
                writer_lock(&database).is_err(),
                "child process bypassed cursor lock"
            );
        }
    }
    #[test]
    fn cursor_lock_is_shared_between_processes_and_released_on_drop() -> Result<()> {
        let database = format!("tron_lock_{}", std::process::id());
        let guard = writer_lock(&database)?;
        let status = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "services::tron::publication_store::tests::writer_lock_child",
                "--nocapture",
            ])
            .env("TRON_TEST_LOCK_DATABASE", &database)
            .status()?;
        ensure!(
            status.success(),
            "competing writer process did not respect ownership"
        );
        drop(guard);
        let _released = writer_lock(&database)?;
        Ok(())
    }
}
