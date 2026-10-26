//! Run only against a disposable, empty ClickHouse, never a deployed database:
//! BITCOIN_TEST_CLICKHOUSE_URL=http://... cargo test --test clickhouse_pipeline -- --ignored
use aml_warehouse::{
    Connection,
    publication::{self, Block},
};
use anyhow::Result;
use axum::{Json, Router, extract::State, routing::post};
use bitcoin_aml::{
    api,
    config::{DatabaseConfig, IngestConfig},
    db::{Database, SyncState, next_revision},
    ingestion, model,
    rpc::Rpc,
    warehouse,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tower::ServiceExt;

struct TestConnection<'a>(&'a Database);
impl Connection for TestConnection<'_> {
    async fn execute(&self, sql: &str) -> Result<()> {
        self.0.execute(sql).await
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.0
            .rows::<Value>(sql, &[])
            .await?
            .into_iter()
            .map(|row| {
                Ok(row
                    .as_object()
                    .unwrap()
                    .values()
                    .next()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned())
            })
            .collect()
    }
}
async fn readiness(db: &Database) -> Result<axum::http::StatusCode> {
    Ok(api::router(api::Api::new(
        db.clone(),
        "fixture-service-key-0123456789abcdef".into(),
    )?)
    .oneshot(
        axum::http::Request::builder()
            .uri("/ready")
            .body(axum::body::Body::empty())?,
    )
    .await?
    .status())
}

