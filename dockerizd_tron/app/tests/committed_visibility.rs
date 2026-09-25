//! Requires an empty, disposable tron_db initialized from the baseline SQL.
use anyhow::{Result, ensure};
use arz_axum_for_services::{
    db::tron_schema::validate_tron_schema,
    services::tron::{
        neo4j::flow_graph::build_wallet_flow_graph, wallet_activity::build_wallet_activity,
        wallet_holdings::build_wallet_holdings,
    },
};
use clickhouse::Client;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires TRON_ANALYTICS_TEST_URL pointing to an empty disposable ClickHouse"]
async fn incomplete_blocks_are_hidden_from_graph_activity_and_balances() -> Result<()> {
    let client = Arc::new(
        Client::default()
            .with_url(std::env::var("TRON_ANALYTICS_TEST_URL")?)
            .with_user(
                std::env::var("TRON_ANALYTICS_TEST_USER").unwrap_or_else(|_| "default".into()),
            )
            .with_password(std::env::var("TRON_ANALYTICS_TEST_PASSWORD").unwrap_or_default())
            .with_database("tron_db")
            .with_option("join_algorithm", "auto")
            .with_option("max_threads", "2"),
    );
    ensure!(
        client
            .query("SELECT count() FROM address_relationships")
            .fetch_one::<u64>()
            .await?
            == 0,
        "refusing non-empty database: use a disposable test instance"
    );
    // Simulate an existing volume with the unsafe old view; startup must upgrade it without deleting facts.
    client.query("CREATE OR REPLACE VIEW tron_db.address_relationships_canonical AS SELECT * FROM tron_db.address_relationships FINAL").execute().await?;
    validate_tron_schema(&client).await?;
    validate_tron_schema(&client).await?;
    let from = "TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ";
    let to = "TMeWat4Y7Sx8bfskXt1R5nDV3ZuiTDxr2N";
    for (block, state) in [
        (1u64, "COMPLETE"),
        (2, "FAILED"),
        (3, "PROCESSING"),
        (4, "MISSING"),
    ] {
        if state != "MISSING" {
            client.query("INSERT INTO ingested_blocks (block_number,block_hash,parent_hash,block_timestamp,transaction_count,finality_status,ingestion_status,indexed_at_unix_ms,updated_at) VALUES (?,?,'parent',1700000000000,1,'SOLID',?,1,toDateTime64('2026-01-01 00:00:00',3))")
                .bind(block).bind(format!("hash-{block}")).bind(state).execute().await?;
        }
        client.query("INSERT INTO transactions (tx_hash,block_number,timestamp,initiator_address,target_address,status) VALUES (?,?,1700000000000,?,?,1)")
            .bind(format!("tx-{block}")).bind(block).bind(from).bind(to).execute().await?;
        client.query("INSERT INTO address_relationships (relationship_id,from_address,to_address,token_address,tx_hash,block_number,timestamp,amount,transfer_type) VALUES (?,?,?,'TRX',?,?,1700000000000,100,'native_transfer')")
            .bind(format!("edge-{block}")).bind(from).bind(to).bind(format!("tx-{block}")).bind(block).execute().await?;
        client.query("INSERT INTO semantic_aml_events (event_id,tx_hash,block_number,timestamp,event_type,subject_address,protocol,detector,detector_version,confidence,evidence_json) VALUES (?,?,?,1700000000000,'test_event',?,'test','fixture','1',0.5,'{}')")
            .bind(format!("event-{block}")).bind(format!("tx-{block}")).bind(block).bind(to).execute().await?;
    }
    for table in [
        "address_relationships_canonical",
        "transactions_canonical",
        "semantic_aml_events_canonical",
    ] {
        let count = client
            .query(&format!("SELECT count() FROM {table}"))
            .fetch_one::<u64>()
            .await?;
        assert_eq!(
            count, 1,
            "{table} must exclude failed, processing and missing-journal facts"
        );
    }
    let graph = build_wallet_flow_graph(client.clone(), None, to, 2, 100).await?;
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].block_number, 1);
    let activity = build_wallet_activity(client.clone(), to, None, Some(100)).await?;
    assert_eq!(activity.recent_semantic_events.len(), 1);
    build_wallet_holdings(client.clone(), to, None).await?;
    assert_eq!(client.query("SELECT toString(balance_raw) FROM wallet_asset_balances WHERE address=? AND asset_id='TRX'")
        .bind(to).fetch_one::<String>().await?, "100");
    client.query("INSERT INTO ingested_blocks (block_number,block_hash,parent_hash,block_timestamp,transaction_count,finality_status,ingestion_status,indexed_at_unix_ms,updated_at) VALUES (2,'hash-2','parent',1700000000000,1,'SOLID','COMPLETE',2,toDateTime64('2026-01-01 00:00:01',3))").execute().await?;
    assert_eq!(
        build_wallet_flow_graph(client.clone(), None, to, 2, 100)
            .await?
            .edges
            .len(),
        2
    );
    assert_eq!(client.query("SELECT toString(balance_raw) FROM wallet_asset_balances WHERE address=? AND asset_id='TRX'")
        .bind(to).fetch_one::<String>().await?, "200", "MV deltas must become visible once, without re-insertion");
    assert_eq!(
        client
            .query("SELECT count() FROM address_relationships FINAL")
            .fetch_one::<u64>()
            .await?,
        4,
        "view upgrade must retain raw evidence"
    );
    Ok(())
}
