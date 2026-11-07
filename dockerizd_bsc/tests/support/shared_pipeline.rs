// Included in ingestion::service::tests to exercise the actual ingestor and API.
// All RPC/intelligence data and credentials belong to explicit disposable fixtures.
fn fixture_abi_string(s: &str) -> String {
    format!(
        "0x{:064x}{:064x}{}{}",
        32,
        s.len(),
        s.as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "0".repeat((32 - s.len() % 32) % 32 * 2)
    )
}
fn fixture_nft_logs() -> Vec<Value> {
    let topic_a = format!("0x{}{}", "0".repeat(24), "a".repeat(40));
    let topic_b = format!("0x{}{}", "0".repeat(24), "b".repeat(40));
    let max = "f".repeat(64);
    let batch = format!(
        "0x{:064x}{:064x}{:064x}{}{:064x}{:064x}{}{:064x}",
        64, 160, 2, max, 7, 2, max, 2
    );
    vec![
        json!({"address":format!("0x{}","6".repeat(40)),"topics":["0x4a39dc06d4c0dbc64b70af90fd698a233a518aa5d07e595d983b8c0526c8f7fb",topic_a,topic_a,topic_b],"data":batch,"blockNumber":"0x64","blockHash":BLOCK_HASH,"transactionHash":TX_SUCCESS,"transactionIndex":"0x0","logIndex":"0x1","removed":false}),
        json!({"address":format!("0x{}","7".repeat(40)),"topics":["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",topic_a,topic_b,format!("0x{max}")],"data":"0x","blockNumber":"0x64","blockHash":BLOCK_HASH,"transactionHash":TX_SUCCESS,"transactionIndex":"0x0","logIndex":"0x2","removed":false}),
    ]
}
#[tokio::test]
#[ignore = "requires a disposable BSC_TEST_CLICKHOUSE_URL and explicit fixture credentials"]
async fn shared_live_api_metadata_nft_and_revocation() -> anyhow::Result<()> {
    use crate::{
        config::ClickHouseConfig,
        db::warehouse::Warehouse,
        intelligence::{self, Claim},
        investigation::{
            self,
            api::{ApiState, router},
        },
        metadata::{TokenRpc, worker_once},
    };
    use anyhow::{Context, ensure};
    let (admin, database) = disposable_database("pipeline").await;
    let (rpc_url, state, rpc_server) = start_mock().await;
    state.nfts.store(true, Ordering::SeqCst);
    let endpoint = env::var("BSC_TEST_CLICKHOUSE_URL")?;
    let user = env::var("BSC_TEST_CLICKHOUSE_USER")?;
    let password = env::var("BSC_TEST_CLICKHOUSE_PASSWORD")?;
    let config = ClickHouseConfig::fixture(&endpoint, &user, &password, &database);
    let db = Warehouse::new(config.clone())?;
    let native = crate::db::Client::from_inner(admin.clone(), &database);
    let store = ClickHouseIngestionStore::new(&config);
    let mut app_config = test_app_config(&rpc_url);
    app_config.trace_mode = TraceMode::Disabled;
    let ingestor = CanonicalIngestor::new(
        &app_config,
        test_ingestion_config(),
        store.clone(),
        "synthetic-bsc".into(),
    )?;
    assert!(
        CanonicalIngestor::new(
            &app_config,
            test_ingestion_config(),
            store.clone(),
            "competing-fixture".into()
        )
        .is_err()
    );
    let first = ingestor.ingest_range(100, 100).await?;
    assert_eq!(first.log_count, 3);
    assert_eq!(first.relationship_count, 5);
    let before = db.snapshot().await?;
    assert_eq!(before.state().last_block_number, 100);
    assert_eq!(
        native
            .query("SELECT count() FROM aml_evm_traces_canonical")
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert_eq!(
        native
            .query("SELECT count() FROM aml_address_relationships_canonical WHERE token_id=?")
            .bind(aml_warehouse::RawAmount::MAX)
            .fetch_one::<u64>()
            .await?,
        2
    );
    assert_eq!(native.query("SELECT toString(amount_raw) FROM aml_address_relationships_canonical WHERE event_index=1 AND event_sub_index=0").fetch_one::<String>().await?,aml_warehouse::RawAmount::MAX);
    assert_eq!(native.query("SELECT event_sub_index FROM aml_address_relationships_canonical WHERE event_index=1 ORDER BY event_sub_index").fetch_all::<u32>().await?,vec![0,1]);
    assert_eq!(
        native
            .query("SELECT count() FROM aml_address_relationships_canonical WHERE tx_hash=?")
            .bind(TX_FAILED)
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert!(
        native
            .query("SELECT raw_evidence_json FROM aml_ingested_blocks_canonical")
            .fetch_one::<String>()
            .await?
            .contains("extraData")
    );
    assert_eq!(
        native
            .query(
                "SELECT count() FROM aml_raw_transactions_canonical WHERE receipt_json IS NOT NULL"
            )
            .fetch_one::<u64>()
            .await?,
        3
    );
    let rpc = TokenRpc::new(app_config.clone())?;
    assert_eq!(worker_once(&db, &rpc).await?, 3);
    assert_eq!(native.query("SELECT count() FROM aml_asset_metadata_visible WHERE observed_block=100 AND observed_block_hash=?").bind(BLOCK_HASH).fetch_one::<u64>().await?,3);
    drop(ingestor);
    app_config.trace_mode = TraceMode::Required;
    let ingestor = CanonicalIngestor::new(
        &app_config,
        test_ingestion_config(),
        store,
        "synthetic-bsc-restart".into(),
    )?;
    let replay = ingestor.replay_range(100, 100).await?;
    assert_eq!(replay.relationship_count, 6);
    let after = db.snapshot().await?;
    assert_eq!(after.state().last_block_number, 100);
    assert!(after.state().revision > before.state().revision);
    assert_eq!(
        native
            .query("SELECT count() FROM aml_evm_traces_canonical")
            .fetch_one::<u64>()
            .await?,
        4
    );
    assert_eq!(ingestor.sync_once(None, Some(1)).await?.next_block, 101);
    let a = format!("0x{}", "a".repeat(40));
    let b = format!("0x{}", "b".repeat(40));
    let claim: Claim = serde_json::from_value(
        json!({"network_id":"eip155:56","claim_id":"fixture:seed-a","address":a,"claim_kind":"label","entity_id":"fixture:entity-a","entity_name":"Synthetic seed","entity_type":"illicit","address_role":"wallet","confidence":0.95,"risk_level":90,"is_exposure_seed":true,"seed_category":"illicit","source_id":"fixture:source","source_reference":"https://example.invalid/case","evidence_refs":[format!("tx:{TX_SUCCESS}")],"review_status":"approved","reviewed_by":"fixture-analyst","review_note":"Reviewed synthetic fixture"}),
    )?;
    intelligence::import(&db, vec![claim.clone()], false).await?;
    let approved = intelligence::active(&db, &a).await?;
    assert_eq!(approved[0]["is_exposure_seed"], true);
    let report = investigation::investigate(&db, &b, &Default::default())
        .await
        .context("common BSC investigation")?;
    assert!(
        !report["exposure_paths"]
            .as_array()
            .context("missing exposure paths")?
            .is_empty()
    );
    assert_eq!(report["source_snapshot"]["as_of_block"], 100);
    let key = "bsc-fixture-service-key-0123456789abcdef";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let api_url = format!("http://{}", listener.local_addr()?);
    let api = router(ApiState::new(db.clone(), key.into(), rpc.clone())?);
    let api_server = tokio::spawn(async move { axum::serve(listener, api).await.unwrap() });
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(format!("{api_url}/status"))
            .send()
            .await?
            .status()
            .as_u16(),
        401
    );
    for route in [
        "/status".to_owned(),
        format!("/api/bsc/wallet/{b}/investigation"),
        format!("/api/bsc/wallet/{b}/holdings"),
        format!("/api/bsc/wallet/{a}/paths/{b}"),
    ] {
        let response = http
            .get(format!("{api_url}{route}"))
            .header("X-AML-Service-Key", key)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        ensure!(status.is_success(), "API {route}: {status}: {body}");
        let value: Value = serde_json::from_str(&body)?;
        if route.ends_with("/holdings") {
            assert!(
                value["assets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["asset_id"] == "eip155:56/native:bnb" && v["amount"] == "4096")
            );
            assert!(value["assets"].as_array().unwrap().iter().any(|v| {
                v["asset_id"]
                    .as_str()
                    .is_some_and(|id| id.contains("erc721:"))
                    && v["token_id"] == aml_warehouse::RawAmount::MAX
                    && v["amount"] == "1"
            }));
        }
    }
    let mut revoked = claim;
    revoked.review_status = "revoked".into();
    revoked.review_note = "Withdrawn synthetic fixture".into();
    intelligence::import(&db, vec![revoked], false).await?;
    assert!(intelligence::active(&db, &a).await?.is_empty());
    let (paths, _) = intelligence::exposure(&db, &b, 100, crate::db::now_ms()).await?;
    assert!(paths.is_empty());
    state.reorged.store(true, Ordering::SeqCst);
    let recovery = ingestor.sync_once(None, Some(1)).await?;
    assert_eq!(recovery.reorg_repaired_from, Some(100));
    assert_eq!(
        native
            .query("SELECT count() FROM aml_asset_metadata_visible")
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert_eq!(
        native
            .query("SELECT count() FROM aml_address_relationships_canonical")
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert_eq!(native.query("SELECT count() FROM system.tables WHERE database=currentDatabase() AND NOT startsWith(name,'aml_')").fetch_one::<u64>().await?,0);
    api_server.abort();
    rpc_server.abort();
    drop(ingestor);
    admin
        .query(&format!("DROP DATABASE {database}"))
        .execute()
        .await?;
    Ok(())
}
