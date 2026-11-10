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

fn fixture_bridge_log() -> Value {
    let data = format!(
        "0x{}{:0>64}{:0>64}{:0>64}{:064x}{:064x}{}",
        "2".repeat(64),
        "3".repeat(40),
        "4".repeat(40),
        "8ac76a51cc950d9822d68b83fe1ad97b32cd580d",
        99_000_000_000_000_000_000u128,
        1,
        "1".repeat(64)
    );
    json!({"address":"0xdd90e5e87a2081dcf0391920868ebc2ffb81a1af",
        "topics":["0x79fa08de5149d912dce8e5e8da7a7c17ccdf23dd5d3bfe196802e6eb86347c7c"],
        "data":data,"blockNumber":"0x64","blockHash":BLOCK_HASH,"transactionHash":TX_SUCCESS,
        "transactionIndex":"0x0","logIndex":"0x1","removed":false})
}

fn fixture_allbridge_log() -> Value {
    use aml_chain_api::bridge::allbridge;
    let mut log = fixture_bridge_log();
    log["address"] = json!(allbridge::emitter(aml_chain_api::Network::Bsc));
    log["topics"] = json!([format!("0x{}", allbridge::SEND)]);
    log["logIndex"] = json!("0x2");
    log["data"] = json!(format!(
        "0x{:064x}{:064x}{:064x}{:0>64}{:064x}{:064x}",
        100_000, 2025, 3, "a614f803b6fd780986a42c78ec9c7f77e6ded13c", 9, 1
    ));
    log
}

