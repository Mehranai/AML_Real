//! Live RPC -> real block writer -> shared readers. No native fact seeding.
use aml_warehouse::{Connection, Network, publication};
use anyhow::Result;
use arz_axum_for_services::{
    db::sync_state::get_last_synced_block,
    services::{
        loader::LoaderTron,
        tron::{
            fetcher::{ingest_tron_range, replay_tron_range},
            ingestion_health::{IngestionHealthOptions, load_tron_ingestion_health},
            wallet_investigation::{WalletInvestigationOptions, build_wallet_investigation},
        },
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
#[path = "support/live_ingestion.rs"]
mod fixture;
use fixture::{Rpc, scalar, setup, source};

#[tokio::test]
#[ignore = "requires a fresh disposable WAREHOUSE_TEST_URL"]
async fn live_publication_keeps_exact_positions_nft_resources_and_existing_readers() -> Result<()> {
    let rpc = Rpc::start().await?;
    let success = fixture::salted(source::multi_transaction(), 1);
    let mut failed = fixture::simple(42, 2);
    failed["ret"][0]["contractRet"] = json!("REVERT");
    rpc.block(100, vec![success.clone(), failed.clone()]);
    rpc.block(101, vec![]);
    rpc.receipt(100, &success, true);
    rpc.receipt(100, &failed, true);
    {
        let mut receipts = rpc.state.receipts.write().unwrap();
        let info = receipts.get_mut(success["txID"].as_str().unwrap()).unwrap();
        info["log"].as_array_mut().unwrap().push(json!({"address":&source::TARGET[2..],"topics":["ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef","0".repeat(64),format!("{}{}","0".repeat(24),&source::OWNER[2..])],"data":format!("{:064x}",1)}));
    }
    rpc.state
        .delays
        .write()
        .unwrap()
        .insert(success["txID"].as_str().unwrap().into(), 400);
    let (config, client) = setup("live", &rpc).await?;
    ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 101).await?;
    assert_eq!(get_last_synced_block(&client).await?, Some(101));
    let state = publication::state(&*client, client.database(), Network::Tron).await?;
    assert_eq!(state.start_block, 100);
    let txs=client.query("SELECT toJSONString(tuple(tx_hash,transaction_index,status)) FROM aml_transactions_canonical ORDER BY transaction_index").fetch_all::<String>().await?;
    assert_eq!(
        serde_json::from_str::<Value>(&txs[0])?,
        json!([success["txID"], 0, "success"])
    );
    assert_eq!(
        serde_json::from_str::<Value>(&txs[1])?,
        json!([failed["txID"], 1, "failed"])
    );
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_address_relationships_canonical"
        )
        .await?,
        8
    );
    assert_eq!(scalar(&client,"SELECT toString(count()) FROM aml_address_relationships_canonical WHERE transaction_index=1").await?,0);
    let nft=client.query("SELECT toJSONString(tuple(asset_id,toString(token_id),toString(amount_raw),event_index,event_sub_index)) FROM aml_address_relationships_canonical WHERE transfer_type='trc721_transfer'").fetch_one::<String>().await?;
    assert_eq!(
        serde_json::from_str::<Value>(&nft)?,
        json!([
            format!(
                "trc721:{}/{}",
                source::TARGET,
                aml_warehouse::RawAmount::MAX
            ),
            aml_warehouse::RawAmount::MAX,
            "1",
            2,
            0
        ])
    );
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_tron_logs_canonical"
        )
        .await?,
        7
    );
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_tron_internals_canonical"
        )
        .await?,
        6
    );
    assert_eq!(scalar(&client,"SELECT toString(count()) FROM aml_tron_resources_canonical WHERE isNull(energy_fee) AND net_usage=0").await?,2);
    assert_eq!(scalar(&client,"SELECT toString(count()) FROM aml_semantic_aml_events_canonical WHERE event_type='mint'").await?,1);
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM token_metadata_discoveries"
        )
        .await?,
        1
    );
    let owner = aml_warehouse::values::tron_wallet(source::OWNER)?;
    let investigation = build_wallet_investigation(
        client.clone(),
        &owner,
        WalletInvestigationOptions::new(Some(1), Some(100), Some(0), None, None, None),
    )
    .await?;
    assert!(!investigation.graph.edges.is_empty());
    assert_eq!(investigation.source_snapshot["as_of_block"], 101);
    assert_eq!(investigation.source_snapshot["history_complete"], false);
    assert!(investigation.holdings.source.contains("not_rpc_balance"));
    let token = investigation
        .holdings
        .assets
        .iter()
        .find(|a| a.asset_type == "trc20")
        .unwrap();
    assert_eq!(
        token.balance_raw,
        ("1".parse::<num_bigint::BigInt>()?
            - aml_warehouse::RawAmount::MAX.parse::<num_bigint::BigInt>()?)
        .to_string()
    );
    assert!(
        token.decimals.is_none() && token.balance_decimal.is_none() && token.balance_incomplete
    );
    let health = load_tron_ingestion_health(
        &config,
        &client,
        IngestionHealthOptions::bounded(None, None, None),
    )
    .await?;
    assert_eq!(health.block_journal.missing_block_count, 0);
    assert_eq!(health.block_journal.gap_window_start, Some(100));
    for table in [
        "transactions",
        "address_relationships",
        "semantic_aml_events",
        "transaction_features",
        "ingested_blocks",
        "sync_state",
        "token_metadata_discoveries",
        "ingestion_failures",
    ] {
        assert_eq!(
            Connection::query(
                &*client,
                &format!(
                    "SELECT toString(count()) FROM system.tables WHERE database='{}' AND name='{table}'",
                    client.database()
                )
            )
            .await?,
            vec!["0"],
            "native fact/cursor writer remained: {table}"
        );
    }
    let restarted = Arc::new(LoaderTron::new(&config).await?);
    let calls = rpc.state.receipt_calls.load(Ordering::SeqCst);
    ingest_tron_range(restarted.clone(), 100, 101).await?;
    assert_eq!(rpc.state.receipt_calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        publication::state(&*client, client.database(), Network::Tron).await?,
        state
    );
    replay_tron_range(restarted, 100, 100).await?;
    let replayed = publication::state(&*client, client.database(), Network::Tron).await?;
    assert_eq!(replayed.last_block_number, 101);
    assert!(replayed.revision > state.revision);
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_address_relationships_canonical"
        )
        .await?,
        8
    );
    let report = arz_axum_for_services::services::tron::performance::run_historical_benchmark(
        &config,
        Arc::new(LoaderTron::new(&config).await?),
        100,
        101,
        Some(owner),
    )
    .await?;
    assert_eq!(report.status, "NOOP");
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_ingestion_benchmarks_current WHERE status='complete'"
        )
        .await?,
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires a fresh disposable WAREHOUSE_TEST_URL"]
async fn invalid_transaction_or_receipt_never_partially_publishes_and_can_retry() -> Result<()> {
    let rpc = Rpc::start().await?;
    let valid = fixture::simple(10, 11);
    let mut bad = fixture::simple(20, 12);
    bad["raw_data"]["contract"][0]["parameter"]["value"]["amount"] = json!(999);
    rpc.block(100, vec![valid.clone(), bad.clone()]);
    rpc.receipt(100, &valid, false);
    rpc.receipt(100, &bad, false);
    let (config, client) = setup("invalid", &rpc).await?;
    assert!(
        ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100)
            .await
            .is_err()
    );
    assert_eq!(get_last_synced_block(&client).await?, None);
    assert_eq!(
        scalar(&client, "SELECT toString(count()) FROM aml_transactions").await?,
        0,
        "validation must finish before any fact IO"
    );
    assert_eq!(scalar(&client,"SELECT toString(count()) FROM aml_ingestion_failures_current WHERE status='dead_letter'").await?,1);
    let mut repaired = fixture::simple(20, 12);
    repaired["txID"] = json!(repaired["txID"].as_str().unwrap().to_ascii_uppercase());
    rpc.block(100, vec![valid, repaired.clone()]);
    rpc.receipt(100, &repaired, false);
    rpc.state
        .receipts
        .write()
        .unwrap()
        .get_mut(repaired["txID"].as_str().unwrap())
        .unwrap()["id"] = json!("f".repeat(64));
    assert!(
        ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100)
            .await
            .is_err()
    );
    assert_eq!(get_last_synced_block(&client).await?, None);
    rpc.receipt(100, &repaired, false);
    ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100).await?;
    assert_eq!(get_last_synced_block(&client).await?, Some(100));
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_ingestion_failures_current WHERE status!='resolved'"
        )
        .await?,
        0
    );
    let mut published = publication::state(&*client, client.database(), Network::Tron).await?;
    // A successful header check/skip cannot mark a failed receipt replay resolved.
    rpc.state
        .receipts
        .write()
        .unwrap()
        .get_mut(repaired["txID"].as_str().unwrap())
        .unwrap()["id"] = json!("f".repeat(64));
    assert!(
        replay_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100)
            .await
            .is_err()
    );
    rpc.receipt(100, &repaired, false);
    let calls = rpc.state.receipt_calls.load(Ordering::SeqCst);
    arz_axum_for_services::services::tron::fetcher::fetch_tron(
        Arc::new(LoaderTron::new(&config).await?),
        101,
        0,
    )
    .await?;
    ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100).await?;
    assert_eq!(rpc.state.receipt_calls.load(Ordering::SeqCst), calls);
    assert_eq!(scalar(&client, "SELECT toString(count()) FROM aml_ingestion_failures_current WHERE operation='PROCESS_TX' AND status!='resolved'").await?, 1);
    assert_eq!(
        publication::state(&*client, client.database(), Network::Tron).await?,
        published
    );
    replay_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100).await?;
    let retried = publication::state(&*client, client.database(), Network::Tron).await?;
    assert!(retried.revision > published.revision);
    published = retried;
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_ingestion_failures_current WHERE status!='resolved'"
        )
        .await?,
        0
    );
    rpc.state.blocks.write().unwrap().get_mut(&100).unwrap()["blockID"] = json!("d".repeat(64));
    assert!(
        replay_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100)
            .await
            .is_err()
    );
    assert_eq!(
        publication::state(&*client, client.database(), Network::Tron).await?,
        published
    );
    // Ordinary skips and the follow loop's empty poll must verify the same tip.
    assert!(
        ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 100, 100)
            .await
            .is_err()
    );
    assert!(
        arz_axum_for_services::services::tron::fetcher::fetch_tron(
            Arc::new(LoaderTron::new(&config).await?),
            101,
            0
        )
        .await
        .is_err()
    );
    assert_eq!(
        publication::state(&*client, client.database(), Network::Tron).await?,
        published
    );
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_transactions_canonical"
        )
        .await?,
        2
    );
    rpc.state.blocks.write().unwrap().get_mut(&100).unwrap()["blockID"] = json!(fixture::hash(100));
    rpc.state.head.store(99, Ordering::SeqCst);
    assert!(
        arz_axum_for_services::services::tron::fetcher::fetch_tron(
            Arc::new(LoaderTron::new(&config).await?),
            101,
            0
        )
        .await
        .is_err()
    );
    rpc.state.head.store(100, Ordering::SeqCst);
    rpc.block(102, vec![]);
    assert!(
        ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 102, 102)
            .await
            .is_err()
    );
    rpc.block(101, vec![]);
    rpc.state.blocks.write().unwrap().get_mut(&101).unwrap()["block_header"]["raw_data"]["parentHash"] =
        json!("c".repeat(64));
    assert!(
        ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 101, 101)
            .await
            .is_err()
    );
    assert_eq!(
        publication::state(&*client, client.database(), Network::Tron).await?,
        published
    );
    // Restore the source, then resume in a fresh Loader and read the recovered data.
    rpc.block(101, vec![]);
    ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 101, 101).await?;
    assert_eq!(get_last_synced_block(&client).await?, Some(101));
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_transactions_canonical"
        )
        .await?,
        2
    );
    let owner = aml_warehouse::values::tron_wallet(source::OWNER)?;
    let investigation = build_wallet_investigation(
        client.clone(),
        &owner,
        WalletInvestigationOptions::new(Some(1), Some(100), Some(0), None, None, None),
    )
    .await?;
    assert_eq!(investigation.source_snapshot["as_of_block"], 101);
    assert!(!investigation.graph.edges.is_empty());
    Ok(())
}

