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
async fn transfer_page(db: &Database, address: &str, query: &str) -> Result<Value> {
    let key = "fixture-service-key-0123456789abcdef";
    let response = api::router(api::Api::new(db.clone(), key.into())?)
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/bitcoin/wallet/{address}/transfers?{query}"))
                .header("x-aml-service-key", key)
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    Ok(serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1_048_576).await?,
    )?)
}

async fn api_value(db: &Database, path: &str) -> Result<Value> {
    let key = "fixture-service-key-0123456789abcdef";
    let response = api::router(api::Api::new(db.clone(), key.into())?)
        .oneshot(
            axum::http::Request::builder()
                .uri(path)
                .header("x-aml-service-key", key)
                .body(axum::body::Body::empty())?,
        )
        .await?;
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 16_777_216).await?;
    anyhow::ensure!(
        status.is_success(),
        "API {path}: {status}: {}",
        String::from_utf8_lossy(&bytes)
    );
    Ok(serde_json::from_slice(&bytes)?)
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
#[ignore = "requires a disposable ClickHouse with the Bitcoin fixture credentials"]
async fn tbtc_funding_reads_only_published_bitcoin_outputs() -> Result<()> {
    let evidence: Value = serde_json::from_str(include_str!(
        "../../shared/chain_api/tests/fixtures/tbtc-mainnet.json"
    ))?;
    let btc = &evidence["btc"];
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_tbtc_fixture".into(),
    })?;
    db.initialize().await?;
    assert_eq!(db.state().await?.revision, 0, "requires an empty database");
    let recipient = "1111111111111111111111111111111111111111";
    let genesis = fixture(0, bitcoin_aml::GENESIS, "", recipient);
    let mut one = fixture(1, &"a".repeat(64), bitcoin_aml::GENESIS, recipient);
    let tx = &mut one["tx"][1];
    tx["txid"] = btc["tx_hash"].clone();
    tx["vout"].as_array_mut().unwrap().push(json!({
        "n": 1, "value": "0.01000000", "scriptPubKey": {
            "hex": btc["script_hex"], "type": "witness_v0_scripthash"
        }
    }));
    tx["vout"][0]["value"] = json!("49.98000000");
    let blocks = Arc::new(RwLock::new(vec![genesis.clone(), one]));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let mock = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/", post(rpc))
                .with_state(blocks.clone()),
        )
        .into_future(),
    );
    let funding = format!(
        "/internal/bridge/tbtc/funding/{}?block_number=1",
        btc["tx_hash"].as_str().unwrap()
    );
    assert_eq!(api_value(&db, &funding).await?["outputs"], json!([]));
    let rpc = Rpc::new(&url, "test".into(), "test".into(), 5)?;
    let config = IngestConfig {
        start: 0,
        end: None,
        confirmations: 1,
        reorg_depth: 10,
        poll_seconds: 1,
        verbosity: 3,
    };
    ingestion::run(db.clone(), rpc.clone(), config.clone(), true).await?;
    let found = api_value(&db, &funding).await?;
    assert_eq!(found["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(found["outputs"][0]["output_index"], 1);
    assert_eq!(found["outputs"][0]["value_sats"], 1_000_000);
    assert_eq!(found["outputs"][0]["script_hex"], btc["script_hex"]);
    assert_eq!(
        api_value(&db, &format!("{funding}&output_index=1")).await?,
        found
    );
    assert_eq!(
        api_value(&db, &format!("{funding}&output_index=0")).await?["outputs"],
        json!([])
    );
    assert_eq!(
        api_value(&db, "/internal/bridge/snapshot").await?,
        found["source_snapshot"]
    );
    let root = model::ScriptOutput::from_rpc(&output(0, "1.0", recipient))?.address;
    let graph = api::investigation(&db, &root, 40).await?;
    assert!(
        graph["graph"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["tx_hash"] == btc["tx_hash"] && edge["block_number"] == 1)
    );
    let spend = format!(
        "/internal/bridge/tbtc/spends/{}?block_number=1",
        btc["tx_hash"].as_str().unwrap()
    );
    assert_eq!(
        api_value(&db, &format!("{spend}&output_index=1")).await?["inputs"],
        json!([])
    );
    let mut two = fixture(2, &"d".repeat(64), &"a".repeat(64), recipient);
    let spend_tx = "e".repeat(64);
    two["tx"].as_array_mut().unwrap().push(json!({
        "txid":spend_tx, "vin":[{"txid":btc["tx_hash"],"vout":1}],
        "vout":[output(0,"0.00990000",recipient)]
    }));
    blocks.write().unwrap().push(two);
    ingestion::run(db.clone(), rpc.clone(), config.clone(), true).await?;
    let spent = api_value(&db, &format!("{spend}&output_index=1")).await?;
    assert_eq!(spent["inputs"].as_array().unwrap().len(), 1);
    assert_eq!(spent["inputs"][0]["tx_hash"], spend_tx);
    assert_eq!(spent["inputs"][0]["block_number"], 2);
    assert_eq!(spent["inputs"][0]["input_index"], 0);
    assert_eq!(spent["inputs"][0]["prev_output_index"], 1);
    assert_eq!(
        api_value(&db, &format!("{spend}&output_index=0")).await?["inputs"],
        json!([])
    );
    let replacement = fixture(1, &"b".repeat(64), bitcoin_aml::GENESIS, recipient);
    let replacement_tip = fixture(2, &"c".repeat(64), &"b".repeat(64), recipient);
    *blocks.write().unwrap() = vec![genesis, replacement, replacement_tip];
    ingestion::run(db.clone(), rpc, config, true).await?;
    assert_eq!(db.state().await?.tip_height, 2);
    assert_eq!(api_value(&db, &funding).await?["outputs"], json!([]));
    assert_eq!(
        api_value(&db, &format!("{spend}&output_index=1")).await?["inputs"],
        json!([])
    );
    mock.abort();
    Ok(())
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
    assert_eq!(readiness(&db).await?, axum::http::StatusCode::OK);
    let root=db.rows::<Value>("SELECT address FROM aml_bitcoin_outputs_canonical WHERE address IS NOT NULL GROUP BY address HAVING count()>1 LIMIT 1",&[]).await?[0]["address"].as_str().unwrap().to_owned();
    let first = transfer_page(&db, &root, "limit=1").await?;
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    let second = transfer_page(&db, &root, &format!("limit=1&cursor={cursor}")).await?;
    assert_eq!(first["source_snapshot"], second["source_snapshot"]);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
    assert_eq!(first["model"], "address_transaction_utxo");
    let filtered = transfer_page(
        &db,
        &root,
        "limit=100&from_block=1&to_block=1&asset_id=native:BTC",
    )
    .await?;
    assert!(
        filtered["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["block_number"] == 1 && r["amount"].is_string())
    );
    let legs: Vec<Value> = db.rows("SELECT count() AS n,countIf(from_subject_type='address' AND to_subject_type='address') AS fabricated,countIf(amount_raw=4999999999) AS exact FROM aml_address_relationships_canonical",&[]).await?;
    assert_eq!(legs[0]["n"], 4);
    assert_eq!(legs[0]["fabricated"], 0);
    assert_eq!(legs[0]["exact"], 1);
    async fn maintenance_cli(rpc_url: &str, args: &[&str]) -> Result<Value> {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_bitcoin_aml"));
        command
            .current_dir(std::env::temp_dir())
            .env_clear()
            .env(
                "BITCOIN_CLICKHOUSE_URL",
                std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?,
            )
            .env("BITCOIN_CLICKHOUSE_USER", "bitcoin_test")
            .env("BITCOIN_CLICKHOUSE_PASSWORD", "bitcoin-test-only")
            .env("BITCOIN_RPC_URL", rpc_url)
            .env("BITCOIN_RPC_USER", "test")
            .env("BITCOIN_RPC_PASSWORD", "test")
            .env(
                "BITCOIN_WRITER_LOCK_DIR",
                std::env::var("BITCOIN_WRITER_LOCK_DIR")?,
            )
            .env("BITCOIN_START_BLOCK", "0")
            .env("BITCOIN_CONFIRMATIONS", "1")
            .args(["maintenance"])
            .args(args);
        let output = tokio::task::spawn_blocking(move || command.output()).await??;
        anyhow::ensure!(
            output.status.success(),
            "operator CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }
    let scan = maintenance_cli(
        &rpc_url,
        &[
            "scan-gaps",
            "--from-block",
            "0",
            "--to-block",
            "1",
            "--max-blocks",
            "2",
        ],
    )
    .await?;
    assert_eq!(scan["gaps"], 0);
    let snapshot = db.state().await?;
    db.execute("ALTER TABLE aml_transactions DELETE WHERE block_number=1 AND transaction_index=1 SETTINGS mutations_sync=2").await?;
    assert_eq!(
        maintenance_cli(
            &rpc_url,
            &[
                "scan-gaps",
                "--from-block",
                "0",
                "--to-block",
                "1",
                "--max-blocks",
                "2"
            ]
        )
        .await?["gaps"],
        1
    );
    assert_eq!(
        maintenance_cli(
            &rpc_url,
            &[
                "replay",
                "--from-block",
                "1",
                "--to-block",
                "1",
                "--max-blocks",
                "1"
            ]
        )
        .await?["status"],
        "complete"
    );
    assert_eq!(db.state().await?.tip_height, snapshot.tip_height);
    assert_eq!(db.state().await?.tip_hash, snapshot.tip_hash);
    assert!(db.state().await?.revision > snapshot.revision);
    assert_eq!(
        maintenance_cli(
            &rpc_url,
            &[
                "scan-gaps",
                "--from-block",
                "0",
                "--to-block",
                "1",
                "--max-blocks",
                "2"
            ]
        )
        .await?["gaps"],
        0
    );
    if let Ok(path) = std::env::var("CHAIN_RESPONSE_PROOF") {
        let target=db.rows::<Value>("SELECT address FROM aml_bitcoin_outputs_canonical WHERE address IS NOT NULL AND address!={root:String} LIMIT 1",&[("root",root.clone())]).await?[0]["address"].as_str().unwrap().to_owned();
        let wallet = api_value(
            &db,
            &format!("/api/bitcoin/wallet/{root}/investigation?limit=40"),
        )
        .await?;
        assert_eq!(
            wallet["semantic_evidence"]["contract"],
            "chain_semantic_evidence_v1"
        );
        assert_eq!(
            wallet["semantic_evidence"]["coverage"]["nft_transfers"],
            "not_applicable"
        );
        assert_eq!(
            wallet["risk_engine"]["policy_version"],
            aml_risk_policy::POLICY
        );
        let paths = api_value(
            &db,
            &format!("/api/bitcoin/wallet/{root}/paths/{target}?max_hops=3&max_nodes=200"),
        )
        .await?;
        let mut receipts = serde_json::Map::new();
        let mut routes = vec![
            "/status".into(),
            "/api/bitcoin/assets/native".into(),
            "/api/bitcoin/semantics/config".into(),
            "/api/bitcoin/clusters/config".into(),
            format!("/api/bitcoin/wallet/{root}/intelligence/snapshot"),
        ];
        for (field, kind) in [
            ("exposure_snapshot", "exposure"),
            ("cluster_snapshot", "clusters"),
            ("risk_snapshot", "risk"),
        ] {
            let key = if kind == "risk" {
                "assessment_id"
            } else {
                "run_id"
            };
            if let Some(id) = wallet[field][key].as_str() {
                routes.push(format!("/api/bitcoin/wallet/{root}/{kind}/{id}/snapshot"));
            }
        }
        for route in routes {
            receipts.insert(route.clone(), api_value(&db, &route).await?);
        }
        std::fs::write(
            path,
            serde_json::to_vec(&json!({"wallet":wallet,"paths":paths,"receipts":receipts}))?,
        )?;
    }
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
    assert_eq!(investigation["source_snapshot"]["contract"], "canonical_v1");
    assert_eq!(
        investigation["source_snapshot"]["source_revision"]
            .as_str()
            .unwrap()
            .parse::<u64>()?,
        initial.revision
    );
    assert_eq!(investigation["source_snapshot"]["finality"], "not_asserted");
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
    assert_ne!(
        db.status().await?["source_snapshot"]["snapshot_id"],
        investigation["source_snapshot"]["snapshot_id"]
    );
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