#[tokio::test]
#[ignore = "requires a disposable BSC_TEST_CLICKHOUSE_URL and fixture credentials"]
async fn bridge_reads_use_canonical_receipt_logs() -> anyhow::Result<()> {
    use crate::{
        config::ClickHouseConfig,
        db::warehouse::Warehouse,
        investigation::api::{ApiState, router},
        metadata::TokenRpc,
    };
    use aml_chain_api::{
        Network,
        bridge::{Observation, Side, Window, allbridge},
    };
    let (admin, database) = disposable_database("bridge_logs").await;
    let (rpc_url, state, rpc_server) = start_mock().await;
    state.bridge.store(true, Ordering::SeqCst);
    let config = ClickHouseConfig::fixture(
        &env::var("BSC_TEST_CLICKHOUSE_URL")?,
        &env::var("BSC_TEST_CLICKHOUSE_USER")?,
        &env::var("BSC_TEST_CLICKHOUSE_PASSWORD")?,
        &database,
    );
    let mut app_config = test_app_config(&rpc_url);
    app_config.trace_mode = TraceMode::Disabled;
    let ingestor = CanonicalIngestor::new(
        &app_config,
        test_ingestion_config(),
        ClickHouseIngestionStore::new(&config),
        "bridge-log-fixture".into(),
    )?;
    ingestor.ingest_range(99, 100).await?;
    let key = "bsc-bridge-fixture-key-0123456789abcdef";
    let api = router(ApiState::new(
        Warehouse::new(config)?,
        key.into(),
        TokenRpc::new(app_config)?,
    )?);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, api).await.unwrap() });
    let client = reqwest::Client::new();
    let transaction = format!("{base}/internal/bridge/transactions/{TX_SUCCESS}?block_number=100");
    assert_eq!(
        client.get(&transaction).send().await?.status(),
        axum::http::StatusCode::UNAUTHORIZED
    );
    let request = |url| client.get(url).header("X-AML-Service-Key", key);
    let anchor: Observation = request(transaction.clone())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(anchor.logs.len(), 2);
    let log = anchor.logs.iter().find(|log| log.log_index == 1).unwrap();
    let allbridge = anchor.logs.iter().find(|log| log.log_index == 2).unwrap();
    assert!(allbridge::decode(Network::Bsc, allbridge).is_some());
    let snapshot: Value = request(format!("{base}/internal/bridge/snapshot"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(snapshot, anchor.source_snapshot);
    let leg = log.decode(Network::Bsc).expect("Celer relay must decode");
    assert_eq!(leg.operation_id, format!("0x{}", "1".repeat(64)));
    let operation = format!("{base}/internal/bridge/operations/{}", leg.operation_id);
    let mut window = Window {
        side: Side::Relay,
        from_ms: log.timestamp_ms - 1,
        to_ms: log.timestamp_ms,
    };
    let hit: Observation = request(operation.clone())
        .query(&window)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(hit.source_snapshot, anchor.source_snapshot);
    assert_eq!(hit.logs[0].tx_hash, TX_SUCCESS);
    window.side = Side::Send;
    let allbridge_hit: Observation = request(format!(
        "{base}/internal/bridge/allbridge/operations/0x{:064x}",
        9
    ))
    .query(&window)
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(allbridge_hit.source_snapshot, anchor.source_snapshot);
    assert_eq!(allbridge_hit.logs.len(), 1);
    assert_eq!(allbridge_hit.logs[0].log_index, 2);
    let wrong_side: Observation = request(operation.clone())
        .query(&window)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(wrong_side.logs.is_empty());
    window.side = Side::Relay;
    window.to_ms = window.from_ms;
    let outside: Observation = request(operation.clone())
        .query(&window)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(outside.logs.is_empty());
    let invalid = request(operation)
        .query(&[("side", "relay"), ("from_ms", "1"), ("to_ms", "9999999999")])
        .send()
        .await?;
    assert_eq!(invalid.status(), axum::http::StatusCode::BAD_REQUEST);
    state.reorged.store(true, Ordering::SeqCst);
    let recovery = ingestor.sync_once(None, Some(1)).await?;
    assert_eq!(recovery.reorg_repaired_from, Some(100));
    let displaced: Observation = request(transaction)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(displaced.logs.is_empty());
    assert_ne!(displaced.source_snapshot, anchor.source_snapshot);
    server.abort();
    rpc_server.abort();
    drop(ingestor);
    admin
        .query(&format!("DROP DATABASE {database}"))
        .execute()
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a disposable BSC_TEST_CLICKHOUSE_URL and fixture credentials"]
async fn bounded_cluster_leads_reach_wallet_api_without_becoming_attribution() -> anyhow::Result<()>
{
    use crate::{
        config::ClickHouseConfig,
        db::warehouse::Warehouse,
        investigation::api::{ApiState, router},
        metadata::TokenRpc,
    };
    let (admin, database) = disposable_database("cluster_leads").await;
    let (rpc_url, state, server) = start_mock().await;
    state.cluster_transfers.store(true, Ordering::SeqCst);
    let config = ClickHouseConfig::fixture(
        &env::var("BSC_TEST_CLICKHOUSE_URL")?,
        &env::var("BSC_TEST_CLICKHOUSE_USER")?,
        &env::var("BSC_TEST_CLICKHOUSE_PASSWORD")?,
        &database,
    );
    let db = Warehouse::new(config.clone())?;
    let mut app_config = test_app_config(&rpc_url);
    app_config.trace_mode = TraceMode::Disabled;
    let ingestor = CanonicalIngestor::new(
        &app_config,
        test_ingestion_config(),
        ClickHouseIngestionStore::new(&config),
        "cluster-leads-fixture".into(),
    )?;
    ingestor.ingest_range(100, 100).await?;
    let key = "bsc-fixture-service-key-0123456789abcdef";
    let app = router(ApiState::new(db, key.into(), TokenRpc::new(app_config)?)?);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!(
        "http://{}/api/bsc/wallet/0x{}",
        listener.local_addr()?,
        "a".repeat(40)
    );
    let api = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let get = |suffix: &str| {
        client
            .get(format!("{url}{suffix}"))
            .header("X-AML-Service-Key", key)
            .send()
    };
    let wallet: Value = get("/investigation")
        .await?
        .error_for_status()?
        .json()
        .await?;
    let dedicated: Value = get("/cluster-candidates")
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(wallet["cluster_candidates"], dedicated["candidates"]);
    let lead = &wallet["cluster_candidates"][0];
    assert_eq!(wallet["cluster_candidate_coverage"]["status"], "available");
    assert_eq!(lead["destination"], CLUSTER_DESTINATION);
    assert_eq!(lead["distinct_senders"], 3);
    assert_eq!(lead["review_status"], "pending");
    assert_eq!(lead["evidence_refs"].as_array().unwrap().len(), 3);
    assert!(
        lead["evidence_refs"]
            .as_array()
            .unwrap()
            .contains(&lead["subject_relationship_id"])
    );
    assert_eq!(
        wallet["analysis_evidence"]["clustering"]["items"][0]["kind"],
        "investigative_lead"
    );
    assert_eq!(
        wallet["analysis_evidence"]["clustering"]["items"][0]["score_eligible"],
        false
    );
    assert_eq!(wallet["evidence"]["attribution"]["items"], json!([]));
    let filtered: Value = get("/investigation?from_ms=1700000000001")
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(filtered["cluster_candidates"], json!([]));
    api.abort();
    server.abort();
    drop(ingestor);
    admin
        .query(&format!("DROP DATABASE {database}"))
        .execute()
        .await?;
    Ok(())
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
    let probe = CanonicalIngestor::probe_history(&app_config, test_ingestion_config()).await?;
    assert_eq!(probe["full_history_verified"], false);
    assert_eq!(probe["history_samples"][0]["transactions"], 3);
    use aml_warehouse::maintenance::Command;
    assert_eq!(
        db.maintenance_read(
            &Command::parse(&[
                "scan-gaps".into(),
                "--from-block".into(),
                "100".into(),
                "--to-block".into(),
                "100".into(),
                "--max-blocks".into(),
                "1".into()
            ])?,
            false
        )
        .await?["gaps"],
        0
    );
    let source_before = db.snapshot().await?;
    aml_warehouse::Connection::execute(&native,"ALTER TABLE aml_transactions DELETE WHERE block_number=100 AND transaction_index=0 SETTINGS mutations_sync=2").await?;
    let scan = Command::parse(&[
        "scan-gaps".into(),
        "--from-block".into(),
        "100".into(),
        "--to-block".into(),
        "100".into(),
        "--max-blocks".into(),
        "1".into(),
    ])?;
    assert_eq!(db.maintenance_read(&scan, false).await?["gaps"], 1);
    let mut repair = scan.clone();
    repair.action = "repair".into();
    assert_eq!(ingestor.maintenance(&repair).await?["status"], "complete");
    assert_eq!(db.maintenance_read(&scan, false).await?["gaps"], 0);
    assert_eq!(
        db.snapshot().await?.state().last_block_number,
        source_before.state().last_block_number
    );
    assert!(db.snapshot().await?.state().revision > source_before.state().revision);
    state.chain_id.store(1, Ordering::SeqCst);
    assert!(
        CanonicalIngestor::probe_history(&app_config, test_ingestion_config())
            .await
            .is_err()
    );
    repair.action = "replay".into();
    assert!(ingestor.maintenance(&repair).await.is_err());
    state.chain_id.store(56, Ordering::SeqCst);
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
    let first: Value = http
        .get(format!("{api_url}/api/bsc/wallet/{b}/transfers?limit=1"))
        .header("X-AML-Service-Key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    let second: Value = http
        .get(format!(
            "{api_url}/api/bsc/wallet/{b}/transfers?limit=1&cursor={cursor}"
        ))
        .header("X-AML-Service-Key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(first["source_snapshot"], second["source_snapshot"]);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
    let filtered: Value = http
        .get(format!(
            "{api_url}/api/bsc/wallet/{b}/transfers?limit=100&asset_id=native:BNB"
        ))
        .header("X-AML-Service-Key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(!filtered["items"].as_array().unwrap().is_empty());
    assert!(
        filtered["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["asset_id"] == "native:BNB" && r["amount"].is_string())
    );
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
        if route.ends_with("/investigation") {
            let semantics = &value["semantic_evidence"];
            assert_eq!(semantics["contract"], "chain_semantic_evidence_v1");
            assert!(
                semantics["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["grade"] == "observed_transfer"
                        && item["token_id"] == aml_warehouse::RawAmount::MAX
                        && item["amount_raw"] == "1")
            );
        }
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
    if let Ok(path) = std::env::var("CHAIN_RESPONSE_PROOF") {
        let wallet: Value = http
            .get(format!("{api_url}/api/bsc/wallet/{a}/investigation"))
            .header("X-AML-Service-Key", key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(
            !wallet["evidence"]["attribution"]["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let paths: Value = http
            .get(format!("{api_url}/api/bsc/wallet/{a}/paths/{b}?max_hops=3"))
            .header("X-AML-Service-Key", key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let status: Value = http
            .get(format!("{api_url}/status"))
            .header("X-AML-Service-Key", key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        std::fs::write(
            path,
            serde_json::to_vec(
                &json!({"wallet":wallet,"paths":paths,"receipts":{"/status":status}}),
            )?,
        )?;
    }
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
    let stale = http
        .get(format!(
            "{api_url}/api/bsc/wallet/{b}/transfers?limit=1&cursor={cursor}"
        ))
        .header("X-AML-Service-Key", key)
        .send()
        .await?;
    assert_eq!(stale.status(), 409);
    assert_eq!(stale.json::<Value>().await?["code"], "snapshot_changed");
    api_server.abort();
    rpc_server.abort();
    drop(ingestor);
    admin
        .query(&format!("DROP DATABASE {database}"))
        .execute()
        .await?;
    Ok(())
}
