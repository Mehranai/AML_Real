//! Real ingestion -> governed records -> sealed analytics, with no native schema installed.
use aml_warehouse::{Connection, Network, visibility};
use anyhow::Result;
use arz_axum_for_services::{
    db::{Client, records},
    helper::tron::TronClient,
    services::{
        loader::LoaderTron,
        tron::{
            address_clustering::discover_address_cluster_claims,
            entity_intelligence::{
                EntityLabelSubmission, IntelligenceReviewInput, IntelligenceSourceInput,
                load_wallet_intelligence, register_intelligence_source,
                review_intelligence_subject, submit_entity_label,
            },
            fetcher::{ingest_tron_range, replay_tron_range},
            tron_metadata_worker::worker_once,
            wallet_exposure::load_wallet_exposure_summary,
            wallet_investigation::{WalletInvestigationOptions, build_wallet_investigation},
        },
    },
    tasks::exposure_task::{run_all_exposure_scans, run_exposure_scan},
};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
#[path = "support/live_ingestion.rs"]
mod fixture;
use fixture::{Rpc, scalar, setup, source};
fn hex_address(n: u8) -> String {
    format!("41{}", format!("{n:02x}").repeat(20))
}
fn address(n: u8) -> String {
    aml_warehouse::values::tron_wallet(&hex_address(n)).unwrap()
}
fn transfer(from: u8, to: u8, amount: u64, salt: u64) -> Value {
    fixture::salted(
        source::transaction(vec![source::contract(
            1,
            "TransferContract",
            &[
                source::bytes(1, &hex::decode(hex_address(from)).unwrap()),
                source::bytes(2, &hex::decode(hex_address(to)).unwrap()),
                source::integer(3, amount),
            ]
            .concat(),
            json!({"owner_address":hex_address(from),"to_address":hex_address(to),"amount":amount}),
        )]),
        salt,
    )
}
async fn source_registration(client: &Client, id: &str, active: bool) -> Result<()> {
    register_intelligence_source(
        client,
        IntelligenceSourceInput {
            source_id: id.into(),
            source_name: id.into(),
            source_type: "ANALYST".into(),
            trust_tier: "HIGH".into(),
            reference_url: "https://example.invalid/fixture".into(),
            license: "test".into(),
            is_active: active,
            created_by: "fixture-reviewer".into(),
        },
    )
    .await?;
    Ok(())
}
async fn label(client: &Client, n: u8, kind: &str, risk: u8) -> Result<String> {
    Ok(submit_entity_label(
        client,
        EntityLabelSubmission {
            address: address(n),
            label_id: String::new(),
            entity_id: format!("fixture:{n}"),
            entity_name: format!("Fixture {n}"),
            entity_type: kind.into(),
            address_role: if n == 4 { "HOT" } else { "UNKNOWN" }.into(),
            confidence: 0.99,
            risk_percent: risk,
            source: "fixture_labels".into(),
            source_record_id: format!("record-{n}"),
            supersedes_label_id: String::new(),
            submitted_by: "fixture-analyst".into(),
            case_id: "case-fixture".into(),
            evidence_refs: vec!["case:fixture".into()],
        },
    )
    .await?
    .label_id)
}
async fn review(client: Arc<Client>, kind: &str, id: &str, decision: &str) -> Result<()> {
    review_intelligence_subject(
        client,
        IntelligenceReviewInput {
            subject_type: kind.into(),
            subject_id: id.into(),
            decision: decision.into(),
            reviewer: "fixture-reviewer".into(),
            reason: format!("Fixture {decision}: independent reviewed evidence"),
            evidence_refs: vec!["case:fixture".into()],
        },
    )
    .await?;
    Ok(())
}
async fn investigate(client: Arc<Client>, n: u8) -> Result<Value> {
    Ok(serde_json::to_value(
        build_wallet_investigation(
            client,
            &address(n),
            WalletInvestigationOptions::new(None, None, None, None, None, None),
        )
        .await?,
    )?)
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore = "requires fresh disposable WAREHOUSE_TEST_URL"]
async fn live_governance_metadata_analytics_and_authenticated_api_use_only_shared_tables()
-> Result<()> {
    let rpc = Rpc::start().await?;
    let meta = fixture::salted(source::multi_transaction(), 999);
    let blocks = [
        (
            100,
            vec![
                transfer(2, 7, 1, 1),
                transfer(1, 2, 100, 2),
                transfer(7, 9, 100, 3),
                transfer(8, 9, 100, 4),
                meta.clone(),
            ],
        ),
        (
            101,
            vec![
                transfer(2, 3, 60, 5),
                transfer(2, 4, 40, 6),
                transfer(9, 4, 200, 7),
            ],
        ),
        (102, vec![transfer(4, 5, 40, 8)]),
        (103, vec![]),
    ];
    for (height, txs) in blocks {
        rpc.block(height, txs.clone());
        for tx in txs {
            rpc.receipt(height, &tx, tx["txID"] == meta["txID"]);
        }
    }
    let (config, client) = setup("operations", &rpc).await?;
    let mut bootstrap = Command::new(env!("CARGO_BIN_EXE_tron_schema"));
    let bootstrap_db = format!("{}_bootstrap", client.database());
    bootstrap
        .env("CLICKHOUSE_URL", &config.clickhouse_url)
        .env("CLICKHOUSE_USER", &config.clickhouse_user)
        .env("CLICKHOUSE_PASSWORD", &config.clickhouse_pass)
        .env("CLICKHOUSE_DB_TRON", &bootstrap_db)
        .current_dir(std::env::temp_dir());
    let output = tokio::task::spawn_blocking(move || bootstrap.output()).await??;
    assert!(
        output.status.success(),
        "configured fresh schema initialization failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(scalar(&client,&format!("SELECT toString(count()) FROM system.tables WHERE database='{bootstrap_db}' AND NOT startsWith(name,'aml_')")).await?,0);
    let loader = Arc::new(LoaderTron::new(&config).await?);
    ingest_tron_range(loader.clone(), 100, 103).await?;
    assert_eq!(scalar(&client,&format!("SELECT toString(count()) FROM system.tables WHERE database='{}' AND NOT startsWith(name,'aml_')",client.database())).await?,0);
    source_registration(&client, "fixture_labels", true).await?;
    source_registration(&client, "tron_structural_heuristics_v1", true).await?;
    let seed = label(&client, 1, "scam", 100).await?;
    let exchange = label(&client, 4, "centralized_exchange", 0).await?;
    assert!(
        load_wallet_intelligence(client.clone(), &address(1))
            .await?
            .active_entity
            .is_none()
    );
    review(client.clone(), "ENTITY_LABEL", &seed, "APPROVED").await?;
    review(client.clone(), "ENTITY_LABEL", &exchange, "APPROVED").await?;
    // Duplicate submission keeps the approved version; changed content cannot reuse an ID.
    assert_eq!(label(&client, 1, "scam", 100).await?, seed);
    assert!(
        label(&client, 1, "scam", 99).await.is_err(),
        "changed content reused a reviewed ID"
    );
    assert!(
        load_wallet_intelligence(client.clone(), &address(1))
            .await?
            .active_entity
            .is_some()
    );
    eprintln!("governed label review and idempotent import passed");
    let report = discover_address_cluster_claims(&client, 100, 103, 100).await?;
    assert!(report.deposit_claims_proposed > 0, "{report:?}");
    let intelligence = load_wallet_intelligence(client.clone(), &address(9)).await?;
    assert!(!intelligence.cluster_claims.is_empty());
    assert!(intelligence.active_clusters.is_empty());
    let cluster = intelligence.cluster_claims[0].claim_id.clone();
    review(client.clone(), "CLUSTER_CLAIM", &cluster, "APPROVED").await?;
    assert!(
        !load_wallet_intelligence(client.clone(), &address(9))
            .await?
            .active_clusters
            .is_empty()
    );
    eprintln!("reviewed cluster publication passed");
    let (_, count) = run_all_exposure_scans(client.clone(), 5).await?;
    assert!(count >= 3);
    let direct = investigate(client.clone(), 2).await?;
    let indirect = investigate(client.clone(), 3).await?;
    assert_eq!(direct["exposure"]["min_hop_distance"], 1);
    assert_eq!(
        direct["exposure"]["top_sources"][0]["exposure_type"],
        "RECEIVED_FROM_SEED"
    );
    assert_eq!(indirect["exposure"]["min_hop_distance"], 2);
    assert!(
        load_wallet_exposure_summary(client.clone(), &address(5), None)
            .await?
            .top_sources
            .is_empty(),
        "service pooled outflow leaked seed attribution"
    );
    assert!(
        load_wallet_exposure_summary(client.clone(), &address(7), None)
            .await?
            .top_sources
            .is_empty(),
        "backwards transaction was followed"
    );
    let paths = client
        .rows_from("aml_exposure_paths", "visible", "1")
        .await?;
    assert!(paths.iter().all(|p| p["fund_continuity_claimed"] == 0
        && p["tx_hashes"].as_array().unwrap().len() == p["hop_count"].as_u64().unwrap() as usize));
    assert!(
        paths.iter().any(|p| p["subject_id"] == address(3)
            && p["relationship_ids"].as_array().unwrap().len() == 2)
    );
    // A retry produces a fresh seal, never duplicate visible aggregates.
    run_exposure_scan(client.clone(), &address(1), 5).await?;
    assert_eq!(
        load_wallet_exposure_summary(client.clone(), &address(2), None)
            .await?
            .source_count,
        1
    );
    // Real worker uses only walletsolidity constant calls, tied to its actual indexed head.
    eprintln!("sealed exposure and source-bound paths passed");
    let metadata_rpc = TronClient::new(&rpc.url, None, 3)?;
    rpc.state.constant_fail.store(true, Ordering::SeqCst);
    assert!(worker_once(&client, &metadata_rpc, 10, 3).await? > 0);
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_asset_metadata_visible"
        )
        .await?,
        0
    );
    assert_eq!(
        client
            .rows_from("aml_metadata_jobs", "visible", "1")
            .await?[0]["status"],
        "retry"
    );
    assert_eq!(
        worker_once(&client, &metadata_rpc, 10, 3).await?,
        0,
        "backoff was ignored"
    );
    // Test-only expiry avoids a 30-second sleep; production state is written by worker_once.
    for mut job in client
        .rows_from("aml_metadata_jobs", "visible", "1")
        .await?
    {
        job["next_attempt_unix_ms"] = json!(0);
        job["revision"] = json!(client.revision("aml_metadata_jobs").await?);
        client.insert_rows("aml_metadata_jobs", &[job]).await?;
    }
    rpc.state.constant_fail.store(false, Ordering::SeqCst);
    assert!(worker_once(&client, &metadata_rpc, 10, 3).await? > 0);
    let metadata = client
        .rows_from("aml_asset_metadata", "visible", "1")
        .await?;
    assert!(!metadata.is_empty());
    assert!(
        metadata
            .iter()
            .all(|m| m["observed_block"] == 103 && m["decimals"] == 6 && m["is_verified"] == 0)
    );
    assert_eq!(worker_once(&client, &metadata_rpc, 10, 3).await?, 0);
    let enriched = investigate(client.clone(), 0x11).await?;
    assert!(
        enriched["holdings"]["assets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["asset_type"] == "trc20"
                && a["metadata_available"] == true
                && a["decimals"] == 6)
    );
    // Authenticated product binary, configured database, status and legacy snapshot boundary.
    eprintln!("metadata retry and observation head passed");
    let bind = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = bind.local_addr()?.port();
    drop(bind);
    let key = "fixture-service-key-at-least-32-characters";
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_tron_graph_api"))
            .env("CLICKHOUSE_URL", &config.clickhouse_url)
            .env("CLICKHOUSE_USER", &config.clickhouse_user)
            .env("CLICKHOUSE_PASSWORD", &config.clickhouse_pass)
            .env("CLICKHOUSE_DB_TRON", &config.clickhouse_db_tron)
            .env("TRON_GRAPH_API_ADDR", format!("127.0.0.1:{port}"))
            .env("AML_SERVICE_AUTH_REQUIRED", "true")
            .env("AML_SERVICE_KEY", key)
            .env("TRON_RPC_URL", &rpc.url)
            .current_dir(std::env::temp_dir())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let http = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");
    let mut ready = false;
    for _ in 0..200 {
        if http.get(format!("{base}/health")).send().await.is_ok() {
            ready = true;
            break;
        }
        if process.0.try_wait()?.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "product API failed to start");
    assert_eq!(
        http.get(format!("{base}/status")).send().await?.status(),
        401
    );
    let status: Value = http
        .get(format!("{base}/status"))
        .header("x-aml-service-key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let wallet_url = format!("{base}/api/tron/wallet/{}/investigation", address(2));
    let fetch_wallet = || async {
        http.get(wallet_url.as_str())
            .header("x-aml-service-key", key)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
    };
    let via_api = fetch_wallet().await?;
    let hash = via_api["graph"]["edges"][0]["tx_hash"].as_str().unwrap();
    let receipt_url = |hash: &str| format!("{base}/internal/kyt/transactions/{hash}");
    assert_eq!(http.get(receipt_url(hash)).send().await?.status(), 401);
    let receipt: aml_chain_api::kyt::TransactionObservation = http
        .get(receipt_url(hash))
        .header("x-aml-service-key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(receipt.tx_hash, hash);
    assert_eq!(receipt.source_snapshot, via_api["source_snapshot"]);
    let occurrence = receipt.occurrence.expect("canonical transaction");
    assert!(occurrence.participants.contains(&address(2)));
    assert!(!occurrence.participants_truncated);
    let absent: aml_chain_api::kyt::TransactionObservation = http
        .get(receipt_url(&"f".repeat(64)))
        .header("x-aml-service-key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(absent.occurrence.is_none() && !absent.ambiguous);
    assert_eq!(via_api["source_snapshot"], status["source_snapshot"]);
    assert_eq!(
        via_api["operational_snapshot"],
        status["operational_snapshot"]
    );
    assert_eq!(via_api["ai_risk"]["status"], "DISABLED");
    if let Ok(path) = std::env::var("CHAIN_RESPONSE_PROOF") {
        let wallet: Value = http
            .get(format!(
                "{base}/api/tron/wallet/{}/investigation",
                address(1)
            ))
            .header("x-aml-service-key", key)
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
            .get(format!(
                "{base}/api/tron/wallet/{}/paths/{}?max_hops=3",
                address(2),
                address(3)
            ))
            .header("x-aml-service-key", key)
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
    let first: Value = http
        .get(format!(
            "{base}/api/tron/wallet/{}/transfers?limit=1",
            address(2)
        ))
        .header("x-aml-service-key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    let second: Value = http
        .get(format!(
            "{base}/api/tron/wallet/{}/transfers?limit=1&cursor={cursor}",
            address(2)
        ))
        .header("x-aml-service-key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(first["source_snapshot"], second["source_snapshot"]);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
    let filtered:Value=http.get(format!("{base}/api/tron/wallet/{}/transfers?limit=100&from_block=100&to_block=100&asset_id=native:TRX",address(2)))
        .header("x-aml-service-key",key).send().await?.error_for_status()?.json().await?;
    assert!(!filtered["items"].as_array().unwrap().is_empty());
    assert!(
        filtered["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["block_number"] == 100 && r["asset_id"] == "native:TRX")
    );
    assert_eq!(
        http.get(format!("{base}/api/analysis/tron/wallet/{}", address(2)))
            .header("x-aml-service-key", key)
            .send()
            .await?
            .status(),
        410
    );
    // Replaying the source changes the source revision: stale derived/metadata results disappear.
    eprintln!("authenticated product API passed");
    replay_tron_range(loader, 100, 100).await?;
    let stale = http
        .get(format!(
            "{base}/api/tron/wallet/{}/transfers?limit=1&cursor={cursor}",
            address(2)
        ))
        .header("x-aml-service-key", key)
        .send()
        .await?;
    assert_eq!(stale.status(), 409);
    assert_eq!(stale.json::<Value>().await?["code"], "snapshot_changed");
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_address_exposures_visible"
        )
        .await?,
        0
    );
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_asset_metadata_visible"
        )
        .await?,
        0
    );
    run_all_exposure_scans(client.clone(), 5).await?;
    rpc.state.advance_constant_head.store(102, Ordering::SeqCst);
    worker_once(&client, &metadata_rpc, 10, 3).await?;
    assert_eq!(
        scalar(
            &client,
            "SELECT toString(count()) FROM aml_asset_metadata_visible"
        )
        .await?,
        0,
        "moving solid head was accepted as a fixed metadata observation"
    );
    assert_eq!(
        client
            .rows_from("aml_metadata_jobs", "visible", "1")
            .await?[0]["status"],
        "retry"
    );
    rpc.state.head.store(103, Ordering::SeqCst);
    for mut job in client
        .rows_from("aml_metadata_jobs", "visible", "1")
        .await?
    {
        job["next_attempt_unix_ms"] = json!(0);
        job["revision"] = json!(client.revision("aml_metadata_jobs").await?);
        client.insert_rows("aml_metadata_jobs", &[job]).await?;
    }
    worker_once(&client, &metadata_rpc, 10, 3).await?;
    {
        let _lock = records::operational_lock(&client)?;
        records::publish_clusters(&client).await?;
    }
    let before = fetch_wallet().await?;
    review(client.clone(), "CLUSTER_CLAIM", &cluster, "REVOKED").await?;
    assert!(
        load_wallet_intelligence(client.clone(), &address(9))
            .await?
            .active_clusters
            .is_empty()
    );
    review(client.clone(), "ENTITY_LABEL", &seed, "REVOKED").await?;
    assert!(
        load_wallet_intelligence(client.clone(), &address(1))
            .await?
            .active_entity
            .is_none()
    );
    assert!(
        load_wallet_exposure_summary(client.clone(), &address(2), None)
            .await?
            .top_sources
            .is_empty()
    );
    let after = fetch_wallet().await?;
    assert_eq!(before["source_snapshot"], after["source_snapshot"]);
    assert_ne!(
        before["operational_snapshot"],
        after["operational_snapshot"]
    );
    source_registration(&client, "fixture_labels", false).await?;
    assert!(label(&client, 6, "scam", 100).await.is_err());
    assert!(
        review(client.clone(), "ENTITY_LABEL", &seed, "APPROVED")
            .await
            .is_err()
    );
    assert!(
        load_wallet_intelligence(client.clone(), &address(4))
            .await?
            .active_exchange
            .is_none()
    );
    assert_eq!(scalar(&client,&format!("SELECT toString(count()) FROM system.tables WHERE database='{}' AND NOT startsWith(name,'aml_')",client.database())).await?,0);
    visibility::capture(&*client, client.database(), Network::Tron).await?;
    if let Ok(path) = std::env::var("TRON_OPERATIONS_PROOF") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &json!({"direct":before,"indirect":indirect,"revoked":after,"api":via_api}),
            )?,
        )?;
    }
    // Raw SQL bypass confirms no native relation was needed by live consumers.
    assert_eq!(Connection::query(&*client,&format!("SELECT toString(count()) FROM system.tables WHERE database='{}' AND name='wallet_analysis_snapshots'",client.database())).await?,vec!["0"]);
    Ok(())
}