#[tokio::test]
#[ignore = "requires a fresh disposable WAREHOUSE_TEST_URL"]
async fn genesis_empty_blocks_and_unambiguous_legacy_trc10_keep_original_evidence() -> Result<()> {
    let rpc = Rpc::start().await?;
    let genesis = source::transaction(vec![source::contract(
        1,
        "TransferContract",
        &[
            source::bytes(1, b"0x000000000000000000000"),
            source::bytes(2, &hex::decode(source::TARGET)?),
            source::integer(3, 100),
        ]
        .concat(),
        json!({"owner_address":hex::encode("0x000000000000000000000"),"to_address":source::TARGET,"amount":100}),
    )]);
    rpc.block(0, vec![genesis]);
    rpc.state.blocks.write().unwrap().get_mut(&0).unwrap()["block_header"]["raw_data"]
        .as_object_mut()
        .unwrap()
        .remove("number");
    rpc.block(1, vec![]);
    let (config, client) = setup("genesis", &rpc).await?;
    ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 0, 1).await?;
    assert_eq!(rpc.state.receipt_calls.load(Ordering::SeqCst), 0);
    assert_eq!(scalar(&client,"SELECT toString(count()) FROM aml_transactions_canonical WHERE status='unknown' AND isNull(from_address) AND isNull(amount_raw)").await?,1);
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_tron_resources_canonical"
        )
        .await?,
        0
    );
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_address_relationships_canonical"
        )
        .await?,
        0
    );
    let name = hex::encode("OLD_COIN");
    let legacy = fixture::salted(
        source::transaction(vec![source::contract(
            2,
            "TransferAssetContract",
            &[
                source::bytes(1, b"OLD_COIN"),
                source::bytes(2, &hex::decode(source::OWNER)?),
                source::bytes(3, &hex::decode(source::TARGET)?),
                source::integer(4, 7),
            ]
            .concat(),
            json!({"asset_name":name,"owner_address":source::OWNER,"to_address":source::TARGET,"amount":7}),
        )]),
        20,
    );
    rpc.block(2, vec![legacy.clone()]);
    rpc.receipt(2, &legacy, false);
    assert!(
        ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 2, 2)
            .await
            .is_err(),
        "ambiguous/missing catalog must fail closed"
    );
    assert_eq!(get_last_synced_block(&client).await?, Some(1));
    rpc.state.catalog.write().unwrap().insert(
        name.to_ascii_uppercase(),
        json!({"name":hex::encode("OLD_COIN"),"id":"1000099","owner_address":source::OWNER}),
    );
    ingest_tron_range(Arc::new(LoaderTron::new(&config).await?), 2, 2).await?;
    let row=client.query("SELECT toJSONString(tuple(asset_id,toString(amount_raw),raw_evidence_json)) FROM aml_address_relationships_canonical").fetch_one::<String>().await?;
    let row: Value = serde_json::from_str(&row)?;
    assert_eq!(row[0], "trc10:1000099");
    assert_eq!(row[1], "7");
    assert!(
        row[2]
            .as_str()
            .unwrap()
            .contains("current_identity_catalog_not_historical_state")
    );
    Ok(())
}
