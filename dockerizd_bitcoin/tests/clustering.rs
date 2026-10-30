//! Amount-preserving synthetic evidence, not consensus-valid Bitcoin blocks.
use anyhow::Result;
use bitcoin_aml::{
    GENESIS, NETWORK_ID, activity, api, clustering,
    config::DatabaseConfig,
    db::Database,
    model,
    warehouse::{self, LegacyBlock, WriteContext},
};
use serde_json::{Value, json};
use std::collections::HashMap;
use tower::ServiceExt;
const KEY: &str = "fixture-service-key-0123456789abcdef";
fn output(index: usize, sats: u64, byte: u8, p2sh: bool) -> Value {
    let hex = if p2sh {
        format!("a914{}87", format!("{byte:02x}").repeat(20))
    } else {
        format!("76a914{}88ac", format!("{byte:02x}").repeat(20))
    };
    json!({"n":index,"value":model::decimal(sats),"scriptPubKey":{"hex":hex,"type":if p2sh{"scripthash"}else{"pubkeyhash"}}})
}
fn address(byte: u8) -> String {
    model::ScriptOutput::from_rpc(&output(0, 1, byte, false))
        .unwrap()
        .address
}
async fn db(name: &str) -> Result<Database> {
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: name.into(),
    })?;
    db.initialize().await?;
    assert_eq!(
        db.state().await?.revision,
        0,
        "fresh disposable fixture only"
    );
    Ok(db)
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
    let evidence = model::extract(&block, &format!("cluster-{height}"), resolved)?;
    warehouse::write_block(
        db,
        &state,
        &LegacyBlock {
            block_height: height,
            block_hash: hash.into(),
            parent_hash: state.tip_hash.clone(),
            block_time: 1700000000 + height,
            commit_id: format!("cluster-{height}"),
            transaction_count: evidence.transactions.len() as u32,
        },
        &evidence,
        WriteContext {
            observed: height,
            start: 0,
            source_id: "synthetic-cluster-fixture",
            legacy_fence: None,
        },
    )
    .await?;
    for o in evidence.outputs {
        resolved.insert(model::outpoint(&o.txid, o.output_index), o.output);
    }
    Ok(())
}
fn review(value: &Value, decision: &str) -> clustering::Review {
    let run = value["cluster_snapshot"]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    clustering::Review {
        network_id: NETWORK_ID.into(),
        run_id: run.clone(),
        cluster_id: value["clusters"][0]["cluster_id"].as_str().unwrap().into(),
        expected_version: run,
        decision: decision.into(),
        reviewed_by: "synthetic-reviewer".into(),
        reason: "Checked lead against synthetic transaction witnesses; no ownership inference"
            .into(),
        evidence_refs: vec!["case:synthetic-cluster".into()],
    }
}
async fn http(
    router: axum::Router,
    path: &str,
    body: Option<Value>,
    authorized: bool,
) -> Result<(u16, Value)> {
    let mut r = axum::http::Request::builder()
        .uri(path)
        .method(if body.is_some() { "POST" } else { "GET" });
    if authorized {
        r = r.header("x-aml-service-key", KEY);
    }
    r = r.header("content-type", "application/json");
    let response = router
        .oneshot(r.body(axum::body::Body::from(
            body.map(|b| b.to_string()).unwrap_or_default(),
        ))?)
        .await?;
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024).await?;
    Ok((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}
async fn current(db: &Database, value: &Value) -> Result<Value> {
    clustering::snapshot(
        db,
        value["address"].as_str().unwrap(),
        value["cluster_snapshot"]["run_id"].as_str().unwrap(),
    )
    .await
}
#[tokio::test]
#[ignore = "requires a fresh disposable ClickHouse"]
async fn canonical_leads_persist_review_and_invalidate_without_ownership_or_risk() -> Result<()> {
    let db = db("bitcoin_cluster_test").await?;
    let empty = api::investigation(&db, &address(1), 40).await?;
    assert_eq!(empty["cluster_coverage"]["status"], "waiting_for_blocks");
    assert!(empty["cluster_snapshot"].is_null());
    let mut resolved = HashMap::new();
    let first = "1".repeat(64);
    append(&db,0,GENESIS,vec![json!({"txid":first,"vin":[{"coinbase":"00"}],"vout":[output(0,600,1,false),output(1,400,2,false),output(2,1000,5,false),output(3,1000,9,false),output(4,1000,10,false)]})],&mut resolved).await?;
    append(&db,1,&"b".repeat(64),vec![
        json!({"txid":"2".repeat(64),"vin":[{"coinbase":"00"}],"vout":[output(0,1,20,false)]}),
        json!({"txid":"c".repeat(64),"vin":[{"txid":first,"vout":0},{"txid":first,"vout":1}],"vout":[output(0,999,3,false)]}),
        json!({"txid":"d".repeat(64),"vin":[{"txid":first,"vout":2}],"vout":[output(0,600,6,false),output(1,399,7,true)]}),
        json!({"txid":"e".repeat(64),"vin":[{"txid":first,"vout":3},{"txid":first,"vout":4}],"vout":[output(0,700,13,false),output(1,700,14,false),output(2,599,15,false)]})],&mut resolved).await?;
    let pending = api::investigation(&db, &address(1), 40).await?;
    assert_eq!(pending["clusters"].as_array().unwrap().len(), 1);
    assert_eq!(pending["clusters"][0]["review_status"], "pending");
    assert!(pending["risk_engine"]["risk_score"].is_null());
    let pending_current = current(&db, &pending).await?;
    assert_eq!(pending_current["cluster_snapshot"]["current"], true);
    let router = api::router(api::Api::new(db.clone(), KEY.into())?);
    let path = format!("/api/bitcoin/wallet/{}/clusters/review", address(1));
    let decision = serde_json::to_value(review(&pending, "approved"))?;
    assert_eq!(
        http(router.clone(), &path, Some(decision.clone()), false)
            .await?
            .0,
        401
    );
    let mut invalid = decision.clone();
    invalid["reviewed_by"] = json!("");
    assert_eq!(
        http(router.clone(), &path, Some(invalid), true).await?.0,
        400
    );
    let (status, receipt) = http(router.clone(), &path, Some(decision.clone()), true).await?;
    assert_eq!(status, 200, "{receipt}");
    assert_eq!(receipt["clusters"][0]["review_status"], "approved");
    assert_eq!(
        current(&db, &pending).await?["cluster_snapshot"]["current"],
        false
    );
    assert_eq!(
        http(router.clone(), &path, Some(decision), true).await?.0,
        409
    );
    let approved = api::investigation(&db, &address(1), 40).await?;
    let approved_current = current(&db, &approved).await?;
    assert_eq!(approved["clusters"][0]["review_status"], "approved");
    assert!(approved["risk_engine"]["risk_score"].is_null());
    let source_run = approved["cluster_snapshot"]["run_id"].as_str().unwrap();
    let rows:Vec<Value>=db.rows("SELECT status,entity_id FROM aml_cluster_memberships_visible WHERE run_id={run:String}",&[("run",source_run.into())]).await?;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|r| r["status"] == "pending" && r["entity_id"].is_null())
    );
    clustering::review(&db, &address(1), review(&approved, "revoked")).await?;
    let revoked = api::investigation(&db, &address(1), 40).await?;
    let revoked_current = current(&db, &revoked).await?;
    assert_eq!(revoked["clusters"][0]["review_status"], "revoked");
    assert!(revoked["risk_engine"]["risk_score"].is_null());
    assert_eq!(
        current(&db, &approved).await?["cluster_snapshot"]["current"],
        false
    );
    let rejected = clustering::review(&db, &address(1), review(&revoked, "rejected")).await?;
    assert_eq!(rejected["clusters"][0]["review_status"], "rejected");
    let change = api::investigation(&db, &address(5), 40).await?;
    let change_current = current(&db, &change).await?;
    assert_eq!(change["clusters"][0]["heuristic"], "change_candidate");
    assert_eq!(
        change["clusters"][0]["evidence"]["match"]["candidate_output_index"],
        0
    );
    let collaborative = api::investigation(&db, &address(9), 40).await?;
    assert!(collaborative["clusters"].as_array().unwrap().is_empty());
    assert!(
        collaborative["cluster_coverage"]["suppressed_transactions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["reason"] == "possible_collaborative_transaction")
    );
    db.execute("OPTIMIZE TABLE aml_cluster_versions FINAL")
        .await?;
    db.execute("OPTIMIZE TABLE aml_cluster_claims FINAL")
        .await?;
    let history:Vec<Value>=db.rows("SELECT raw_evidence_json FROM aml_cluster_versions_current WHERE change_type='review' ORDER BY revision",&[]).await?;
    assert_eq!(history.len(), 3);
    let decisions=history.iter().map(|r|serde_json::from_str::<Value>(r["raw_evidence_json"].as_str().unwrap()).unwrap()["review_status"].clone()).collect::<Vec<_>>();
    assert_eq!(
        decisions,
        vec![json!("approved"), json!("revoked"), json!("rejected")]
    );
    let mut bounded = activity::read(&db, &address(1), &Default::default()).await?;
    let root = bounded["recent_transactions"][0].clone();
    bounded["recent_transactions"] = json!(vec![root; 26]);
    let partial = clustering::compute(&db, &address(1), &bounded).await?;
    assert_eq!(partial["cluster_coverage"]["status"], "partial");
    assert_eq!(partial["cluster_coverage"]["persisted"], false);
    let partial_current = current(&db, &partial).await?;
    assert_eq!(partial_current["cluster_snapshot"]["current"], false);
    assert_eq!(partial_current["cluster_snapshot"]["source_current"], true);
    assert!(
        clustering::review(&db, &address(1), review(&partial, "approved"))
            .await
            .is_err()
    );
    // A missing publication seal keeps all output rows audit-only.
    let change_id = change["cluster_snapshot"]["run_id"].as_str().unwrap();
    db.execute(&format!("INSERT INTO aml_analysis_publications SELECT * REPLACE('invalidated' AS status,revision+1 AS revision) FROM aml_analysis_publications_current WHERE family='cluster' AND publication_id='{change_id}'")).await?;
    assert_eq!(
        current(&db, &change).await?["cluster_snapshot"]["current"],
        false
    );
    // Corrupting one child row must not return a seemingly complete bundle.
    db.execute(&format!("INSERT INTO aml_cluster_memberships SELECT * REPLACE('active' AS status,revision+1 AS revision) FROM aml_cluster_memberships_current WHERE run_id='{change_id}' LIMIT 1")).await?;
    assert!(
        clustering::inspect(&db, &address(5), change_id)
            .await
            .is_err()
    );
    let states: Vec<Value> = db
        .rows(
            "SELECT count() AS n FROM aml_intelligence_claims_current",
            &[],
        )
        .await?;
    assert_eq!(states[0]["n"], 0);
    // Remove the source branch; old leads and all of their reviews are historical.
    let previous = db.state().await?;
    let mut rolled = previous.clone();
    rolled.revision = db.next_revision(previous.revision).await?;
    rolled.tip_height = 0;
    rolled.tip_hash = GENESIS.into();
    db.publish_after(&previous, &rolled).await?;
    assert_eq!(
        current(&db, &revoked).await?["cluster_snapshot"]["current"],
        false
    );
    let after = api::investigation(&db, &address(1), 40).await?;
    assert!(after["clusters"].as_array().unwrap().is_empty());
    if let Ok(path) = std::env::var("BITCOIN_CLUSTER_ACCEPTANCE") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &json!({"pending":pending,"pending_current":pending_current,"approved":approved,"approved_current":approved_current,"revoked":revoked,"revoked_current":revoked_current,"change":change,"change_current":change_current,"collaborative":collaborative,"partial":partial,"partial_current":partial_current,"after_reorg":after}),
            )?,
        )?;
    }
    Ok(())
}
