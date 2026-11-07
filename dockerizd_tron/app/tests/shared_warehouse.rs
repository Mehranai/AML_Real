//! Covers the ClickHouse 0.14.1 native driver used by TRON and Ethereum.
use aml_warehouse::{Connection, Network, initialize, validate};
use anyhow::Result;
use clickhouse::Client;
#[path = "support/source_evidence.rs"]
mod source;

aml_warehouse::clickhouse_connection!(NativeConnection);

#[tokio::test]
#[ignore = "requires WAREHOUSE_TEST_URL pointing to a disposable ClickHouse"]
async fn native_driver_initialization_and_readonly_validation() -> Result<()> {
    let client = Client::default()
        .with_url(std::env::var("WAREHOUSE_TEST_URL")?)
        .with_user("warehouse_test")
        .with_password("warehouse-test-only");
    let database = "warehouse_native_tron";
    assert_eq!(
        NativeConnection(&client)
            .query("SELECT 'literal ? ?? ?fields'")
            .await?,
        vec!["literal ? ?? ?fields".to_owned()]
    );
    assert_eq!(
        NativeConnection(&client)
            .query_bounded("SELECT 'bounded ?' SETTINGS max_execution_time=2,max_threads=1")
            .await?,
        vec!["bounded ?".to_owned()]
    );
    initialize(&NativeConnection(&client), database, Network::Tron).await?;
    let state = aml_warehouse::publication::State {
        network_id: Network::Tron.id().into(),
        revision: 1,
        ..Default::default()
    };
    aml_warehouse::publication::insert(
        &NativeConnection(&client),
        database,
        "aml_sync_state",
        &[aml_warehouse::core_schema::extend(
            "aml_sync_state",
            serde_json::to_value(&state)?,
        )?],
    )
    .await?;
    assert_eq!(
        aml_warehouse::publication::state(&NativeConnection(&client), database, Network::Tron)
            .await?,
        state
    );
    for view in aml_warehouse::core::View::ALL {
        let rows = aml_warehouse::core::read(
            &NativeConnection(&client),
            database,
            Network::Tron,
            view,
            &Default::default(),
        )
        .await?;
        assert_eq!(
            rows.len(),
            usize::from(matches!(view, aml_warehouse::core::View::State))
        );
    }
    for table in [
        "aml_asset_metadata",
        "aml_protocol_registry",
        "aml_entity_addresses",
        "aml_cluster_memberships",
        "aml_wallet_risk_signals",
    ] {
        assert!(
            aml_warehouse::analytics::read(
                &NativeConnection(&client),
                database,
                Network::Tron,
                table,
                aml_warehouse::analytics::Mode::Current,
                10
            )
            .await?
            .is_empty()
        );
    }
    let readonly = client.with_option("readonly", "1");
    let subject = aml_warehouse::relationships::Subject::address(
        Network::Tron,
        "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb",
    )?;
    let empty = aml_warehouse::relationships::read_neighborhood(
        &NativeConnection(&readonly),
        database,
        Network::Tron,
        &subject,
        Default::default(),
    )
    .await?;
    assert_eq!(empty.graph.nodes.len(), 1);
    assert!(empty.graph.edges.is_empty());
    let query = aml_warehouse::relationships::query_template()
        .lines()
        .filter(|l| !l.starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("{{database}}", database)
        .replace("{{network}}", &format!("'{}'", Network::Tron.id()))
        .replace("{{projection}}", "r.relationship_id")
        .replace("{{selector}}", "1=0")
        .replace("{{witness_selector}}", "1=0")
        .replace("{{limit}}", "1");
    assert!(NativeConnection(&readonly).query(&query).await?.is_empty());
    let summary = validate(&NativeConnection(&readonly), database, Network::Tron).await?;
    assert!(!summary.newly_applied);
    assert!(
        initialize(&NativeConnection(&readonly), database, Network::Tron)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires WAREHOUSE_TEST_URL pointing to a disposable ClickHouse"]
async fn rpc_source_facts_round_trip_through_shared_publication_and_retry() -> Result<()> {
    use aml_warehouse::{
        core,
        publication::{self, Block},
    };
    use arz_axum_for_services::{
        helper::tron::TronClient, services::tron::source_evidence::VerifiedTransaction,
    };
    use axum::{Json, Router, routing::post};
    use serde_json::{Value, json};

    let original_tx = source::multi_transaction();
    let original_receipt = source::receipt(&original_tx);
    let rpc_block = json!({"blockID":"a".repeat(64),"block_header":{"raw_data":{"number":source::HEIGHT,"timestamp":source::TIME,"parentHash":"b".repeat(64)}},"transactions":[original_tx]});
    let block_response = rpc_block.clone();
    let receipt_response = original_receipt.clone();
    let expected_id = original_tx["txID"].clone();
    let app = Router::new()
        .route(
            "/wallet/getblockbynum",
            post(move |Json(request): Json<Value>| {
                let response = block_response.clone();
                async move {
                    assert_eq!(request["num"], source::HEIGHT);
                    Json(response)
                }
            }),
        )
        .route(
            "/wallet/gettransactioninfobyid",
            post(move |Json(request): Json<Value>| {
                let response = receipt_response.clone();
                let id = expected_id.clone();
                async move {
                    assert_eq!(request["value"], id);
                    Json(response)
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let rpc_server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let rpc = TronClient::new(&url, None, 3)?;
    let fetched = rpc.get_block(source::HEIGHT).await?;
    assert_eq!(fetched, rpc_block);
    let tx = &fetched["transactions"][0];
    let receipt = rpc.get_tx_receipt(tx["txID"].as_str().unwrap()).await?;
    let verified = VerifiedTransaction::verify(tx, &receipt, source::HEIGHT, source::TIME)?;
    rpc_server.abort();

    let client = Client::default()
        .with_url(std::env::var("WAREHOUSE_TEST_URL")?)
        .with_user("warehouse_test")
        .with_password("warehouse-test-only");
    let db = NativeConnection(&client);
    let database = "warehouse_tron_source";
    initialize(&db, database, Network::Tron).await?;
    let before = publication::state(&db, database, Network::Tron).await?;
    assert_eq!(before.revision, 0, "source test requires a fresh database");
    let mut block = Block {
        network_id: Network::Tron.id().into(),
        block_number: source::HEIGHT,
        block_hash: fetched["blockID"].as_str().unwrap().into(),
        block_timestamp_unix_ms: source::TIME,
        commit_id: "interrupted-source-attempt".into(),
        revision: publication::next_revision(&db, database, Network::Tron, 0).await?,
        parent_hash: "b".repeat(64),
        canonical: 1,
        ingestion_status: "complete".into(),
        transaction_count: 1,
        receipt_coverage: Some(1),
        trace_coverage: None,
        prevout_coverage: None,
        source_id: "synthetic-tron-rpc".into(),
        created_at_unix_ms: 99,
    };
    let assemble = |block: &Block| -> Result<std::collections::BTreeMap<String, Vec<Value>>> {
        let mut facts = verified.facts(block, 0, 99)?;
        facts.insert("aml_transactions".into(),vec![json!({"network_id":block.network_id,"block_number":block.block_number,"block_hash":block.block_hash,"block_timestamp_unix_ms":block.block_timestamp_unix_ms,"commit_id":block.commit_id,"revision":block.revision,"tx_hash":verified.tx_hash(),"transaction_index":0,"from_address":null,"to_address":null,"amount_raw":null,"fee_raw":"0","asset_id":"native:TRX","decimals":6,"status":"confirmed","coinbase":0,"input_count":null,"output_count":null,"raw_evidence_json":serde_json::to_string(tx)?,"created_at_unix_ms":99})]);
        Ok(facts)
    };
    let facts = assemble(&block)?;
    core::validate_batch(&before, &block, &facts, &serde_json::to_string(&fetched)?)?;
    publication::reserve(&db, database, &block).await?;
    for (table, rows) in &facts {
        publication::facts(&db, database, &block, table, rows).await?;
    }
    // A process stopped after writing source facts must expose none of them.
    for table in [
        "aml_tron_contracts",
        "aml_tron_logs",
        "aml_tron_internals",
        "aml_tron_resources",
        "aml_raw_transactions",
    ] {
        assert_eq!(
            db.query(&format!(
                "SELECT toString(count()) FROM {database}.{table}_canonical"
            ))
            .await?,
            vec!["0"]
        );
    }
    assert_eq!(
        publication::state(&db, database, Network::Tron).await?,
        before
    );
    block.revision =
        publication::next_revision(&db, database, Network::Tron, before.revision).await?;
    block.commit_id = "retried-source-attempt".into();
    let expected = assemble(&block)?;
    let published = core::publish_observed_batch(
        &db,
        database,
        &before,
        &block,
        &expected,
        &serde_json::to_string(&fetched)?,
        source::HEIGHT + 5,
    )
    .await?;
    assert_eq!(published.last_block_number, source::HEIGHT);
    assert_eq!(
        publication::state(&db, database, Network::Tron).await?,
        published
    );
    for (table, rows) in &expected {
        let contract = aml_warehouse::current_contract()?;
        let shape = contract.tables.iter().find(|t| t.name == *table).unwrap();
        let sql = format!(
            "SELECT toJSONString(tuple({})) FROM {database}.{table}_canonical ORDER BY {}",
            core::projection(&shape.columns),
            shape.order_by.join(",")
        );
        let actual = core::decode_rows(table, Network::Tron, db.query(&sql).await?)?;
        assert_eq!(
            actual, *rows,
            "all original values/positions/NULLs must round-trip: {table}"
        );
    }
    let mut forged = tx.clone();
    forged["raw_data"]["contract"][0]["parameter"]["value"]["amount"] = json!(1);
    assert!(VerifiedTransaction::verify(&forged, &receipt, source::HEIGHT, source::TIME).is_err());
    assert_eq!(
        publication::state(&db, database, Network::Tron).await?,
        published
    );
    assert!(
        !initialize(&db, database, Network::Tron)
            .await?
            .newly_applied
    );
    Ok(())
}
