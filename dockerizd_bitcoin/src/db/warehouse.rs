//! Bitcoin's core writer, readers and resumable legacy migration use the shared
//! warehouse. The legacy tables are retained only as migration evidence.
use crate::{
    db::{Database, SyncState},
    model::{Evidence, Input, Output, Transaction},
};
use aml_warehouse::{
    Network, RawAmount, UnixMillis,
    publication::{Block, State},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const INPUTS: &str = "SELECT commit_id,block_number AS block_height,block_hash,intDiv(block_timestamp_unix_ms,1000) AS block_time,tx_hash AS txid,transaction_index AS tx_index,input_index,prev_tx_hash AS prev_txid,prev_output_index AS prev_vout,ifNull(address,'') AS address,script_hex,script_type,value_sats FROM aml_bitcoin_inputs_canonical WHERE network_id='bip122:000000000019d6689c085ae165831e93'";
pub const OUTPUTS: &str = "SELECT commit_id,block_number AS block_height,block_hash,intDiv(block_timestamp_unix_ms,1000) AS block_time,tx_hash AS txid,transaction_index AS tx_index,output_index,coinbase,ifNull(address,'') AS address,script_hex,script_type,value_sats FROM aml_bitcoin_outputs_canonical WHERE network_id='bip122:000000000019d6689c085ae165831e93'";

pub fn utxos() -> String {
    // Preserve the occurrence ordering required by historical BIP30 duplicates.
    format!(
        "SELECT o.* FROM (SELECT * FROM ({OUTPUTS}) ORDER BY block_height DESC,tx_index DESC LIMIT 1 BY txid,output_index) AS o LEFT JOIN (SELECT prev_txid,prev_vout,max(tuple(block_height,tx_index)) AS spent_at,toUInt8(1) AS spent FROM ({INPUTS}) GROUP BY prev_txid,prev_vout) AS i ON o.txid=i.prev_txid AND o.output_index=i.prev_vout WHERE ifNull(i.spent,0)=0 OR i.spent_at<tuple(o.block_height,o.tx_index)"
    )
}

pub fn from_state(state: State) -> SyncState {
    SyncState {
        id: 0,
        revision: state.revision,
        initialized: state.initialized,
        start_height: state.start_block,
        tip_height: state.last_block_number,
        tip_hash: state.last_block_hash,
        observed_rpc_height: state.observed_rpc_height,
        published_at: state.published_at_unix_ms,
    }
}
pub fn to_state(state: &SyncState) -> State {
    State {
        network_id: crate::NETWORK_ID.into(),
        revision: state.revision,
        initialized: state.initialized,
        start_block: state.start_height,
        last_block_number: state.tip_height,
        last_block_hash: state.tip_hash.clone(),
        observed_rpc_height: state.observed_rpc_height,
        published_at_unix_ms: state.published_at,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LegacyBlock {
    pub block_height: u64,
    pub block_hash: String,
    pub parent_hash: String,
    pub block_time: u64,
    pub commit_id: String,
    pub transaction_count: u32,
}

/// Validate exact transaction/leg counts, ordering and conservation before any
/// publication. This also rejects incomplete or inconsistent legacy evidence.
pub fn validate_evidence(block: &LegacyBlock, evidence: &Evidence) -> Result<()> {
    crate::model::hash(&block.block_hash)?;
    if block.block_height > 0 {
        crate::model::hash(&block.parent_hash)?;
    }
    UnixMillis::from_seconds(block.block_time)?;
    ensure!(
        evidence.transactions.len() == block.transaction_count as usize
            && block.transaction_count > 0,
        "transaction count differs from complete block"
    );
    let mut transactions = HashMap::new();
    for (index, tx) in evidence.transactions.iter().enumerate() {
        crate::model::hash(&tx.txid)?;
        ensure!(
            tx.tx_index as usize == index && tx.coinbase == u8::from(index == 0),
            "invalid transaction ordering/coinbase"
        );
        ensure!(
            transactions.insert(tx.txid.as_str(), tx).is_none(),
            "duplicate transaction hash within block"
        );
    }
    let header = |commit: &str, height: u64, hash: &str, time: u64| -> Result<()> {
        ensure!(
            commit == block.commit_id
                && height == block.block_height
                && hash == block.block_hash
                && time == block.block_time,
            "evidence header does not match source block"
        );
        Ok(())
    };
    let mut input_totals = HashMap::<&str, (u32, u64)>::new();
    let mut output_totals = HashMap::<&str, (u32, u64)>::new();
    let mut spent = HashSet::new();
    for input in &evidence.inputs {
        input.output.validate()?;
        crate::model::hash(&input.prev_txid)?;
        header(
            &input.commit_id,
            input.block_height,
            &input.block_hash,
            input.block_time,
        )?;
        let tx = transactions
            .get(input.txid.as_str())
            .context("input without transaction")?;
        ensure!(
            tx.tx_index == input.tx_index && tx.coinbase == 0,
            "input transaction mismatch"
        );
        ensure!(
            spent.insert((&input.prev_txid, input.prev_vout)),
            "duplicate spent outpoint"
        );
        let total = input_totals.entry(&input.txid).or_default();
        ensure!(input.input_index == total.0, "input index gap/duplicate");
        total.0 += 1;
        total.1 = total
            .1
            .checked_add(input.output.value_sats)
            .context("input total overflow")?;
        RawAmount::from_satoshis(input.output.value_sats)?;
    }
    for output in &evidence.outputs {
        output.output.validate()?;
        header(
            &output.commit_id,
            output.block_height,
            &output.block_hash,
            output.block_time,
        )?;
        let tx = transactions
            .get(output.txid.as_str())
            .context("output without transaction")?;
        ensure!(
            tx.tx_index == output.tx_index && tx.coinbase == output.coinbase,
            "output transaction mismatch"
        );
        let total = output_totals.entry(&output.txid).or_default();
        ensure!(output.output_index == total.0, "output index gap/duplicate");
        total.0 += 1;
        total.1 = total
            .1
            .checked_add(output.output.value_sats)
            .context("output total overflow")?;
        RawAmount::from_satoshis(output.output.value_sats)?;
    }
    for tx in &evidence.transactions {
        header(
            &tx.commit_id,
            tx.block_height,
            &tx.block_hash,
            tx.block_time,
        )?;
        RawAmount::from_satoshis(tx.output_sats)?;
        RawAmount::from_satoshis(tx.fee_sats)?;
        let inputs = input_totals
            .get(tx.txid.as_str())
            .copied()
            .unwrap_or_default();
        let outputs = output_totals
            .get(tx.txid.as_str())
            .copied()
            .unwrap_or_default();
        ensure!(
            outputs == (tx.output_count, tx.output_sats),
            "output totals differ"
        );
        // The legacy input_count includes the coinbase vin, which has no prevout.
        ensure!(
            inputs.0 + u32::from(tx.coinbase) == tx.input_count,
            "input count differs"
        );
        ensure!(
            if tx.coinbase == 1 {
                tx.fee_sats == 0
            } else {
                inputs.1.checked_sub(outputs.1) == Some(tx.fee_sats)
            },
            "transaction amount conservation failed"
        );
    }
    Ok(())
}

pub fn map_evidence(
    block: &Block,
    evidence: &Evidence,
) -> Result<BTreeMap<&'static str, Vec<Value>>> {
    let header = json!({"network_id":block.network_id,"block_number":block.block_number,"block_hash":block.block_hash,"block_timestamp_unix_ms":block.block_timestamp_unix_ms,"commit_id":block.commit_id,"revision":block.revision,"created_at_unix_ms":block.created_at_unix_ms});
    let with_header = |mut row: Value| {
        row.as_object_mut()
            .unwrap()
            .extend(header.as_object().unwrap().clone());
        row
    };
    let mut tables = BTreeMap::from([
        ("aml_transactions", Vec::new()),
        ("aml_bitcoin_inputs", Vec::new()),
        ("aml_bitcoin_outputs", Vec::new()),
        ("aml_address_relationships", Vec::new()),
        (
            "aml_semantic_aml_events",
            crate::semantics::block_facts(block, evidence)?,
        ),
    ]);
    for tx in &evidence.transactions {
        tables.get_mut("aml_transactions").unwrap().push(with_header(json!({
            "tx_hash":tx.txid,"transaction_index":tx.tx_index,"from_address":null,"to_address":null,
            "amount_raw":null,"fee_raw":tx.fee_sats.to_string(),"asset_id":Network::Bitcoin.native_asset(),"decimals":Network::Bitcoin.native_decimals(),
            "status":"confirmed","coinbase":tx.coinbase,"input_count":tx.input_count,"output_count":tx.output_count,
            "raw_evidence_json":serde_json::to_string(tx)?
        })));
    }
    for input in &evidence.inputs {
        let address = (!input.output.address.is_empty()).then_some(&input.output.address);
        let transaction = aml_warehouse::relationships::transaction_subject_id(
            Network::Bitcoin,
            &block.block_hash,
            &input.txid,
        )?;
        tables.get_mut("aml_bitcoin_inputs").unwrap().push(with_header(json!({"tx_hash":input.txid,"transaction_index":input.tx_index,"input_index":input.input_index,"address":address,"script_hex":input.output.script_hex,"script_type":input.output.script_type,"value_sats":input.output.value_sats,"prev_tx_hash":input.prev_txid,"prev_output_index":input.prev_vout})));
        tables.get_mut("aml_address_relationships").unwrap().push(with_header(json!({
            "relationship_id":format!("{}:{}:in:{}",block.block_hash,input.txid,input.input_index),"tx_hash":input.txid,"transaction_index":input.tx_index,"event_index":input.input_index,
            "from_subject_id":input.output.subject(),"from_subject_type":if address.is_some(){"address"}else{"script"},"to_subject_id":transaction,"to_subject_type":"transaction",
            "from_address":address,"to_address":null,"asset_id":Network::Bitcoin.native_asset(),"amount_raw":input.output.value_sats.to_string(),"decimals":Network::Bitcoin.native_decimals(),
            "transfer_type":"utxo_input","attribution_method":"observed_prevout","confidence":1.0,"prev_tx_hash":input.prev_txid,"prev_output_index":input.prev_vout,"output_index":null,
            "evidence_refs":[format!("outpoint:{}:{}",input.prev_txid,input.prev_vout)],
            "event_sub_index":0,"trace_address":[],"token_id":null,"raw_evidence_json":serde_json::to_string(input)?
        })));
    }
    for output in &evidence.outputs {
        let address = (!output.output.address.is_empty()).then_some(&output.output.address);
        let transaction = aml_warehouse::relationships::transaction_subject_id(
            Network::Bitcoin,
            &block.block_hash,
            &output.txid,
        )?;
        tables.get_mut("aml_bitcoin_outputs").unwrap().push(with_header(json!({"tx_hash":output.txid,"transaction_index":output.tx_index,"output_index":output.output_index,"coinbase":output.coinbase,"address":address,"script_hex":output.output.script_hex,"script_type":output.output.script_type,"value_sats":output.output.value_sats})));
        tables.get_mut("aml_address_relationships").unwrap().push(with_header(json!({
            "relationship_id":format!("{}:{}:out:{}",block.block_hash,output.txid,output.output_index),"tx_hash":output.txid,"transaction_index":output.tx_index,"event_index":output.output_index,
            "from_subject_id":transaction,"from_subject_type":"transaction","to_subject_id":output.output.subject(),"to_subject_type":if address.is_some(){"address"}else{"script"},
            "from_address":null,"to_address":address,"asset_id":Network::Bitcoin.native_asset(),"amount_raw":output.output.value_sats.to_string(),"decimals":Network::Bitcoin.native_decimals(),
            "transfer_type":"utxo_output","attribution_method":"observed_output","confidence":1.0,"prev_tx_hash":null,"prev_output_index":null,"output_index":output.output_index,
            "evidence_refs":[format!("outpoint:{}:{}",output.txid,output.output_index)],
            "event_sub_index":0,"trace_address":[],"token_id":null,"raw_evidence_json":serde_json::to_string(output)?
        })));
    }
    let contract = aml_warehouse::current_contract()?;
    for (table, rows) in &tables {
        for row in rows {
            if *table == "aml_address_relationships" {
                aml_warehouse::relationships::validate_with_contract(&contract, row)?;
            } else {
                aml_warehouse::semantics::validate_row(&contract, table, row)?;
            }
        }
    }
    Ok(tables)
}

pub use crate::storage::writer::{WriteContext, verify_rows, write_block};

pub async fn migration_ready(db: &Database) -> Result<()> {
    let legacy = db.legacy_state().await?;
    if legacy.initialized == 0 {
        return Ok(());
    }
    let current = db.state().await?;
    // A completed backfill remains completed after a legitimate later reorg.
    // Its original marker is retained as evidence even if the tip rolls back.
    let receipts: Vec<Value> = db.rows("SELECT count() AS n FROM aml_ingested_blocks FINAL WHERE network_id={network:String} AND block_number={height:UInt64} AND block_hash={hash:String} AND source_id={source:String} AND ingestion_status='complete' AND canonical=1 AND revision<={revision:UInt64}",&[("network",crate::NETWORK_ID.into()),("height",legacy.tip_height.to_string()),("hash",legacy.tip_hash.clone()),("source",format!("bitcoin_legacy_backfill_v1:{}",legacy.revision)),("revision",current.revision.to_string())]).await?;
    ensure!(
        current.start_height == legacy.start_height && receipts[0]["n"].as_u64().unwrap_or(0) > 0,
        "Bitcoin legacy history must be migrated: stop ingestion, run bitcoin_aml warehouse-backfill --max-blocks 1000 until complete"
    );
    Ok(())
}

pub async fn backfill(db: &Database, max_blocks: u64) -> Result<Value> {
    let _owner = crate::writer_lock::WriterLock::for_database(db.database())?;
    ensure!(
        (1..=100_000).contains(&max_blocks),
        "--max-blocks must be 1..100000"
    );
    db.check_schema().await?;
    let legacy = db.legacy_state().await?;
    let mut current = db.state().await?;
    if legacy.initialized == 0 {
        return Ok(json!({"complete":true,"migrated_blocks":0,"legacy_initialized":false}));
    }
    let source_id = format!("bitcoin_legacy_backfill_v1:{}", legacy.revision);
    if current.initialized == 1 {
        ensure!(
            current.start_height == legacy.start_height && current.tip_height <= legacy.tip_height,
            "backfill requires a warehouse prefix of the legacy history"
        );
        let blocks: Vec<LegacyBlock> = db
            .rows(
                "SELECT * FROM bitcoin_blocks_canonical WHERE block_height={height:UInt64}",
                &[("height", current.tip_height.to_string())],
            )
            .await?;
        ensure!(
            blocks
                .first()
                .is_some_and(|b| b.block_hash == current.tip_hash),
            "legacy history changed since the last backfill checkpoint"
        );
        let markers: Vec<Value> = db.rows("SELECT source_id FROM aml_ingested_blocks_canonical WHERE network_id={network:String} AND block_number={height:UInt64}",&[("network",crate::NETWORK_ID.into()),("height",current.tip_height.to_string())]).await?;
        ensure!(
            markers.first().is_some_and(|m| m["source_id"] == source_id),
            "legacy checkpoint changed between backfill runs; restore a consistent snapshot before resuming"
        );
    }
    let mut migrated = 0;
    while migrated < max_blocks
        && (current.initialized == 0 || current.tip_height < legacy.tip_height)
    {
        ensure!(
            db.legacy_state().await? == legacy,
            "legacy checkpoint changed; stop all writers before backfill"
        );
        let height = if current.initialized == 1 {
            current
                .tip_height
                .checked_add(1)
                .context("height exhausted")?
        } else {
            legacy.start_height
        };
        let mut blocks: Vec<LegacyBlock> = db
            .rows(
                "SELECT * FROM bitcoin_blocks_canonical WHERE block_height={height:UInt64}",
                &[("height", height.to_string())],
            )
            .await?;
        ensure!(blocks.len() == 1, "missing/ambiguous legacy block {height}");
        let block = blocks.pop().unwrap();
        let params = [("commit", block.commit_id.clone())];
        let evidence = Evidence {
            transactions:db.rows::<Transaction>("SELECT * FROM bitcoin_transactions FINAL WHERE commit_id={commit:String} ORDER BY tx_index",&params).await?,
            inputs:db.rows::<Input>("SELECT * FROM bitcoin_inputs FINAL WHERE commit_id={commit:String} ORDER BY tx_index,input_index",&params).await?,
            outputs:db.rows::<Output>("SELECT * FROM bitcoin_outputs FINAL WHERE commit_id={commit:String} ORDER BY tx_index,output_index",&params).await?,
        };
        ensure!(
            db.legacy_state().await? == legacy,
            "legacy checkpoint changed while loading evidence"
        );
        current = write_block(
            db,
            &current,
            &block,
            &evidence,
            WriteContext {
                observed: legacy.observed_rpc_height,
                start: legacy.start_height,
                source_id: &source_id,
                legacy_fence: Some(&legacy),
            },
        )
        .await?;
        migrated += 1;
    }
    ensure!(
        db.legacy_state().await? == legacy,
        "legacy checkpoint changed during backfill"
    );
    Ok(
        json!({"network_id":Network::Bitcoin.id(),"complete":current.initialized==1 && current.tip_height==legacy.tip_height,"migrated_blocks":migrated,"start_block":current.start_height,"last_block_number":current.tip_height,"last_block_hash":current.tip_hash,"source_revision":legacy.revision.to_string(),"warehouse_revision":current.revision.to_string(),"verification":"all_core_columns_and_exact_amounts"}),
    )
}
