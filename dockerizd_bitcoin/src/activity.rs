//! Exact scoped UTXO aggregates. Counterparties are transaction co-occurrences,
//! never allocated input/output payments or ownership clusters.
use crate::{NETWORK_ID, db::Database, model};
use aml_warehouse::{Network, RawAmount, visibility};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
const PREFIX: &str = include_str!("../sql/queries/activity.sql");
const BUDGET: &str = " SETTINGS max_threads=2,max_memory_usage=268435456,max_execution_time=15";
pub const MAX_TRANSACTIONS: u64 = 100_000;
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    All,
    Received,
    Spent,
    Both,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub limit: usize,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub from_unix_ms: Option<u64>,
    pub to_unix_ms: Option<u64>,
    pub direction: Direction,
    pub activity_limit: usize,
    pub counterparty_limit: usize,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            limit: 40,
            from_block: None,
            to_block: None,
            from_unix_ms: None,
            to_unix_ms: None,
            direction: Direction::All,
            activity_limit: 90,
            counterparty_limit: 20,
        }
    }
}
impl Query {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=100).contains(&self.limit)
                && (1..=366).contains(&self.activity_limit)
                && (1..=100).contains(&self.counterparty_limit),
            "limits: transactions/graph 1..100, activity days 1..366, counterparties 1..100"
        );
        ensure!(
            self.from_block
                .zip(self.to_block)
                .is_none_or(|(a, b)| a <= b),
            "from_block must not exceed to_block"
        );
        ensure!(
            self.from_unix_ms
                .zip(self.to_unix_ms)
                .is_none_or(|(a, b)| a <= b),
            "from_unix_ms must not exceed to_unix_ms"
        );
        Ok(())
    }
    fn prefix(&self) -> String {
        PREFIX
            .lines()
            .filter(|l| !l.starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .replace(
                "{{direction}}",
                match self.direction {
                    Direction::All => "1",
                    Direction::Received => "received_output_count>0",
                    Direction::Spent => "spent_input_count>0",
                    Direction::Both => "received_output_count>0 AND spent_input_count>0",
                },
            )
    }
}
#[derive(Debug)]
pub struct ScopeLimit;
impl std::fmt::Display for ScopeLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Activity scope exceeds {MAX_TRANSACTIONS} transactions; narrow the block or time filters"
        )
    }
}
impl std::error::Error for ScopeLimit {}

