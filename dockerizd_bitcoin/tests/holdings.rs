//! Disposable amount-preserving fixtures, not consensus-valid Bitcoin blocks.
use anyhow::Result;
use bitcoin_aml::{
    GENESIS, api, assets,
    config::DatabaseConfig,
    db::Database,
    holdings::{self, Query},
    model,
    warehouse::{self, LegacyBlock, WriteContext},
};
use serde_json::{Value, json};
use std::collections::HashMap;
use tower::ServiceExt;
fn output(index: usize, sats: u64, byte: u8) -> Value {
    let script = if byte == 0 {
        "6a0100".into()
    } else {
        format!("76a914{}88ac", format!("{byte:02x}").repeat(20))
    };
    json!({"n":index,"value":model::decimal(sats),"scriptPubKey":{"hex":script,"type":if byte==0{"nulldata"}else{"pubkeyhash"}}})
}
fn address(byte: u8) -> String {
    model::ScriptOutput::from_rpc(&output(0, 1, byte))
        .unwrap()
        .address
}
fn coinbase(height: u64) -> Value {
    json!({"txid":format!("{:064x}",height+1),"vin":[{"coinbase":"00"}],"vout":[output(0,1,90)]})
}
fn spend(hash: char, prev: String, index: u32, byte: u8, sats: u64) -> Value {
    json!({"txid":hash.to_string().repeat(64),"vin":[{"txid":prev,"vout":index}],"vout":[output(0,sats,byte)]})
}
async fn append(
    db: &Database,
    height: u64,
    hash: String,
    transactions: Vec<Value>,
    resolved: &mut HashMap<String, model::ScriptOutput>,
    start: u64,
) -> Result<()> {
    let previous = db.state().await?;
    let block = json!({"height":height,"hash":hash,"previousblockhash":if height==0 {String::new()}else if previous.initialized==1{previous.tip_hash.clone()}else{format!("{:064x}",height-1)},"time":1700000000+height,"tx":transactions});
    let evidence = model::extract(&block, &format!("holdings-{height}"), resolved)?;
    let source = LegacyBlock {
        block_height: height,
        block_hash: hash,
        parent_hash: block["previousblockhash"].as_str().unwrap().into(),
        block_time: 1700000000 + height,
        commit_id: format!("holdings-{height}"),
        transaction_count: evidence.transactions.len() as u32,
    };
    warehouse::write_block(
        db,
        &previous,
        &source,
        &evidence,
        WriteContext {
            observed: height,
            start,
            source_id: "synthetic-holdings-fixture",
            legacy_fence: None,
        },
    )
    .await?;
    for o in evidence.outputs {
        resolved.insert(model::outpoint(&o.txid, o.output_index), o.output);
    }
    Ok(())
}
async fn database(name: &str) -> Result<Database> {
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: name.into(),
    })?;
    db.initialize().await?;
    assert_eq!(db.state().await?.revision, 0, "fresh fixture only");
    Ok(db)
}
async fn get(router: axum::Router, path: &str, key: Option<&str>) -> Result<(u16, Value)> {
    let request = axum::http::Request::builder().uri(path);
    let request = if let Some(key) = key {
        request.header("x-aml-service-key", key)
    } else {
        request
    };
    let response = router
        .oneshot(request.body(axum::body::Body::empty())?)
        .await?;
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024).await?;
    Ok((
        status,
        serde_json::from_slice(&body).unwrap_or_else(|_| json!(String::from_utf8_lossy(&body))),
    ))
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8"]
async fn native_metadata_exact_holdings_maturity_reorg_and_output_states() -> Result<()> {
    let db = database("bitcoin_holdings_test").await?;
    let metadata = assets::read(&db).await?;
    assets::initialize(&db).await?;
    assert_eq!(metadata, assets::read(&db).await?);
    let counts: Vec<Value> = db
        .rows("SELECT count() AS n FROM aml_asset_metadata", &[])
        .await?;
    assert_eq!(counts[0]["n"], 1);
    let empty = api::wallet_holdings(&db, &address(4)).await?;
    assert_eq!(empty["status"], "waiting_for_blocks");
    assert!(empty["balance_sats"].is_null());
    assert!(empty["utxo_count"].is_null());
    assert!(holdings::binding_matches(&empty));
    let key = "holdings-fixture-key-000000000000000000";
    let router = api::router(api::Api::new(db.clone(), key.into())?);
    assert_eq!(
        get(router.clone(), "/api/bitcoin/assets/native", None)
            .await?
            .0,
        401
    );
    let native = get(router.clone(), "/api/bitcoin/assets/native", Some(key)).await?;
    assert_eq!(native.0, 200);
    assert_eq!(native.1["assets"][0]["decimals"], 8);
    assert!(native.1["assets"][0]["total_supply_raw"].is_null());
    for suffix in ["?limit=0", "?limit=101", "?asset=erc20"] {
        assert_eq!(
            get(
                router.clone(),
                &format!("/api/bitcoin/wallet/{}/holdings{suffix}", address(4)),
                Some(key)
            )
            .await?
            .0,
            400
        );
    }
    let mut resolved = HashMap::new();
    let first = format!("{:064x}", 1);
    let mut immature = Value::Null;
    let mut boundary = Value::Null;
    for height in 0..=100 {
        let mut transactions = vec![coinbase(height)];
        if height == 0 {
            transactions[0]["vout"] = json!([
                output(0, 100, 4),
                output(1, 50, 4),
                output(2, 200, 5),
                output(3, 7, 0)
            ]);
        }
        if height == 1 {
            transactions.push(spend('a', first.clone(), 1, 6, 49));
            transactions.push(spend('b', first.clone(), 2, 4, 199));
        }
        let hash = if height == 0 {
            GENESIS.into()
        } else {
            format!("{:064x}", 10000 + height)
        };
        append(&db, height, hash, transactions, &mut resolved, 0).await?;
        if height == 98 {
            immature = api::wallet_holdings(&db, &address(4)).await?;
            assert_eq!(immature["balance_sats"], "299");
            assert_eq!(immature["immature_sats"], "100");
            assert_eq!(immature["mature_sats"], "199");
            assert_eq!(immature["eligible_next_block_sats"], "199");
        }
        if height == 99 {
            boundary = api::wallet_holdings(&db, &address(4)).await?;
            assert_eq!(boundary["immature_sats"], "100");
            assert_eq!(boundary["eligible_next_block_sats"], "299");
        }
    }
    let full = api::investigation(&db, &address(4), 40).await?;
    assert_eq!(full["holdings"]["balance_sats"], "299");
    assert_eq!(full["holdings"]["balance_decimal"], "0.00000299");
    assert_eq!(full["holdings"]["mature_sats"], "299");
    assert_eq!(full["holdings"]["immature_sats"], "0");
    assert_eq!(full["holdings"]["utxo_count"], 2);
    assert_eq!(full["holdings"]["spent_output_count"], 1);
    assert_eq!(full["holdings"]["latest_output_count"], 3);
    assert_eq!(full["holdings"]["status"], "available");
    assert_eq!(full["holdings"]["is_complete_balance"], true);
    assert!(full["holdings"]["spendable_sats"].is_null());
    assert_eq!(full["holdings"]["capabilities"]["erc20"], "not_applicable");
    assert_eq!(full["risk_engine"]["risk_level"], "UNKNOWN");
    assert!(holdings::binding_matches(&full["holdings"]));
    let spent = full["holdings"]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["status"] == "spent")
        .unwrap();
    assert_eq!(spent["spent_in_tx_hash"], "a".repeat(64));
    assert_eq!(spent["value_sats"], "50");
    let limited = holdings::read(&db, &address(4), &Query { limit: 1 }).await?;
    assert_eq!(limited["outputs_truncated"], true);
    assert_eq!(limited["balance_sats"], "299");
    let zero = api::wallet_holdings(&db, &address(99)).await?;
    assert_eq!(zero["status"], "available");
    assert_eq!(zero["balance_sats"], "0");
    let mut bad = full["holdings"].clone();
    bad["balance_sats"] = json!("0");
    assert!(!holdings::binding_matches(&bad));
    let endpoint = get(
        router.clone(),
        &format!("/api/bitcoin/wallet/{}/holdings", address(4)),
        Some(key),
    )
    .await?;
    assert_eq!(endpoint.0, 200);
    assert_eq!(endpoint.1, full["holdings"]);
    if let Ok(file) = std::env::var("BITCOIN_HOLDINGS_ACCEPTANCE") {
        std::fs::write(
            file,
            serde_json::to_vec_pretty(
                &json!({"scope":"synthetic ClickHouse holdings acceptance","full":full,"empty":empty,"immature":immature,"boundary":boundary,"limited":limited,"zero":zero,"metadata":metadata}),
            )?,
        )?;
    }
    let previous = db.state().await?;
    let mut rolled = previous.clone();
    rolled.revision = db.next_revision(previous.revision).await?;
    rolled.tip_height = 99;
    rolled.tip_hash = format!("{:064x}", 10099);
    db.publish_after(&previous, &rolled).await?;
    assert_eq!(
        api::wallet_holdings(&db, &address(4)).await?["immature_sats"],
        "100"
    );
    append(
        &db,
        100,
        "f".repeat(64),
        vec![coinbase(100), spend('c', "b".repeat(64), 0, 7, 198)],
        &mut resolved,
        0,
    )
    .await?;
    let replaced = api::wallet_holdings(&db, &address(4)).await?;
    assert_eq!(replaced["balance_sats"], "100");
    assert_eq!(replaced["spent_output_count"], 2);
    assert_ne!(replaced["source_snapshot"], full["source_snapshot"]);
    // A conflicting native registry row must fail reads/readiness and is never auto-repaired.
    let rows: Vec<Value> = db
        .rows(
            "SELECT * FROM aml_asset_metadata_current WHERE asset_id='native:BTC'",
            &[],
        )
        .await?;
    let mut poisoned = rows[0].clone();
    poisoned["symbol"] = json!("FAKE");
    poisoned["revision"] = json!(poisoned["revision"].as_u64().unwrap() + 1);
    reqwest::Client::new()
        .post(std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?)
        .query(&[
            ("database", "bitcoin_holdings_test"),
            ("query", "INSERT INTO aml_asset_metadata FORMAT JSONEachRow"),
        ])
        .header("X-ClickHouse-User", "bitcoin_test")
        .header("X-ClickHouse-Key", "bitcoin-test-only")
        .body(format!("{poisoned}\n"))
        .send()
        .await?
        .error_for_status()?;
    assert!(assets::read(&db).await.is_err());
    assert!(assets::initialize(&db).await.is_err());
    assert!(db.ready().await.is_err());
    assert_eq!(
        get(router, "/api/bitcoin/assets/native", Some(key))
            .await?
            .0,
        503
    );
    let after: Vec<Value> = db
        .rows("SELECT count() AS n FROM aml_asset_metadata", &[])
        .await?;
    assert_eq!(after[0]["n"], 2);
    Ok(())
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8"]
async fn repeated_txid_address_replacement_and_partial_history() -> Result<()> {
    let db = database("bitcoin_holdings_occurrences").await?;
    let mut resolved = HashMap::new();
    let duplicate = "d".repeat(64);
    append(&db,10,format!("{:064x}",10),vec![json!({"txid":duplicate,"vin":[{"coinbase":"00"}],"vout":[output(0,100,4),output(1,50,5)]})],&mut resolved,10).await?;
    append(
        &db,
        11,
        format!("{:064x}", 11),
        vec![coinbase(11), spend('e', duplicate.clone(), 0, 6, 99)],
        &mut resolved,
        10,
    )
    .await?;
    append(&db,12,format!("{:064x}",12),vec![json!({"txid":duplicate,"vin":[{"coinbase":"00"}],"vout":[output(0,7,4),output(1,9,7)]})],&mut resolved,10).await?;
    let repeated = api::wallet_holdings(&db, &address(4)).await?;
    assert_eq!(repeated["status"], "partial");
    assert_eq!(repeated["balance_sats"], "7");
    assert_eq!(repeated["immature_sats"], "7");
    assert_eq!(repeated["spent_output_count"], 0);
    assert_eq!(repeated["outputs"][0]["block_number"], 12);
    let old_address = api::wallet_holdings(&db, &address(5)).await?;
    assert_eq!(old_address["balance_sats"], "0");
    assert_eq!(old_address["latest_output_count"], 0);
    assert_eq!(
        old_address["is_complete_balance"], false,
        "zero observed in a partial window is not a complete zero balance"
    );
    let previous = db.state().await?;
    let mut rolled = previous.clone();
    rolled.revision = db.next_revision(previous.revision).await?;
    rolled.tip_height = 11;
    rolled.tip_hash = format!("{:064x}", 11);
    db.publish_after(&previous, &rolled).await?;
    let restored = api::wallet_holdings(&db, &address(4)).await?;
    assert_eq!(restored["balance_sats"], "0");
    assert_eq!(restored["spent_output_count"], 1);
    assert_eq!(restored["outputs"][0]["spent_in_tx_hash"], "e".repeat(64));
    assert_eq!(
        api::wallet_holdings(&db, &address(5)).await?["balance_sats"],
        "50"
    );
    Ok(())
}