fn output(n: u64, amount: &str, hash: &str) -> Value {
    json!({"n":n,"value":amount,"scriptPubKey":{"hex":format!("76a914{hash}88ac"),"type":"pubkeyhash"}})
}
fn fixture(height: u64, hash: &str, parent: &str, recipient: &str) -> Value {
    let mut transactions = vec![
        json!({"txid":format!("{:064x}", height+1),"vin":[{"coinbase":"00"}],
        "vout":[output(0,"50.00000000", "1111111111111111111111111111111111111111")]}),
    ];
    if height == 1 {
        transactions.push(
            json!({"txid":"9".repeat(64),"vin":[{"txid":format!("{:064x}",1),"vout":0}],
            "vout":[output(0,"49.99999999",recipient)]}),
        );
    }
    json!({"height":height,"hash":hash,"previousblockhash":parent,"time":123+height,"tx":transactions})
}
type Mock = Arc<RwLock<Vec<Value>>>;
async fn rpc(State(blocks): State<Mock>, Json(request): Json<Value>) -> Json<Value> {
    let blocks = blocks.read().unwrap();
    if let Some(requests) = request.as_array() {
        return Json(Value::Array(
            requests
                .iter()
                .rev()
                .map(|r| rpc_reply(&blocks, r))
                .collect(),
        ));
    }
    Json(rpc_reply(&blocks, &request))
}
fn rpc_reply(blocks: &[Value], request: &Value) -> Value {
    let result = match request["method"].as_str().unwrap() {
        "getblockchaininfo" => json!({"chain":"main","pruned":false}),
        "getblockcount" => json!(blocks.len() - 1),
        "getblockhash" => blocks[request["params"][0].as_u64().unwrap() as usize]["hash"].clone(),
        "getblock" => blocks
            .iter()
            .find(|b| b["hash"] == request["params"][0])
            .unwrap()
            .clone(),
        _ => return json!({"id":request["id"],"result":null,"error":{"code":-5}}),
    };
    json!({"id":request["id"],"result":result,"error":null})
}
#[tokio::test]
#[ignore = "requires a disposable ClickHouse 23.8 instance"]
async fn pipeline_visibility_restart_utxos_paths_and_reorg() -> Result<()> {
    let url = std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?;
    let db = Database::new(DatabaseConfig {
        url: url.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_aml".into(),
    })?;
    db.initialize().await?;
    db.initialize().await?; // Schema is idempotent.
    let objects: Vec<Value> = db.rows("SELECT count() AS n FROM system.tables WHERE database=currentDatabase() AND startsWith(name,'bitcoin_')",&[]).await?;
    assert_eq!(
        objects[0]["n"], 0,
        "a new database must contain only the shared schema"
    );
    assert_eq!(
        db.state().await?.revision,
        0,
        "test requires an empty database"
    );
    let empty = api::investigation(&db, "1BoatSLRHtKNngkdXEeobR76b53LETtpyT", 40).await?;
    assert_eq!(empty["holdings"]["history_complete"], false);
    assert_eq!(empty["graph"]["edges"].as_array().unwrap().len(), 0);
    let a = "1111111111111111111111111111111111111111";
    let b = "2222222222222222222222222222222222222222";
    let c = "3333333333333333333333333333333333333333";
    let genesis = fixture(0, bitcoin_aml::GENESIS, "", a);
    let one = fixture(1, &"a".repeat(64), bitcoin_aml::GENESIS, b);
    // Seed the previous release's full schema and history, including an
    // unpublished commit that must never enter the migrated canonical history.
    let sql = include_str!("../sql/legacy/001_core.sql")
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    for statement in sql.split(';').filter(|s| !s.trim().is_empty()) {
        db.execute(statement).await?;
    }
    let mut resolved = HashMap::new();
    let genesis_output = model::ScriptOutput::from_rpc(&genesis["tx"][0]["vout"][0])?;
    resolved.insert(model::outpoint(&format!("{:064x}", 1), 0), genesis_output);
    for (index, block) in [genesis.clone(), one.clone()].iter().enumerate() {
        let commit = format!("legacy-{index}");
        let facts = model::extract(block, &commit, &resolved)?;
        db.insert("bitcoin_transactions", &facts.transactions)
            .await?;
        db.insert("bitcoin_outputs", &facts.outputs).await?;
        db.insert("bitcoin_inputs", &facts.inputs).await?;
        db.insert("bitcoin_block_commits",&[json!({"block_height":index,"block_hash":block["hash"],"parent_hash":block["previousblockhash"],"block_time":block["time"],"commit_id":commit,"revision":index+10,"transaction_count":facts.transactions.len()})]).await?;
    }
    let orphan = fixture(2, &"f".repeat(64), &"a".repeat(64), c);
    let orphan_facts = model::extract(&orphan, "legacy-unpublished", &HashMap::new())?;
    db.insert("bitcoin_transactions", &orphan_facts.transactions)
        .await?;
    db.insert("bitcoin_outputs", &orphan_facts.outputs).await?;
    db.insert("bitcoin_block_commits",&[json!({"block_height":2,"block_hash":orphan["hash"],"parent_hash":orphan["previousblockhash"],"block_time":orphan["time"],"commit_id":"legacy-unpublished","revision":100,"transaction_count":1})]).await?;
    db.insert(
        "bitcoin_sync_state",
        &[SyncState {
            id: 0,
            revision: 11,
            initialized: 1,
            start_height: 0,
            tip_height: 1,
            tip_hash: "a".repeat(64),
            observed_rpc_height: 1,
            published_at: 1,
        }],
    )
    .await?;
    assert!(warehouse::migration_ready(&db).await.is_err());
    let legacy_before: Vec<Value> = db.rows("SELECT count() AS n,toString(sum(value_sats)) AS amount FROM bitcoin_outputs_canonical",&[]).await?;
    let partial = warehouse::backfill(&db, 1).await?;
    assert_eq!(partial["complete"], false);
    assert_eq!(
        readiness(&db).await?,
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(db.state().await?.tip_height, 0);
    assert!(warehouse::migration_ready(&db).await.is_err());
    // Simulate an interrupted migration: the next invocation resumes its prefix.
    let migrated = warehouse::backfill(&db, 1).await?;
    assert_eq!(migrated["complete"], true);
    assert_eq!(migrated["migrated_blocks"], 1);
    warehouse::migration_ready(&db).await?;
    assert_eq!(readiness(&db).await?, axum::http::StatusCode::OK);
    assert_eq!(warehouse::backfill(&db, 10).await?["migrated_blocks"], 0);
    let shared_after: Vec<Value> = db.rows("SELECT count() AS n,toString(sum(value_sats)) AS amount FROM aml_bitcoin_outputs_canonical WHERE network_id={network:String}",&[("network",bitcoin_aml::NETWORK_ID.into())]).await?;
    assert_eq!(legacy_before, shared_after);
    assert!(db.block_hash(2).await?.is_none());
    let legs: Vec<Value> = db.rows("SELECT count() AS n,countIf(from_subject_type='address' AND to_subject_type='address') AS fabricated,countIf(amount_raw=4999999999) AS exact FROM aml_address_relationships_canonical",&[]).await?;
    assert_eq!(legs[0]["n"], 4);
    assert_eq!(legs[0]["fabricated"], 0);
    assert_eq!(legs[0]["exact"], 1);
    let blocks: Mock = Arc::new(RwLock::new(vec![genesis.clone(), one.clone()]));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let rpc_url = format!("http://{}", listener.local_addr()?);
    let mock = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/", post(rpc))
                .with_state(blocks.clone()),
        )
        .into_future(),
    );
    let rpc = Rpc::new(&rpc_url, "test".into(), "test".into(), 5)?;
    let config = IngestConfig {
        start: 0,
        end: None,
        confirmations: 1,
        reorg_depth: 10,
        poll_seconds: 1,
        verbosity: 3,
    };
    ingestion::run(db.clone(), rpc.clone(), config.clone(), true).await?;
    let initial = db.state().await?;
    assert_eq!(initial.tip_height, 1);
    // Resuming at the head must not duplicate facts or change the checkpoint.
    ingestion::run(db.clone(), rpc.clone(), config.clone(), true).await?;
    assert_eq!(db.state().await?.revision, initial.revision);
    let recipient = model::ScriptOutput::from_rpc(&one["tx"][1]["vout"][0])?.address;
    let source = model::ScriptOutput::from_rpc(&genesis["tx"][0]["vout"][0])?.address;
    let holding = api::wallet_holdings(&db, &recipient).await?;
    assert_eq!(holding["balance_sats"], "4999999999");
    assert_eq!(holding["history_complete"], true);
    let source_holdings = api::wallet_holdings(&db, &source).await?;
    assert_eq!(source_holdings["balance_sats"], "5000000000"); // first reward was spent; second remains.
    assert_eq!(source_holdings["immature_sats"], "5000000000");
    let investigation = api::investigation(&db, &recipient, 40).await?;
    assert_eq!(investigation["graph"]["edges"].as_array().unwrap().len(), 2);
    let path = api::find_paths(&db, &source, &recipient, 3, 200).await?;
    assert_eq!(path["paths"][0]["fund_continuity_claimed"], false);
    assert_eq!(path["paths"][0]["hop_count"], 1);
    // A crash after evidence and commit insert, before publication, exposes no new block.
    let two = fixture(2, &"b".repeat(64), &"a".repeat(64), c);
    // A future revision also simulates wall-clock rollback between crash/restart.
    let unpublished = next_revision(initial.revision) + 100_000_000;
    let facts = model::extract(&two, "unpublished", &HashMap::new())?;
    let mut pending = Block {
        network_id: bitcoin_aml::NETWORK_ID.into(),
        block_number: 2,
        block_hash: "b".repeat(64),
        block_timestamp_unix_ms: 125000,
        commit_id: "partial".into(),
        revision: unpublished,
        parent_hash: "a".repeat(64),
        canonical: 1,
        ingestion_status: "complete".into(),
        transaction_count: 1,
        receipt_coverage: None,
        trace_coverage: None,
        prevout_coverage: Some(1),
        source_id: "crash_fixture".into(),
        created_at_unix_ms: 1,
    };
    let connection = TestConnection(&db);
    publication::reserve(&connection, "bitcoin_aml", &pending).await?;
    let tables = warehouse::map_evidence(&pending, &facts)?;
    publication::facts(
        &connection,
        "bitcoin_aml",
        &pending,
        "aml_transactions",
        &tables["aml_transactions"],
    )
    .await?;
    // Crash before the complete marker: reserving again must skip its fact revision.
    let next = db.next_revision(initial.revision).await?;
    assert!(next > pending.revision);
    pending.revision = next;
    pending.commit_id = "unpublished-complete".into();
    publication::reserve(&connection, "bitcoin_aml", &pending).await?;
    let tables = warehouse::map_evidence(&pending, &facts)?;
    for (table, rows) in &tables {
        publication::facts(&connection, "bitcoin_aml", &pending, table, rows).await?;
    }
    publication::complete(&connection, "bitcoin_aml", &pending).await?;
    assert!(db.block_hash(2).await?.is_none());
    assert_eq!(
        api::wallet_holdings(&db, &source).await?["balance_sats"],
        "5000000000"
    );
    // A normal restart writes a fresh complete commit and publishes it once.
    let state2 = ingestion::commit(&db, &initial, &two, &HashMap::new(), 2, 0).await?;
    assert!(state2.revision > unpublished);
    assert!(state2.revision > pending.revision);
    assert_eq!(
        api::wallet_holdings(&db, &source).await?["balance_sats"],
        "10000000000"
    );
    // Simulate a reorg replacing heights 1 and 2. Rollback hides every orphaned fact.
    let replacement = fixture(1, &"c".repeat(64), bitcoin_aml::GENESIS, c);
    *blocks.write().unwrap() = vec![genesis, replacement.clone()];
    let rollback = ingestion::reconcile(&db, &rpc, &state2, &config, 1).await?;
    assert_eq!(rollback.tip_height, 0);
    warehouse::migration_ready(&db).await?; // completion survives subsequent reorgs.
    assert_eq!(
        api::wallet_holdings(&db, &recipient).await?["balance_sats"],
        "0"
    );
    assert!(db.block_hash(1).await?.is_none());
    ingestion::run(db.clone(), rpc.clone(), config.clone(), true).await?;
    assert_eq!(db.state().await?.tip_hash, "c".repeat(64));
    assert_eq!(
        api::wallet_holdings(&db, &recipient).await?["balance_sats"],
        "0"
    );
    let new_recipient = model::ScriptOutput::from_rpc(&replacement["tx"][1]["vout"][0])?.address;
    assert_eq!(
        api::wallet_holdings(&db, &new_recipient).await?["balance_sats"],
        "4999999999"
    );
    // An unresolved input fails before publication and cannot advance the checkpoint.
    let mut bad = fixture(2, &"d".repeat(64), &"c".repeat(64), a);
    bad["tx"].as_array_mut().unwrap().push(json!({"txid":"e".repeat(64),"vin":[{"txid":"f".repeat(64),"vout":0}],"vout":[output(0,"1",a)]}));
    let before = db.state().await?;
    assert!(
        ingestion::commit(&db, &before, &bad, &HashMap::new(), 2, 0)
            .await
            .is_err()
    );
    assert_eq!(db.state().await?.revision, before.revision);
    // Production writers no longer append facts to legacy tables.
    let legacy_after: Vec<Value> = db.rows("SELECT count() AS n,toString(sum(value_sats)) AS amount FROM bitcoin_outputs_canonical",&[]).await?;
    assert_eq!(legacy_before, legacy_after);
    // Publication is confined to the configured network and matching headers.
    let mut foreign = tables["aml_bitcoin_outputs"].clone();
    foreign[0]["network_id"] = json!("eip155:1");
    assert!(
        publication::facts(
            &connection,
            "bitcoin_aml",
            &pending,
            "aml_bitcoin_outputs",
            &foreign
        )
        .await
        .is_err()
    );
    // A bounded reorg must fail closed when no ancestor is found in its window.
    let fake = SyncState {
        tip_height: 100,
        tip_hash: "f".repeat(64),
        ..before
    };
    assert!(
        ingestion::reconcile(&db, &rpc, &fake, &config, 1)
            .await
            .is_err()
    );
    mock.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "requires a disposable ClickHouse 23.8 instance"]
