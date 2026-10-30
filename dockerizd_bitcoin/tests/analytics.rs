//! Real ClickHouse, synthetic non-consensus Bitcoin evidence, no external RPC.
use anyhow::Result;
use bitcoin_aml::{
    GENESIS, NETWORK_ID,
    analytics::{Config, Worker},
    api,
    config::DatabaseConfig,
    db::Database,
    intelligence, model, risk,
    warehouse::{self, LegacyBlock, WriteContext},
};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, time::Duration};
use tower::ServiceExt;
const KEY: &str = "fixture-service-key-0123456789abcdef";
fn output(index: usize, sats: u64, byte: u8) -> Value {
    json!({"n":index,"value":model::decimal(sats),"scriptPubKey":{"hex":format!("76a914{}88ac",format!("{byte:02x}").repeat(20)),"type":"pubkeyhash"}})
}
fn address(byte: u8) -> String {
    model::ScriptOutput::from_rpc(&output(0, 1, byte))
        .unwrap()
        .address
}
async fn append(
    db: &Database,
    height: u64,
    hash: &str,
    transactions: Vec<Value>,
    resolved: &mut HashMap<String, model::ScriptOutput>,
) -> Result<()> {
    let state = db.state().await?;
    let block = json!({"height":height,"hash":hash,"previousblockhash":state.tip_hash,"time":1700000000+height,"tx":transactions});
    let evidence = model::extract(&block, &format!("worker-{height}"), resolved)?;
    warehouse::write_block(
        db,
        &state,
        &LegacyBlock {
            block_height: height,
            block_hash: hash.into(),
            parent_hash: state.tip_hash.clone(),
            block_time: 1700000000 + height,
            commit_id: format!("worker-{height}"),
            transaction_count: evidence.transactions.len() as u32,
        },
        &evidence,
        WriteContext {
            observed: height,
            start: 0,
            source_id: "synthetic-worker-fixture",
            legacy_fence: None,
        },
    )
    .await?;
    for o in evidence.outputs {
        resolved.insert(model::outpoint(&o.txid, o.output_index), o.output);
    }
    Ok(())
}
async fn drain(worker: &mut Worker, db: &Database) -> Result<Value> {
    for _ in 0..30 {
        let ran = worker.cycle(db).await?;
        let report = worker.report();
        if !ran
            && report["scan"]["exhausted"] == true
            && report["next_sweep_unix_ms"].as_u64().unwrap_or(0) > 0
        {
            assert_eq!(report["failed_jobs"], 0, "{report}");
            return Ok(report);
        }
    }
    anyhow::bail!(
        "worker did not finish bounded fixture sweep: {}",
        worker.report()
    )
}
fn receipt(report: &Value, addr: &str) -> Value {
    report["recent"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r["address"] == addr)
        .unwrap()
        .clone()
}
async fn decision(db: &Database, decision: &str) -> Result<Value> {
    let current = intelligence::read(db, &address(6)).await?;
    intelligence::review(db,serde_json::from_value(json!({"network_id":NETWORK_ID,"claim_id":"worker-seed","expected_revision":current.claims[0]["revision"],"decision":decision,"reviewed_by":"fixture-reviewer","reason":"Explicit synthetic acceptance decision, not made by worker","evidence_refs":["case:worker-fixture"]}))?,false).await
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse and a persistent test state directory"]
async fn canonical_sweeps_resume_refresh_revoked_seeds_and_follow_reorg_without_approval()
-> Result<()> {
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_worker_test".into(),
    })?;
    db.initialize().await?;
    assert_eq!(
        db.state().await?.revision,
        0,
        "fresh disposable fixture required"
    );
    let directory = PathBuf::from(std::env::var("BITCOIN_ANALYTICS_STATE_DIR")?);
    let cfg = Config {
        directory,
        page_size: 2,
        poll_seconds: 1,
        refresh_seconds: 10,
        job_timeout_seconds: 120,
        retry_seconds: 1,
        max_attempts: 2,
    };
    let mut worker = Worker::open("bitcoin_worker_test", cfg.clone())?;
    assert!(!worker.cycle(&db).await?);
    assert_eq!(worker.report()["reason"], "waiting_for_blocks");
    let mut resolved = HashMap::new();
    let first = "1".repeat(64);
    append(&db,0,GENESIS,vec![json!({"txid":first,"vin":[{"coinbase":"00"}],"vout":(1..=5).map(|b|output((b-1) as usize,200,b)).collect::<Vec<_>>()})],&mut resolved).await?;
    append(&db,1,&"b".repeat(64),vec![json!({"txid":"2".repeat(64),"vin":[{"coinbase":"00"}],"vout":[output(0,1,6)]}),json!({"txid":"c".repeat(64),"vin":(0..5).map(|v|json!({"txid":first,"vout":v})).collect::<Vec<_>>(),"vout":[output(0,999,6)]})],&mut resolved).await?;
    intelligence::register(&db,serde_json::from_value(json!({"network_id":NETWORK_ID,"source_id":"worker-source","source_name":"Synthetic worker source","source_type":"test_fixture","reference_url":"https://example.invalid/worker","trust_tier":"unrated","license":"test only","is_active":true,"created_by":"fixture-operator"}))?,false).await?;
    intelligence::import(&db,vec![serde_json::from_value(json!({"network_id":NETWORK_ID,"claim_id":"worker-seed","address":address(6),"entity_id":"synthetic-risky-service","entity_name":"Synthetic fixture only","entity_type":"test_fixture","address_role":"deposit","confidence":0.95,"risk_level":100,"is_exposure_seed":true,"seed_category":"fixture_only","source_id":"worker-source","source_reference":"case:worker-fixture","evidence_refs":["case:worker-fixture"],"submitted_by":"fixture-operator","valid_from_unix_ms":0}))?],false).await?;
    assert!(worker.cycle(&db).await?);
    let before_restart = worker.report();
    assert_eq!(before_restart["succeeded"], 1);
    assert!(Worker::open("bitcoin_worker_test", cfg.clone()).is_err());
    let _ingestion_owner =
        bitcoin_aml::writer_lock::WriterLock::for_database("bitcoin_worker_test")?;
    drop(_ingestion_owner);
    drop(worker);
    let mut worker = Worker::open("bitcoin_worker_test", cfg.clone())?;
    assert_eq!(worker.report()["scan"], before_restart["scan"]);
    let pending = drain(&mut worker, &db).await?;
    eprintln!("PASS first sweep and restart: six completed jobs, cursor retained");
    assert_eq!(pending["succeeded"], 6);
    assert_eq!(
        intelligence::read(&db, &address(6)).await?.claims[0]["review_status"],
        "pending"
    );
    assert_eq!(
        receipt(&pending, &address(1))["result"]["risk_level"],
        "UNKNOWN"
    );
    let old_receipt = receipt(&pending, &address(1));
    let assessment = risk::inspect(
        &db,
        &address(1),
        old_receipt["result"]["assessment_id"].as_str().unwrap(),
    )
    .await?;
    assert_eq!(assessment["current"], true);
    let evidence: Value =
        serde_json::from_str(assessment["assessment"]["evidence_json"].as_str().unwrap())?;
    assert!(evidence["inputs"]["fingerprint"].is_object());
    assert!(
        evidence["inputs"]["semantic_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event_type"] == "bitcoin_consolidation_candidate")
    );
    let memberships:Vec<Value>=db.rows("SELECT countIf(status='active' OR isNotNull(entity_id)) AS invalid FROM aml_cluster_memberships_current",&[]).await?;
    assert_eq!(memberships[0]["invalid"], 0);
    let approved_receipt = decision(&db, "approved").await?;
    tokio::time::sleep(Duration::from_secs(11)).await;
    let approved = drain(&mut worker, &db).await?;
    eprintln!("PASS second sweep after explicit fixture seed review");
    assert_eq!(approved["succeeded"], 12);
    assert_ne!(
        receipt(&approved, &address(1))["result"]["risk_level"],
        "UNKNOWN"
    );
    let prior = risk::inspect(
        &db,
        &address(1),
        receipt(&approved, &address(1))["result"]["assessment_id"]
            .as_str()
            .unwrap(),
    )
    .await?;
    assert_eq!(prior["current"], true);
    decision(&db, "revoked").await?;
    assert_eq!(
        risk::inspect(&db, &address(1), prior["assessment_id"].as_str().unwrap()).await?["current"],
        false
    );
    tokio::time::sleep(Duration::from_secs(11)).await;
    let revoked = drain(&mut worker, &db).await?;
    eprintln!("PASS third sweep after explicit fixture seed revocation");
    assert_eq!(revoked["succeeded"], 18);
    assert_eq!(
        receipt(&revoked, &address(1))["result"]["risk_level"],
        "UNKNOWN"
    );
    assert_eq!(
        intelligence::read(&db, &address(6)).await?.claims[0]["review_status"],
        "revoked"
    );
    let previous = db.state().await?;
    let mut rolled = previous.clone();
    rolled.revision = db.next_revision(previous.revision).await?;
    rolled.tip_height = 0;
    rolled.tip_hash = GENESIS.into();
    db.publish_after(&previous, &rolled).await?;
    assert!(worker.cycle(&db).await?);
    assert_eq!(worker.report()["scan"]["height"], 0);
    let after_reorg = drain(&mut worker, &db).await?;
    assert_eq!(after_reorg["succeeded"], 23);
    assert_eq!(
        risk::inspect(
            &db,
            &address(1),
            old_receipt["result"]["assessment_id"].as_str().unwrap()
        )
        .await?["current"],
        false
    );
    let route = api::router(api::Api::new(db.clone(), KEY.into())?);
    let request = |authorized: bool| {
        let mut r = axum::http::Request::builder().uri("/api/bitcoin/analytics/status");
        if authorized {
            r = r.header("x-aml-service-key", KEY);
        }
        r.body(axum::body::Body::empty()).unwrap()
    };
    assert_eq!(route.clone().oneshot(request(false)).await?.status(), 401);
    let response = route.oneshot(request(true)).await?;
    assert_eq!(response.status(), 200);
    let status: Value =
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(status["all_addresses_current"], false);
    assert_eq!(status["heartbeat_fresh"], true);
    let investigation = api::investigation(&db, &address(1), 25).await?;
    assert!(investigation["data_coverage"]["analytics_worker"].is_object());
    worker.pause()?;
    assert!(!bitcoin_aml::analytics::cli::healthy(&worker.report()));
    if let Ok(path) = std::env::var("BITCOIN_WORKER_ACCEPTANCE") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &json!({"before_restart":before_restart,"pending":pending,"approved":approved,"approved_receipt":approved_receipt,"revoked":revoked,"after_reorg":after_reorg,"status":status,"investigation":investigation}),
            )?,
        )?;
    }
    Ok(())
}
