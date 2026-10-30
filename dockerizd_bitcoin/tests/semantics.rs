//! Disposable, amount-preserving synthetic blocks; not consensus-valid mainnet.
use aml_warehouse::{
    Connection,
    publication::{self, Block},
};
use anyhow::Result;
use bitcoin_aml::{
    GENESIS, NETWORK_ID, activity, api,
    config::DatabaseConfig,
    db::{self, Database},
    model, semantics,
    warehouse::{self, LegacyBlock, WriteContext},
};
use serde_json::{Value, json};
use std::collections::HashMap;
const DATABASE: &str = "bitcoin_semantic_test";
struct TestConnection {
    url: String,
}
impl TestConnection {
    async fn request(&self, sql: &str, body: &str) -> Result<String> {
        Ok(reqwest::Client::new()
            .post(&self.url)
            .query(&[
                ("query", sql),
                ("database", DATABASE),
                ("output_format_json_quote_64bit_integers", "0"),
            ])
            .header("X-ClickHouse-User", "bitcoin_test")
            .header("X-ClickHouse-Key", "bitcoin-test-only")
            .header(reqwest::header::CONTENT_LENGTH, body.len())
            .body(body.to_owned())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    }
}
impl Connection for TestConnection {
    async fn insert_json(&self, table: &str, body: &str) -> Result<()> {
        self.request(&format!("INSERT INTO {table} FORMAT JSONEachRow"), body)
            .await?;
        Ok(())
    }
    async fn execute(&self, sql: &str) -> Result<()> {
        self.request(sql, "").await?;
        Ok(())
    }
    async fn query(&self, sql: &str) -> Result<Vec<String>> {
        self.request(&format!("{sql} FORMAT JSONEachRow"), "")
            .await?
            .lines()
            .map(|l| {
                let v: Value = serde_json::from_str(l)?;
                Ok(v.as_object()
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
fn output(index: usize, sats: u64, byte: u8) -> Value {
    json!({"n":index,"value":model::decimal(sats),"scriptPubKey":{"hex":format!("76a914{}88ac",format!("{byte:02x}").repeat(20)),"type":"pubkeyhash"}})
}
fn address(byte: u8) -> String {
    model::ScriptOutput::from_rpc(&output(0, 1, byte))
        .unwrap()
        .address
}
fn spend(hash: char, inputs: Vec<(String, u32)>, outputs: Vec<(u8, u64)>) -> Value {
    json!({"txid":hash.to_string().repeat(64),"vin":inputs.into_iter().map(|(tx,index)|json!({"txid":tx,"vout":index})).collect::<Vec<_>>(),"vout":outputs.into_iter().enumerate().map(|(i,(byte,sats))|output(i,sats,byte)).collect::<Vec<_>>()})
}
fn prior(hash: char, index: u32) -> (String, u32) {
    (hash.to_string().repeat(64), index)
}
async fn append(
    db: &Database,
    conn: &TestConnection,
    height: u64,
    transactions: Vec<Value>,
    resolved: &mut HashMap<String, model::ScriptOutput>,
    old_writer: bool,
) -> Result<()> {
    let previous = db.state().await?;
    let hash = if height == 0 {
        GENESIS.into()
    } else {
        format!("{:064x}", 10000 + height)
    };
    let block = json!({"height":height,"hash":hash,"previousblockhash":previous.tip_hash,"time":1700000000+height,"tx":transactions});
    let evidence = model::extract(&block, &format!("fixture-{height}"), resolved)?;
    let source = LegacyBlock {
        block_height: height,
        block_hash: hash.clone(),
        parent_hash: previous.tip_hash.clone(),
        block_time: 1700000000 + height,
        commit_id: format!("fixture-{height}"),
        transaction_count: evidence.transactions.len() as u32,
    };
    if old_writer {
        // Reproduce the previous writer contract: no post-hoc changes to a seal.
        warehouse::validate_evidence(&source, &evidence)?;
        let revision = db.next_revision(previous.revision).await?;
        let block = Block {
            network_id: NETWORK_ID.into(),
            block_number: height,
            block_hash: hash.clone(),
            block_timestamp_unix_ms: source.block_time * 1000,
            commit_id: format!("{revision}-{hash}"),
            revision,
            parent_hash: previous.tip_hash.clone(),
            canonical: 1,
            ingestion_status: "complete".into(),
            transaction_count: source.transaction_count,
            receipt_coverage: None,
            trace_coverage: None,
            prevout_coverage: Some(1),
            source_id: "synthetic-pre-semantic-writer".into(),
            created_at_unix_ms: db::timestamp(),
        };
        let mut tables = warehouse::map_evidence(&block, &evidence)?;
        tables.remove("aml_semantic_aml_events");
        publication::reserve(conn, DATABASE, &block).await?;
        for (name, rows) in &tables {
            publication::facts(conn, DATABASE, &block, name, rows).await?;
        }
        warehouse::verify_rows(db, &block, &tables).await?;
        aml_warehouse::core::complete(
            conn,
            DATABASE,
            &block,
            &tables
                .iter()
                .map(|(n, r)| (n.to_string(), r.len() as u64))
                .collect(),
            &serde_json::to_string(&source)?,
        )
        .await?;
        let mut next = previous.clone();
        next.revision = revision;
        next.tip_height = height;
        next.tip_hash = hash;
        next.observed_rpc_height = height;
        next.published_at = db::timestamp();
        db.publish_after(&previous, &next).await?;
    } else {
        warehouse::write_block(
            db,
            &previous,
            &source,
            &evidence,
            WriteContext {
                observed: height,
                start: 0,
                source_id: "synthetic-semantic-fixture",
                legacy_fence: None,
            },
        )
        .await?;
    }
    for o in evidence.outputs {
        resolved.insert(model::outpoint(&o.txid, o.output_index), o.output);
    }
    Ok(())
}
fn coinbase(height: u64) -> Value {
    json!({"txid":format!("{:064x}",2000+height),"vin":[{"coinbase":"00"}],"vout":[output(0,10000,90)]})
}
async fn read(db: &Database, byte: u8, query: &activity::Query) -> Result<Value> {
    let a = address(byte);
    semantics::read(db, &a, &activity::read(db, &a, query).await?).await
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8"]
async fn semantic_facts_patterns_history_limits_and_risk_snapshots() -> Result<()> {
    use tower::ServiceExt;
    let url = std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?;
    let conn = TestConnection { url: url.clone() };
    let db = Database::new(DatabaseConfig {
        url: url.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: DATABASE.into(),
    })?;
    db.initialize().await?;
    assert_eq!(db.state().await?.revision, 0, "fresh fixture only");
    let mut resolved = HashMap::new();
    let first = format!("{:064x}", 2000);
    append(&db,&conn,0,vec![json!({"txid":first,"vin":[{"coinbase":"00"}],"vout":(0..12).map(|i|output(i,10000,i as u8+1)).collect::<Vec<_>>()})],&mut resolved,false).await?;
    append(
        &db,
        &conn,
        1,
        vec![
            coinbase(1),
            spend(
                'a',
                (0..5).map(|i| (first.clone(), i)).collect(),
                vec![(21, 49000)],
            ),
            spend(
                'b',
                vec![(first.clone(), 5)],
                (31..36).map(|b| (b, 1900)).collect(),
            ),
            spend(
                'c',
                (6..9).map(|i| (first.clone(), i)).collect(),
                vec![(41, 9000), (42, 9000), (43, 9000)],
            ),
            spend('d', vec![(first.clone(), 9)], vec![(30, 8000), (50, 1900)]),
        ],
        &mut resolved,
        false,
    )
    .await?;
    append(
        &db,
        &conn,
        2,
        vec![
            coinbase(2),
            spend('e', vec![prior('d', 0)], vec![(30, 6400), (51, 1500)]),
            spend('8', vec![(first.clone(), 11)], vec![(30, 8000), (52, 1900)]),
            spend(
                '9',
                vec![(first.clone(), 10)],
                (0..256).map(|_| (61, 1)).collect(),
            ),
        ],
        &mut resolved,
        false,
    )
    .await?;
    append(
        &db,
        &conn,
        3,
        vec![
            coinbase(3),
            spend('f', vec![prior('e', 0)], vec![(30, 5000), (53, 1200)]),
        ],
        &mut resolved,
        false,
    )
    .await?;
    append(
        &db,
        &conn,
        4,
        vec![
            coinbase(4),
            spend(
                '7',
                vec![prior('b', 0)],
                (60..65).map(|b| (b, 300)).collect(),
            ),
        ],
        &mut resolved,
        true,
    )
    .await?;
    let patterns = api::investigation(&db, &address(30), 40).await?;
    let events = patterns["semantic_events"].as_array().unwrap();
    let chains = events
        .iter()
        .filter(|e| e["event_type"] == "bitcoin_peel_chain_candidate")
        .collect::<Vec<_>>();
    assert_eq!(chains.len(), 1, "deduplicate the same three-step chain");
    assert_eq!(
        chains[0]["evidence"]["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["tx_hash"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["d".repeat(64), "e".repeat(64), "f".repeat(64)]
    );
    assert_eq!(patterns["semantic_coverage"]["canonical_fact_events"], 4);
    assert_eq!(patterns["risk_engine"]["risk_level"], "UNKNOWN");
    assert!(patterns["risk_engine"]["risk_score"].is_null());
    let id = patterns["risk_snapshot"]["assessment_id"].as_str().unwrap();
    let stored = bitcoin_aml::risk::inspect(&db, &address(30), id).await?;
    assert_eq!(stored["current"], true);
    let saved_inputs: Value =
        serde_json::from_str(stored["assessment"]["evidence_json"].as_str().unwrap())?;
    assert_eq!(
        saved_inputs["inputs"]["semantic_events"],
        patterns["semantic_events"]
    );
    let coinjoin = api::investigation(&db, &address(41), 40).await?;
    assert_eq!(
        coinjoin["semantic_events"][0]["event_type"],
        "bitcoin_possible_coinjoin"
    );
    assert_eq!(coinjoin["semantic_events"][0]["storage"], "canonical_fact");
    assert!(coinjoin["risk_engine"]["risk_score"].is_null());
    assert_eq!(
        read(&db, 21, &Default::default()).await?["semantic_events"][0]["event_type"],
        "bitcoin_consolidation_candidate"
    );
    let historical = read(&db, 60, &Default::default()).await?;
    assert_eq!(
        historical["semantic_events"][0]["event_type"],
        "bitcoin_fan_out_candidate"
    );
    assert_eq!(
        historical["semantic_events"][0]["storage"],
        "computed_from_canonical_facts"
    );
    let counts:Vec<Value>=db.rows("SELECT block_number,expected_counts_json,raw_evidence_json FROM aml_ingested_blocks_canonical ORDER BY block_number",&[]).await?;
    let current = counts.iter().find(|b| b["block_number"] == 1).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(current["expected_counts_json"].as_str().unwrap())?["aml_semantic_aml_events"],
        4
    );
    let capped = counts.iter().find(|b| b["block_number"] == 2).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(capped["raw_evidence_json"].as_str().unwrap())?["semantic_detection"]
            ["skipped_transactions"],
        1
    );
    let partial = read(&db, 61, &Default::default()).await?;
    assert_eq!(partial["semantic_coverage"]["status"], "partial");
    assert!(
        partial["semantic_events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["tx_hash"] != "9".repeat(64)),
        "the oversized transaction must not be classified from partial legs"
    );
    assert_eq!(
        partial["semantic_events"][0]["event_type"], "bitcoin_fan_out_candidate",
        "valid historical evidence for this same address remains available"
    );
    let scoped = read(
        &db,
        30,
        &activity::Query {
            from_block: Some(2),
            to_block: Some(2),
            ..Default::default()
        },
    )
    .await?;
    assert!(
        scoped["semantic_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event_type"] == "bitcoin_peel_chain_candidate")
    );
    assert_eq!(
        scoped["semantic_coverage"]["query_scope"]["filters"]["from_block"],
        2
    );
    let key = "semantic-test-key-00000000000000000000";
    let router = api::router(api::Api::new(db.clone(), key.into())?);
    let route = format!(
        "/api/bitcoin/wallet/{}/semantics?from_block=2&to_block=2",
        address(30)
    );
    let request = |authenticated: bool| {
        let b = axum::http::Request::builder().uri(&route);
        let b = if authenticated {
            b.header("x-aml-service-key", key)
        } else {
            b
        };
        b.body(axum::body::Body::empty()).unwrap()
    };
    assert_eq!(router.clone().oneshot(request(false)).await?.status(), 401);
    let response = router.clone().oneshot(request(true)).await?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: Value =
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), MAX_RESPONSE).await?)?;
    assert_eq!(body, scoped);
    let assessment_count: Vec<Value> = db
        .rows("SELECT count() AS n FROM aml_wallet_risk_assessments", &[])
        .await?;
    router.oneshot(request(true)).await?;
    assert_eq!(
        assessment_count,
        db.rows::<Value>("SELECT count() AS n FROM aml_wallet_risk_assessments", &[])
            .await?,
        "read endpoint is not a risk writer"
    );
    if let Ok(file) = std::env::var("BITCOIN_SEMANTIC_ACCEPTANCE") {
        std::fs::write(
            file,
            serde_json::to_vec_pretty(
                &json!({"scope":"synthetic semantic ClickHouse acceptance","patterns":patterns,"coinjoin":coinjoin,"historical":historical,"partial":partial,"scoped":scoped}),
            )?,
        )?;
    }
    let before = db.state().await?;
    let mut txs = vec![coinbase(5)];
    for i in 0..30 {
        let hash = format!("{:064x}", 3000 + i);
        let prior = if i == 0 {
            prior('f', 0)
        } else {
            (format!("{:064x}", 2999 + i), 0)
        };
        txs.push(json!({"txid":hash,"vin":[{"txid":prior.0,"vout":prior.1}],"vout":[output(0,4999-i,30)]}));
    }
    append(&db, &conn, 5, txs, &mut resolved, false).await?;
    let limited = read(&db, 30, &Default::default()).await?;
    assert_eq!(limited["semantic_coverage"]["roots_truncated"], true);
    assert_eq!(limited["semantic_coverage"]["status"], "partial");
    // Hide all events from the old branch via the publication fence, preserving raw history.
    let previous = db.state().await?;
    let mut rolled = before;
    rolled.revision = db.next_revision(previous.revision).await?;
    rolled.tip_height = 0;
    rolled.tip_hash = GENESIS.into();
    db.publish_after(&previous, &rolled).await?;
    assert!(
        read(&db, 30, &Default::default()).await?["semantic_events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let visible: Vec<Value> = db
        .rows(
            "SELECT count() AS n FROM aml_semantic_aml_events_canonical",
            &[],
        )
        .await?;
    assert_eq!(visible[0]["n"], 0);
    assert_eq!(
        bitcoin_aml::risk::inspect(&db, &address(30), id).await?["current"],
        false
    );
    Ok(())
}
const MAX_RESPONSE: usize = 4 * 1024 * 1024;