async fn incomplete_legacy_block_stops_migration_and_can_be_repaired() -> Result<()> {
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_migration_incomplete".into(),
    })?;
    db.initialize().await?;
    assert_eq!(
        db.state().await?.revision,
        0,
        "fixture database must be empty"
    );
    let sql = include_str!("../sql/legacy/001_core.sql")
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    for statement in sql.split(';').filter(|s| !s.trim().is_empty()) {
        db.execute(statement).await?;
    }
    let block = fixture(
        0,
        bitcoin_aml::GENESIS,
        "",
        "1111111111111111111111111111111111111111",
    );
    db.insert("bitcoin_block_commits",&[json!({"block_height":0,"block_hash":block["hash"],"parent_hash":"","block_time":block["time"],"commit_id":"repairable","revision":1,"transaction_count":1})]).await?;
    db.insert(
        "bitcoin_sync_state",
        &[SyncState {
            id: 0,
            revision: 1,
            initialized: 1,
            start_height: 0,
            tip_height: 0,
            tip_hash: bitcoin_aml::GENESIS.into(),
            observed_rpc_height: 0,
            published_at: 1,
        }],
    )
    .await?;
    assert!(warehouse::backfill(&db, 0).await.is_err());
    assert!(warehouse::backfill(&db, 100001).await.is_err());
    assert!(warehouse::backfill(&db, 1).await.is_err());
    assert_eq!(db.state().await?.revision, 0);
    assert!(db.block_hash(0).await?.is_none());
    assert!(warehouse::migration_ready(&db).await.is_err());
    let evidence = model::extract(&block, "repairable", &HashMap::new())?;
    db.insert("bitcoin_transactions", &evidence.transactions)
        .await?;
    db.insert("bitcoin_outputs", &evidence.outputs).await?;
    assert_eq!(warehouse::backfill(&db, 1).await?["complete"], true);
    warehouse::migration_ready(&db).await?;
    assert_eq!(
        db.block_hash(0).await?.as_deref(),
        Some(bitcoin_aml::GENESIS)
    );
    // A stale publisher cannot replace the checkpoint established by migration.
    let mut stale = db.state().await?;
    stale.revision += 10;
    assert!(
        db.publish_after(&SyncState::default(), &stale)
            .await
            .is_err()
    );
    // Even if the hash is unchanged, an unexpected old writer creates a new
    // source snapshot. An old completion receipt must not bless its new facts.
    let changed = SyncState {
        revision: 2,
        ..db.legacy_state().await?
    };
    db.insert("bitcoin_sync_state", &[changed]).await?;
    let before = db.state().await?;
    assert!(warehouse::migration_ready(&db).await.is_err());
    assert!(warehouse::backfill(&db, 1).await.is_err());
    assert_eq!(db.state().await?, before);
    // A missing legacy view must never be mistaken for an empty new install.
    db.execute("DROP VIEW bitcoin_sync_current").await?;
    assert!(db.legacy_state().await.is_err());
    assert!(warehouse::backfill(&db, 1).await.is_err());
    assert_eq!(db.state().await?, before);
    Ok(())
}
