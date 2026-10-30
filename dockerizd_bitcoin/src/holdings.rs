//! Exact address-script output states; no private wallet RPC or ownership claim.
use crate::{NETWORK_ID, assets, db::Database, model};
use aml_warehouse::visibility;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
const SQL: &str = include_str!("../sql/queries/holdings.sql");
const SETTINGS: &str = "SETTINGS max_execution_time=15,max_threads=2,max_memory_usage=268435456";
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub limit: usize,
}
impl Default for Query {
    fn default() -> Self {
        Self { limit: 25 }
    }
}
impl Query {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=100).contains(&self.limit),
            "holdings limit must be 1..100"
        );
        Ok(())
    }
}
fn integer(value: &Value, key: &str) -> Result<u64> {
    match &value[key] {
        Value::String(v) => Ok(v.parse()?),
        v => v
            .as_u64()
            .with_context(|| format!("invalid holdings integer {key}")),
    }
}
pub fn maturity(height: u64, tip: u64) -> Result<Value> {
    ensure!(height <= tip, "output above holdings snapshot");
    let spend_height = height
        .checked_add(assets::COINBASE_MATURITY)
        .context("coinbase maturity height overflow")?;
    let confirmations = (tip - height)
        .checked_add(1)
        .context("confirmation count overflow")?;
    Ok(
        json!({"confirmations":confirmations,"coinbase_spend_height":spend_height,"coinbase_mature_at_tip":tip>=spend_height,
        "coinbase_eligible_in_next_block":tip-height>=assets::COINBASE_MATURITY-1,"blocks_to_maturity":spend_height.saturating_sub(tip)}),
    )
}
pub fn binding_matches(data: &Value) -> bool {
    let frame = &data["holdings_snapshot"];
    let mut body = data.clone();
    let Some(object) = body.as_object_mut() else {
        return false;
    };
    object.remove("holdings_snapshot");
    let Some(mut unsigned) = frame.as_object().cloned() else {
        return false;
    };
    unsigned.remove("snapshot_id");
    frame["contract"] == "bitcoin_holdings_v1"
        && frame["network_id"] == data["network_id"]
        && frame["address"] == data["address"]
        && frame["source_snapshot_id"] == data["source_snapshot"]["snapshot_id"]
        && frame["metadata_snapshot_id"] == data["native_asset_snapshot"]["snapshot_id"]
        && frame["evidence_hash"] == aml_risk_policy::digest(&body)
        && frame["snapshot_id"] == aml_risk_policy::digest(&json!(unsigned))
}
pub async fn read(db: &Database, address: &str, query: &Query) -> Result<Value> {
    query.validate()?;
    let address = model::address(address)?;
    let snapshot = visibility::capture(
        &db.warehouse_connection(),
        db.database(),
        aml_warehouse::Network::Bitcoin,
    )
    .await?;
    let metadata = assets::read(db).await?;
    let params = [
        ("network", NETWORK_ID.into()),
        ("address", address.clone()),
        ("height", snapshot.state().last_block_number.to_string()),
    ];
    let summary_sql = format!(
        "{SQL} SELECT count() AS latest_output_count,countIf(is_spent=1) AS spent_output_count,countIf(is_spent=0) AS utxo_count,toString(sumIf(toUInt128(value_sats),is_spent=0)) AS balance_sats,toString(sumIf(toUInt128(value_sats),is_spent=0 AND coinbase=1 AND {{height:UInt64}}-block_number<100)) AS immature_sats,toString(sumIf(toUInt128(value_sats),is_spent=0 AND (coinbase=0 OR {{height:UInt64}}-block_number>=99))) AS eligible_next_block_sats FROM states {SETTINGS}"
    );
    let list_sql = format!(
        "{SQL} SELECT block_number,block_hash,tx_hash,transaction_index,output_index,coinbase,script_type,toString(value_sats) AS value_sats,commit_id,toString(revision) AS revision,is_spent,if(is_spent=1,toNullable(spending.1),NULL) AS spent_in_block,if(is_spent=1,spending.2,'') AS spent_in_block_hash,if(is_spent=1,spending.3,'') AS spent_in_tx_hash,if(is_spent=1,toNullable(spending.4),NULL) AS spent_in_input_index FROM states ORDER BY block_number DESC,transaction_index DESC,tx_hash,output_index LIMIT {} {SETTINGS}",
        query.limit + 1
    );
    let (summaries, mut outputs): (Vec<Value>, Vec<Value>) =
        tokio::try_join!(db.rows(&summary_sql, &params), db.rows(&list_sql, &params))?;
    let summary = summaries.first().context("holdings summary missing")?;
    let balance = integer(summary, "balance_sats")?;
    let immature = integer(summary, "immature_sats")?;
    let eligible = integer(summary, "eligible_next_block_sats")?;
    ensure!(
        balance <= assets::MAX_MONEY && immature <= balance && eligible <= balance,
        "invalid holdings conservation or money range"
    );
    let truncated = outputs.len() > query.limit;
    outputs.truncate(query.limit);
    for output in &mut outputs {
        let height = integer(output, "block_number")?;
        let amount = integer(output, "value_sats")?;
        ensure!(
            amount <= assets::MAX_MONEY,
            "output exceeds Bitcoin money range"
        );
        let coinbase = output["coinbase"] == 1;
        let spent = output["is_spent"] == 1;
        let age = maturity(height, snapshot.state().last_block_number)?;
        output["status"] = json!(if spent {
            "spent"
        } else if coinbase && age["coinbase_mature_at_tip"] == false {
            "unspent_immature_coinbase"
        } else {
            "unspent"
        });
        output["confirmations"] = age["confirmations"].clone();
        output["maturity"] = if coinbase { age } else { Value::Null };
        output["value_btc"] = json!(model::decimal(amount));
        output["outpoint"] = json!(format!(
            "{}:{}",
            output["tx_hash"].as_str().unwrap(),
            integer(output, "output_index")?
        ));
        output["occurrence_id"] = json!(format!(
            "{}:{}:{}",
            output["block_hash"].as_str().unwrap(),
            output["tx_hash"].as_str().unwrap(),
            integer(output, "output_index")?
        ));
        output["spendability_verified"] = json!(false);
    }
    let initialized = snapshot.state().initialized == 1;
    let complete = initialized && snapshot.state().start_block == 0;
    let amount = |v: u64| {
        if initialized {
            json!(v.to_string())
        } else {
            Value::Null
        }
    };
    let decimal = |v: u64| {
        if initialized {
            json!(model::decimal(v))
        } else {
            Value::Null
        }
    };
    let mut value = json!({"contract":"bitcoin_holdings_v1","network_id":NETWORK_ID,"address":address,"asset_id":"native:BTC","symbol":"BTC","decimals":8,
        "status":if !initialized {"waiting_for_blocks"}else if complete {"available"}else{"partial"},"source":"canonical_clickhouse_utxo","scope":"ingested_confirmed_utxos","observation_scope":"decoded_address_scripts_in_ingested_window",
        "balance_sats":amount(balance),"balance_decimal":decimal(balance),"immature_sats":amount(immature),"immature_btc":decimal(immature),"mature_sats":amount(balance-immature),"mature_btc":decimal(balance-immature),"eligible_next_block_sats":amount(eligible),"spendable_sats":null,
        "utxo_count":if initialized {summary["utxo_count"].clone()}else{Value::Null},"spent_output_count":if initialized {summary["spent_output_count"].clone()}else{Value::Null},"latest_output_count":if initialized {summary["latest_output_count"].clone()}else{Value::Null},
        "outputs":outputs,"outputs_truncated":truncated,"outputs_limit":query.limit,"output_state_scope":"latest_occurrence_per_outpoint_before_address_filter","source_snapshot":snapshot.value(),
        "as_of_block":if initialized {Some(snapshot.state().last_block_number)}else{None},"block_hash":snapshot.state().last_block_hash,"revision":snapshot.state().revision.to_string(),"history_complete":complete,"is_complete_balance":complete});
    let details = json!({
        "metadata":metadata["assets"][0],"native_asset_snapshot":metadata["native_asset_snapshot"],"capabilities":metadata["capabilities"],
        "persisted":false,"source_facts_persisted":initialized,"metadata_persisted":true,"mempool_included":false,"ownership_claimed":false,"unavailable_is_not_zero":true,
        "maturity_policy":{"coinbase_blocks":100,"mature_at_tip":"tip_height >= creation_height + 100","eligible_in_next_block":"tip_height + 1 >= creation_height + 100","maturity_does_not_verify_keys_or_script_spendability":true},
        "limitations":["A partial ingestion window gives observed holdings only; outputs before start_block can be absent.","Balances are for decoded address scripts; bare public keys, multisig participants and script-only outputs are not assigned to an address.","Received minus spent activity is not a balance, especially with partial history or repeated txids.","Mature outputs are not verified spendable: keys, script conditions and mempool spends are not checked.","Recent output states are bounded; aggregate holdings are not truncated with the list.","BIP30 superseded output occurrences remain in history but do not reappear as current holdings."]});
    value
        .as_object_mut()
        .unwrap()
        .extend(details.as_object().unwrap().clone());
    let mut frame = json!({"contract":"bitcoin_holdings_v1","network_id":NETWORK_ID,"address":address,"source_snapshot_id":snapshot.id(),"metadata_snapshot_id":metadata["native_asset_snapshot"]["snapshot_id"],"evidence_hash":aml_risk_policy::digest(&value)});
    frame["snapshot_id"] = json!(aml_risk_policy::digest(&frame));
    value["holdings_snapshot"] = frame;
    visibility::unchanged(&db.warehouse_connection(), db.database(), &snapshot).await?;
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coinbase_tip_and_next_block_boundaries_are_distinct() {
        assert_eq!(
            maturity(10, 108).unwrap()["coinbase_eligible_in_next_block"],
            false
        );
        let hundred = maturity(10, 109).unwrap();
        assert_eq!(hundred["confirmations"], 100);
        assert_eq!(hundred["coinbase_eligible_in_next_block"], true);
        assert_eq!(hundred["coinbase_mature_at_tip"], false);
        let mature = maturity(10, 110).unwrap();
        assert_eq!(mature["coinbase_mature_at_tip"], true);
        assert_eq!(mature["blocks_to_maturity"], 0);
        assert!(maturity(11, 10).is_err());
        assert!(maturity(u64::MAX, u64::MAX).is_err());
    }
    #[test]
    fn output_limits_are_strict_and_unknown_options_fail() {
        assert!(Query { limit: 0 }.validate().is_err());
        assert!(Query { limit: 101 }.validate().is_err());
        assert!(serde_json::from_value::<Query>(json!({"asset":"erc20"})).is_err());
    }
    #[test]
    fn genesis_bare_pubkey_and_op_return_are_not_assigned_to_an_address() {
        let genesis = json!({"value":"50.00000000","scriptPubKey":{"hex":"4104678afdb0fe5548271967f1a67130b7105cd6a828e03909a67962e0ea1f61deb649f6bc3f4cef38c4f35504e51ec112de5c384df7ba0b8d578a4c702b6bf11d5fac","type":"pubkey"}});
        let output = model::ScriptOutput::from_rpc(&genesis).unwrap();
        assert!(output.address.is_empty());
        assert!(output.subject().starts_with("script:"));
        let burn = model::ScriptOutput::from_rpc(
            &json!({"value":"0.00000001","scriptPubKey":{"hex":"6a0100","type":"nulldata"}}),
        )
        .unwrap();
        assert!(burn.address.is_empty());
    }
}
