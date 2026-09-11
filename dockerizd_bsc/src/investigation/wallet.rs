use super::{SearchOptions, graph::wallet_graph, now_ms};
use crate::{BSC_NETWORK_ID, db::warehouse::Warehouse, intelligence};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub async fn status(db: &Warehouse) -> Result<Value> {
    let coverage = db.rows("SELECT count() AS complete_blocks, min(block_number) AS first_synced_block,
        max(block_number) AS last_synced_block, countIf(receipt_data_complete=1) AS receipt_complete_blocks,
        countIf(trace_data_complete=1) AS trace_complete_blocks,
        toString(sumWithOverflow(cityHash64(block_number,block_hash,current_revision))) AS canonical_epoch,
        argMax(block_hash,block_number) AS tip_hash, max(indexed_at_unix_ms) AS last_indexed_at_unix_ms
        FROM ingested_blocks_canonical WHERE network_id='eip155:56'", &[]).await?;
    let mut data = coverage.into_iter().next().unwrap_or(json!({}));
    let count = data["complete_blocks"].as_u64().unwrap_or(0);
    data["network_id"] = json!(BSC_NETWORK_ID);
    data["internal_transfer_coverage"] =
        json!(count > 0 && data["trace_complete_blocks"] == data["complete_blocks"]);
    data["contiguous_indexed_range"] = json!(
        count > 0
            && data["last_synced_block"].as_u64().unwrap_or(0)
                - data["first_synced_block"].as_u64().unwrap_or(0)
                + 1
                == count
    );
    data["history_from_genesis"] = json!(
        count > 0 && data["first_synced_block"] == 0 && data["contiguous_indexed_range"] == true
    );
    data["graph_storage"] = json!("central_neo4j");
    data["observed_at_unix_ms"] = json!(now_ms());
    Ok(data)
}

pub(crate) async fn check_epoch(db: &Warehouse, coverage: &Value) -> Result<()> {
    let height = coverage["last_synced_block"]
        .as_u64()
        .unwrap_or(0)
        .to_string();
    let rows=db.rows("SELECT count() AS complete_blocks,
        toString(sumWithOverflow(cityHash64(block_number,block_hash,current_revision))) AS canonical_epoch
        FROM ingested_blocks_canonical WHERE network_id='eip155:56' AND block_number<={height:UInt64}",
        &[("height",&height)]).await?;
    ensure!(
        rows[0]["canonical_epoch"] == coverage["canonical_epoch"]
            && rows[0]["complete_blocks"] == coverage["complete_blocks"],
        "canonical history changed during investigation; retry"
    );
    Ok(())
}

pub async fn investigate(db: &Warehouse, address: &str, options: &SearchOptions) -> Result<Value> {
    options.validate()?;
    let coverage = status(db).await?;
    let height = coverage["last_synced_block"].as_u64().unwrap_or(0);
    let height_string = height.to_string();
    let start = options.from_ms.to_string();
    let end = options.to_ms.to_string();
    let params = [
        ("address", address),
        ("height", height_string.as_str()),
        ("start", start.as_str()),
        ("end", end.as_str()),
        ("asset", options.asset_id.as_str()),
    ];
    let scope = "network_id='eip155:56' AND block_number<={height:UInt64}
        AND block_timestamp_unix_ms BETWEEN {start:UInt64} AND {end:UInt64}
        AND (from_address={address:String} OR to_address={address:String})";
    let transfer_scope = format!("{scope} AND ({{asset:String}}='' OR asset_id={{asset:String}})");
    let mut fingerprint=db.rows(&format!("SELECT count() AS transfer_count, countIf(to_address={{address:String}}) AS inbound_transfers,
        countIf(from_address={{address:String}}) AS outbound_transfers,
        uniqExact(if(from_address={{address:String}},to_address,from_address)) AS unique_counterparties,
        min(block_timestamp_unix_ms) AS first_seen_unix_ms, max(block_timestamp_unix_ms) AS last_seen_unix_ms
        FROM address_relationships_canonical WHERE {transfer_scope}"),&params).await?.remove(0);
    let tx = db
        .rows(
            &format!(
                "SELECT count() AS transaction_count, countIf(status=0) AS failed_transactions,
        countIf(input_data!='' AND input_data!='0x') AS contract_calls,
        toString(sumIf(fee_paid,from_address={{address:String}})) AS paid_fees_raw
        FROM transactions_canonical WHERE {scope}"
            ),
            &params,
        )
        .await?
        .remove(0);
    fingerprint
        .as_object_mut()
        .unwrap()
        .extend(tx.as_object().unwrap().clone());
    let semantic_scope="network_id='eip155:56' AND subject_address={address:String} AND block_number<={height:UInt64}
        AND block_timestamp_unix_ms BETWEEN {start:UInt64} AND {end:UInt64}";
    let semantic_counts = db
        .rows(
            &format!(
                "SELECT countIf(event_type='swap') AS swap_events,
        countIf(startsWith(event_type,'bridge')) AS bridge_events,
        countIf(startsWith(event_type,'mixer_')) AS mixer_events,
        countIf(startsWith(event_type,'liquidity_')) AS liquidity_events
        FROM semantic_aml_events_canonical WHERE {semantic_scope}"
            ),
            &params,
        )
        .await?
        .remove(0);
    fingerprint
        .as_object_mut()
        .unwrap()
        .extend(semantic_counts.as_object().unwrap().clone());
    let semantic_events=db.rows(&format!("SELECT event_id,tx_hash,block_number,block_timestamp_unix_ms,event_type,
        protocol,protocol_contract,counterparty_address,asset_in,asset_out,remote_network_id,remote_receiver,
        bridge_message_id,bridge_direction,amount_in,amount_out,confidence,evidence_refs,evidence_json,detector_version
        FROM semantic_aml_events_canonical WHERE {semantic_scope}
        ORDER BY block_number DESC,event_id LIMIT 201"),&params).await?;
    let mut asset_flows = db
        .rows(
            &format!(
                "SELECT asset_id,token_id,
        toString(sumIf(amount,to_address={{address:String}})) AS inbound_amount,
        toString(sumIf(amount,from_address={{address:String}})) AS outbound_amount,
        countIf(to_address={{address:String}}) AS inbound_transfers,
        countIf(from_address={{address:String}}) AS outbound_transfers,count() AS transfer_count
        FROM address_relationships_canonical WHERE {transfer_scope}
        GROUP BY asset_id,token_id ORDER BY transfer_count DESC,asset_id LIMIT 101"
            ),
            &params,
        )
        .await?;
    let metadata=db.rows("SELECT token_address,name,symbol,decimals,token_standard,metadata_status,metadata_source,is_verified,
        reviewed_by,source_reference,observed_block,created_at_unix_ms
        FROM token_metadata_current WHERE network_id='eip155:56'
        AND token_address IN (SELECT token_address FROM token_metadata_discoveries_canonical
        WHERE tx_hash IN (SELECT tx_hash FROM address_relationships_canonical
        WHERE from_address={address:String} OR to_address={address:String})) LIMIT 500", &[("address",address)]).await?;
    for flow in &mut asset_flows {
        let asset = flow["asset_id"].as_str().unwrap_or("");
        flow["metadata"] = if asset.ends_with("/native:bnb") {
            json!({"symbol":"BNB","name":"BNB","decimals":18})
        } else {
            metadata
                .iter()
                .find(|m| asset.contains(m["token_address"].as_str().unwrap_or("NOT_FOUND")))
                .cloned()
                .unwrap_or(Value::Null)
        };
    }
    let mut counterparties = db
        .rows(
            &format!(
                "SELECT if(from_address={{address:String}},to_address,from_address) AS address,
        countIf(to_address={{address:String}}) AS inbound_transfers,
        countIf(from_address={{address:String}}) AS outbound_transfers,count() AS total_transfers,
        max(block_timestamp_unix_ms) AS last_seen_unix_ms
        FROM address_relationships_canonical WHERE {transfer_scope}
        GROUP BY address ORDER BY total_transfers DESC,address LIMIT 101"
            ),
            &params,
        )
        .await?;
    let total = fingerprint["transfer_count"].as_u64().unwrap_or(0) as f64;
    for counterparty in &mut counterparties {
        counterparty["transfer_share"] = json!(if total > 0.0 {
            counterparty["total_transfers"].as_u64().unwrap_or(0) as f64 / total
        } else {
            0.0
        });
    }
    let activity=db.rows(&format!("SELECT intDiv(block_timestamp_unix_ms,86400000)*86400000 AS day_unix_ms,
        countIf(to_address={{address:String}}) AS received,countIf(from_address={{address:String}}) AS sent,
        uniqExact(tx_hash) AS transactions FROM address_relationships_canonical WHERE {transfer_scope}
        GROUP BY day_unix_ms ORDER BY day_unix_ms DESC LIMIT 367"),&params).await?;
    let wallet_graph = wallet_graph(db, address, options, height).await?;
    let entities = intelligence::active(db, address).await?;
    let clusters = entities
        .iter()
        .filter(|v| v["claim_kind"] == "cluster")
        .cloned()
        .collect::<Vec<_>>();
    let (exposure_paths, exposure_coverage) =
        intelligence::exposure(db, address, height, now_ms()).await?;
    // Detect replay/reorg changes anywhere in the queried prefix, not only at the tip.
    check_epoch(db, &coverage).await?;
    let mut data = json!({
        "api_version":"1.0","network_id":BSC_NETWORK_ID,"address":address,"fingerprint":fingerprint,
        "top_counterparties":counterparties.iter().take(100).collect::<Vec<_>>(),
        "asset_flows":asset_flows.iter().take(100).collect::<Vec<_>>(),
        "semantic_events":semantic_events.iter().take(200).collect::<Vec<_>>(),
        "activity":activity.iter().take(366).collect::<Vec<_>>(),"entities":entities,"clusters":clusters,
        "exposure_paths":exposure_paths,"exposure_coverage":exposure_coverage,
        "graph":wallet_graph,"data_coverage":coverage,
        "holdings":{"status":"on_demand","source":"finalized_rpc","endpoint":format!("/api/bsc/wallet/{address}/holdings"),"scope":"native_and_discovered_assets"},
        "neo4j_projection":{"projected":false,"owner":"main_vm"},
        "risk_engine":{"enabled":false,"status":"central_assessment_required","probability_claimed":false},
        "query_scope":options,"completed_at_unix_ms":now_ms(),
        "limits":{"events_truncated":semantic_events.len()>200,"assets_truncated":asset_flows.len()>100,
            "counterparties_truncated":counterparties.len()>100,"activity_truncated":activity.len()>366},
        "limitations":["Asset flows are observed transfers, NOT wallet balances. Fees/rebases and missing history prevent deriving holdings from net flow.",
            "Transaction counters include external transactions only; transfers also include available internal traces.",
            "Bridge evidence is reported; destination-chain matching is not inferred.","Graph filters apply to flows; semantic counts cover the selected time range."]
    });
    data["data_coverage"]["truncated"] = json!(
        data["graph"]["truncated"] == true
            || semantic_events.len() > 200
            || asset_flows.len() > 100
            || counterparties.len() > 100
            || activity.len() > 366
    );
    Ok(data)
}