fn decimal(raw: &str) -> Result<String> {
    let (negative, amount) = raw.strip_prefix('-').map_or((false, raw), |s| (true, s));
    let amount = RawAmount::parse(amount)?;
    Ok(format!(
        "{}{}",
        if negative && amount.as_str() != "0" {
            "-"
        } else {
            ""
        },
        amount.decimal(8)
    ))
}
fn amounts(row: &mut Value) -> Result<()> {
    for name in [
        "received_sats",
        "spent_sats",
        "net_flow_sats",
        "fees_of_involved_transactions_sats",
        "spending_transaction_fees_sats",
    ] {
        if let Some(raw) = row.get(name) {
            let value = decimal(raw.as_str().context("exact satoshi string required")?)?;
            row[name.replace("_sats", "_btc")] = json!(value);
        }
    }
    Ok(())
}
fn join_keys(left: &str, right: &str) -> String {
    [
        "network_id",
        "block_number",
        "block_hash",
        "block_timestamp_unix_ms",
        "commit_id",
        "revision",
        "tx_hash",
        "transaction_index",
    ]
    .iter()
    .map(|k| format!("{left}.{k}={right}.{k}"))
    .collect::<Vec<_>>()
    .join(" AND ")
}
pub async fn read(db: &Database, address: &str, query: &Query) -> Result<Value> {
    query.validate()?;
    let address = model::address(address)?;
    let snapshot =
        visibility::capture(&db.warehouse_connection(), db.database(), Network::Bitcoin).await?;
    let params = [
        ("network", NETWORK_ID.into()),
        ("address", address.clone()),
        ("from_block", query.from_block.unwrap_or(0).to_string()),
        ("to_block", query.to_block.unwrap_or(u64::MAX).to_string()),
        ("from_time", query.from_unix_ms.unwrap_or(0).to_string()),
        ("to_time", query.to_unix_ms.unwrap_or(u64::MAX).to_string()),
    ];
    let prefix = query.prefix();
    let summary=format!("{prefix} SELECT count() AS transaction_count,
        sum(s.spent_input_count) AS spent_input_count,sum(s.received_output_count) AS received_output_count,
        sum(s.spent_input_count+s.received_output_count) AS transfer_count,
        countIf(s.received_output_count>0) AS receiving_transactions,countIf(s.spent_input_count>0) AS spending_transactions,
        countIf(s.received_output_count>0 AND s.spent_input_count>0) AS both_sides_transactions,
        countIf(s.coinbase=1) AS coinbase_transactions,
        toString(sum(s.received_sats)) AS received_sats,toString(sum(s.spent_sats)) AS spent_sats,
        toString(toInt256(sum(s.received_sats))-toInt256(sum(s.spent_sats))) AS net_flow_sats,
        toString(sumIf(s.fee_sats,s.coinbase=0)) AS fees_of_involved_transactions_sats,
        toString(sumIf(s.fee_sats,s.spent_input_count>0)) AS spending_transaction_fees_sats,
        if(count()=0,NULL,min(s.block_timestamp_unix_ms)) AS first_seen_unix_ms,
        if(count()=0,NULL,max(s.block_timestamp_unix_ms)) AS last_seen_unix_ms,
        if(count()=0,NULL,min(s.block_number)) AS first_seen_block,if(count()=0,NULL,max(s.block_number)) AS last_seen_block,
        uniqExact(intDiv(s.block_timestamp_unix_ms,86400000)) AS active_days,
        if(count()=0,NULL,max(if(s.coinbase=1,0,s.input_count))) AS maximum_transaction_inputs,
        if(count()=0,NULL,max(s.output_count)) AS maximum_transaction_outputs,
        countIf(s.coinbase=0 AND s.input_count>1) AS multi_input_transactions,countIf(s.output_count>1) AS multi_output_transactions,
        (SELECT count() FROM wallet_legs) AS wallet_occurrences,(SELECT count() FROM involved AS s) AS matched_occurrences,
        (SELECT countIf(isNull(s.input_count) OR isNull(s.output_count) OR s.spent_input_count>ifNull(s.input_count,0) OR s.received_output_count>ifNull(s.output_count,0) OR (s.coinbase=1 AND s.spent_input_count>0)) FROM involved AS s) AS invalid_occurrences
        FROM scoped AS s{BUDGET}");
    let mut fingerprint: Value = db
        .rows(&summary, &params)
        .await?
        .into_iter()
        .next()
        .context("activity summary missing")?;
    ensure!(
        fingerprint["wallet_occurrences"] == fingerprint["matched_occurrences"]
            && fingerprint["invalid_occurrences"] == 0,
        "canonical activity legs/transaction witnesses disagree"
    );
    let count = fingerprint["transaction_count"]
        .as_u64()
        .context("transaction count missing")?;
    if count > MAX_TRANSACTIONS {
        return Err(ScopeLimit.into());
    }
    for key in [
        "wallet_occurrences",
        "matched_occurrences",
        "invalid_occurrences",
    ] {
        fingerprint.as_object_mut().unwrap().remove(key);
    }
    let daily=format!("{prefix} SELECT intDiv(s.block_timestamp_unix_ms,86400000)*86400000 AS day_unix_ms,
        count() AS transactions,sum(s.received_output_count) AS received,sum(s.spent_input_count) AS spent,
        toString(sum(s.received_sats)) AS received_sats,toString(sum(s.spent_sats)) AS spent_sats,
        toString(toInt256(sum(s.received_sats))-toInt256(sum(s.spent_sats))) AS net_flow_sats,
        toString(sumIf(s.fee_sats,s.coinbase=0)) AS fees_of_involved_transactions_sats
        FROM scoped AS s GROUP BY day_unix_ms ORDER BY day_unix_ms DESC LIMIT {}{BUDGET}",query.activity_limit+1);
    let mut activity: Vec<Value> = db.rows(&daily, &params).await?;
    let activity_truncated = activity.len() > query.activity_limit;
    activity.truncate(query.activity_limit);
    for day in &mut activity {
        amounts(day)?;
    }
    let recent=format!("{prefix} SELECT block_number,block_hash,toString(revision) AS revision,commit_id,tx_hash,transaction_index,block_timestamp_unix_ms,coinbase,input_count,output_count,
        if(coinbase=1,0,input_count) AS funding_input_count,spent_input_count,received_output_count,toString(received_sats) AS received_sats,toString(spent_sats) AS spent_sats,
        toString(toInt256(received_sats)-toInt256(spent_sats)) AS net_flow_sats,toString(fee_sats) AS transaction_fee_sats
        FROM scoped ORDER BY block_number DESC,transaction_index DESC,tx_hash LIMIT {}{BUDGET}",query.limit+1);
    let mut transactions: Vec<Value> = db.rows(&recent, &params).await?;
    let transactions_truncated = transactions.len() > query.limit;
    transactions.truncate(query.limit);
    for tx in &mut transactions {
        amounts(tx)?;
        tx["occurrence_id"] = json!(format!(
            "tx:{}:{}",
            tx["block_hash"].as_str().unwrap(),
            tx["tx_hash"].as_str().unwrap()
        ));
        tx["fee_attributed_to_wallet"] = json!(false);
    }
    let peer_branch = |table: &str, role: &str, condition: &str| {
        format!("SELECT l.network_id,l.block_number,l.block_hash,l.commit_id,l.revision,l.tx_hash,l.transaction_index,l.block_timestamp_unix_ms,
        if(isNull(l.address),'script','address') AS subject_type,ifNull(l.address,concat('script:',lower(hex(SHA256(l.script_hex))))) AS subject_id,'{role}' AS role
        FROM {table} AS l INNER JOIN scoped AS s ON {} WHERE {condition} AND (isNull(l.address) OR l.address!={{address:String}})",join_keys("l","s"))
    };
    let peers=format!("{prefix}, peer_legs AS ({} UNION ALL {}), counterparties AS (
        SELECT subject_type,subject_id,uniqExact(tuple(network_id,block_number,block_hash,commit_id,revision,tx_hash,transaction_index)) AS transaction_count,
        uniqExactIf(tuple(block_number,block_hash,commit_id,revision,tx_hash,transaction_index),role='input_in_receipt_tx') AS upstream_transactions,
        uniqExactIf(tuple(block_number,block_hash,commit_id,revision,tx_hash,transaction_index),role='output_in_spend_tx') AS downstream_transactions,
        min(block_timestamp_unix_ms) AS first_seen_unix_ms,max(block_timestamp_unix_ms) AS last_seen_unix_ms
        FROM peer_legs GROUP BY subject_type,subject_id)
        SELECT *,count() OVER () AS total_counterparties,countIf(upstream_transactions>0) OVER () AS upstream_subjects,countIf(downstream_transactions>0) OVER () AS downstream_subjects
        FROM counterparties ORDER BY transaction_count DESC,subject_type,subject_id LIMIT {}{BUDGET}",
        peer_branch("aml_bitcoin_inputs_canonical","input_in_receipt_tx","s.received_output_count>0"),peer_branch("aml_bitcoin_outputs_canonical","output_in_spend_tx","s.spent_input_count>0"),query.counterparty_limit+1);
    let mut counterparties: Vec<Value> = db.rows(&peers, &params).await?;
    fingerprint["unique_counterparties"] = counterparties
        .first()
        .map(|p| p["total_counterparties"].clone())
        .unwrap_or(json!(0));
    fingerprint["upstream_subjects"] = counterparties
        .first()
        .map(|p| p["upstream_subjects"].clone())
        .unwrap_or(json!(0));
    fingerprint["downstream_subjects"] = counterparties
        .first()
        .map(|p| p["downstream_subjects"].clone())
        .unwrap_or(json!(0));
    let counterparties_truncated = counterparties.len() > query.counterparty_limit;
    counterparties.truncate(query.counterparty_limit);
    for peer in &mut counterparties {
        for key in [
            "total_counterparties",
            "upstream_subjects",
            "downstream_subjects",
        ] {
            peer.as_object_mut().unwrap().remove(key);
        }
        peer["address"] = if peer["subject_type"] == "address" {
            peer["subject_id"].clone()
        } else {
            Value::Null
        };
        peer["attributed_amount_sats"] = Value::Null;
        peer["fund_continuity_claimed"] = json!(false);
        peer["ownership_claimed"] = json!(false);
    }
    amounts(&mut fingerprint)?;
    fingerprint["is_truncated"] = json!(false);
    fingerprint["transfer_count_semantics"] = json!("observed_wallet_utxo_legs");
    fingerprint["wallet_attributed_fee_sats"] = Value::Null;
    fingerprint["query_scope"] = json!({"contract":"bitcoin_activity_v1","filters":query,"source_snapshot_id":snapshot.id(),"scope":"ingested_canonical_window"});
    visibility::unchanged(&db.warehouse_connection(), db.database(), &snapshot).await?;
    Ok(
        json!({"network_id":NETWORK_ID,"address":address,"source_snapshot":snapshot.value(),"fingerprint":fingerprint,"activity":activity,"recent_transactions":transactions,"top_counterparties":counterparties,
        "asset_flows":[{"asset_id":"native:BTC","symbol":"BTC","decimals":8,"inbound_amount":fingerprint["received_sats"],"outbound_amount":fingerprint["spent_sats"],"net_flow_sats":fingerprint["net_flow_sats"],"amount_semantics":"observed_wallet_output_and_spent_input_values","net_flow_is_balance":false}],
        "query_scope":{"analytics":{"contract":"bitcoin_activity_v1","filters":query,"source_snapshot_id":snapshot.id(),"scope":"ingested_canonical_window","ingested_start_block":snapshot.state().start_block,"ingested_tip_block":if snapshot.state().initialized==1{Some(snapshot.state().last_block_number)}else{None},"requested_block_bounds_ingested":snapshot.state().initialized==1 && query.from_block.is_none_or(|h|(snapshot.state().start_block..=snapshot.state().last_block_number).contains(&h)) && query.to_block.is_none_or(|h|(snapshot.state().start_block..=snapshot.state().last_block_number).contains(&h)),"history_from_genesis":snapshot.state().initialized==1 && snapshot.state().start_block==0,"mempool_included":false},"graph":{"scope":"recent_unfiltered_canonical_neighborhood","max_transactions":query.limit},"holdings":{"scope":"all_ingested_confirmed_utxos","analytics_filters_applied":false},"exposure":{"scope":"bounded_unfiltered_canonical_paths","analytics_filters_applied":false},"risk":{"scope":"reviewed_labels_and_bounded_exposure","analytics_filters_applied_to_exposure":false}},
        "activity_coverage":{"status":if snapshot.state().initialized==0{"waiting_for_blocks"}else{"complete_within_scope"},"summary_complete":snapshot.state().initialized==1,"transactions_in_scope":count,"maximum_transactions":MAX_TRANSACTIONS,"day_buckets":"active_UTC_days_only","activity_truncated":activity_truncated,"transactions_truncated":transactions_truncated,"counterparties_truncated":counterparties_truncated,"source_snapshot":snapshot.value(),"counterparty_method":"directional_transaction_cooccurrence","fund_continuity_claimed":false,"ownership_claimed":false,"limitations":["Activity filters apply to analytics only; graph, holdings and exposure retain their separately declared scopes.","Peer inputs in receipt transactions and peer outputs in spend transactions are context, not allocated payments.","Transaction fees are counted once per involved occurrence and are not attributed to this wallet.","Net flow covers observed legs in the selected ingested window and is not a wallet balance."]}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_rejects_invalid_ranges_caps_and_unknown_fields() {
        for q in [
            Query {
                from_block: Some(2),
                to_block: Some(1),
                ..Default::default()
            },
            Query {
                from_unix_ms: Some(2),
                to_unix_ms: Some(1),
                ..Default::default()
            },
            Query {
                limit: 0,
                ..Default::default()
            },
            Query {
                activity_limit: 367,
                ..Default::default()
            },
            Query {
                counterparty_limit: 101,
                ..Default::default()
            },
        ] {
            assert!(q.validate().is_err());
        }
        assert!(serde_json::from_value::<Query>(json!({"unexpected":true})).is_err());
        assert!(serde_json::from_value::<Query>(json!({"direction":"invalid"})).is_err());
        assert!(
            Query {
                from_block: Some(0),
                to_block: Some(0),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
    }
    #[test]
    fn exact_signed_totals_never_round_or_confuse_negative_flows_with_balance() {
        assert_eq!(decimal("-1").unwrap(), "-0.00000001");
        assert_eq!(decimal("0").unwrap(), "0.00000000");
        assert_eq!(
            decimal("18446744073709551616").unwrap(),
            "184467440737.09551616"
        );
        assert!(decimal("1.5").is_err());
    }
}
