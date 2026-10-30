//! Amount-preserving synthetic fixtures, not consensus-valid Bitcoin blocks.
//! Only a fresh disposable ClickHouse. No node, network labels or production DB.
use anyhow::Result;
use bitcoin_aml::{
    GENESIS, NETWORK_ID, api,
    config::DatabaseConfig,
    db::Database,
    exposure::{self, Options},
    intelligence::{self, Claim, Review, Source},
    model,
    storage::writer::{self, WriteContext},
    warehouse::LegacyBlock,
};
use serde_json::{Value, json};
use std::collections::HashMap;
fn output(index: u64, sats: u64, byte: u8) -> Value {
    let script = if byte == 0 {
        "51".into()
    } else {
        format!("76a914{}88ac", format!("{byte:02x}").repeat(20))
    };
    json!({"n":index,"value":model::decimal(sats),"scriptPubKey":{"hex":script,"type":if byte==0{"nonstandard"}else{"pubkeyhash"}}})
}
fn address(byte: u8) -> String {
    model::ScriptOutput::from_rpc(&output(0, 1, byte))
        .unwrap()
        .address
}
fn spend(hash: char, inputs: Vec<(String, u32)>, outputs: Vec<(u8, u64)>) -> Value {
    json!({"txid":hash.to_string().repeat(64),"vin":inputs.into_iter().map(|(tx,vout)|json!({"txid":tx,"vout":vout})).collect::<Vec<_>>(),"vout":outputs.into_iter().enumerate().map(|(i,(byte,sats))|output(i as u64,sats,byte)).collect::<Vec<_>>()})
}
fn prev(height: u64, index: u32) -> (String, u32) {
    (format!("{:064x}", height + 1), index)
}
fn txprev(hash: char, index: u32) -> (String, u32) {
    (hash.to_string().repeat(64), index)
}
async fn fixtures(db: &Database) -> Result<()> {
    fixtures_with_clock(db, bitcoin_aml::db::timestamp() / 1000 - 4, 1).await
}
async fn fixtures_with_clock(db: &Database, base: u64, step: u64) -> Result<()> {
    let mut state = db.state().await?;
    let mut resolved = HashMap::new();
    for height in 0..4 {
        let mut txs = vec![
            json!({"txid":format!("{:064x}",height+1),"vin":[{"coinbase":"00"}],"vout":if height==0 {
            vec![output(0,100,1),output(1,100,1),output(2,100,2),output(3,100,5),output(4,100,8),output(5,50,9),output(6,100,3),output(7,60,1),output(8,40,1),output(9,60,1)]
        }else{vec![output(0,100,15)]}}),
        ];
        if height == 1 {
            txs.extend([
                spend('a', vec![prev(0, 0)], vec![(4, 60), (3, 39)]),
                spend('b', vec![prev(0, 1), prev(0, 2)], vec![(6, 99), (2, 100)]),
                spend('c', vec![prev(0, 3)], vec![(1, 90)]),
                spend('d', vec![prev(0, 7)], vec![(8, 59)]),
                spend('e', vec![prev(0, 8), prev(0, 9)], vec![(12, 99)]),
            ]);
        }
        if height == 2 {
            txs.extend([
                spend('f', vec![txprev('a', 1)], vec![(7, 38)]),
                spend('9', vec![prev(0, 6)], vec![(10, 99)]),
                spend('8', vec![txprev('d', 0)], vec![(11, 58)]),
                spend('7', vec![txprev('a', 0)], vec![(0, 50), (4, 9)]),
            ]);
        }
        if height == 3 {
            txs.push(spend('6', vec![txprev('7', 0)], vec![(13, 49)]));
        }
        let hash = if height == 0 {
            GENESIS.into()
        } else {
            format!("{:064x}", 100 + height)
        };
        let parent = if height == 0 {
            String::new()
        } else if height == 1 {
            GENESIS.into()
        } else {
            format!("{:064x}", 99 + height)
        };
        let block = json!({"height":height,"hash":hash,"previousblockhash":parent,"time":base+height*step,"tx":txs});
        let facts = model::extract(&block, &format!("fixture-{height}"), &resolved)?;
        let source = LegacyBlock {
            block_height: height,
            block_hash: block["hash"].as_str().unwrap().into(),
            parent_hash: block["previousblockhash"].as_str().unwrap().into(),
            block_time: block["time"].as_u64().unwrap(),
            commit_id: format!("fixture-{height}"),
            transaction_count: facts.transactions.len() as u32,
        };
        state = writer::write_block(
            db,
            &state,
            &source,
            &facts,
            WriteContext {
                observed: 3,
                start: 0,
                source_id: "synthetic-exposure-fixture",
                legacy_fence: None,
            },
        )
        .await?;
        for out in facts.outputs {
            resolved.insert(model::outpoint(&out.txid, out.output_index), out.output);
        }
    }
    Ok(())
}
fn source() -> Source {
    serde_json::from_value(json!({"network_id":NETWORK_ID,"source_id":"exposure-fixture","source_name":"Synthetic fixture only","source_type":"fixture","reference_url":"https://example.invalid/exposure","trust_tier":"unrated","license":"test-only","is_active":true,"created_by":"fixture-operator"})).unwrap()
}
fn claim(id: &str, byte: u8, risk: bool) -> Claim {
    serde_json::from_value(json!({"network_id":NETWORK_ID,"claim_id":id,"address":address(byte),"entity_id":id,"entity_name":"Synthetic fixture, no real attribution","entity_type":if risk{"reviewed_actor"}else{"exchange"},"address_role":"fixture","confidence":1.0,"risk_level":if risk{100}else{0},"is_exposure_seed":risk,"seed_category":if risk{"fixture_only"}else{""},"source_id":"exposure-fixture","source_reference":"case:synthetic-exposure","evidence_refs":["case:synthetic-exposure"],"submitted_by":"fixture-operator","valid_from_unix_ms":0})).unwrap()
}
async fn review(db: &Database, id: &str, revision: Value, decision: &str) -> Result<Value> {
    intelligence::review(db,serde_json::from_value::<Review>(json!({"network_id":NETWORK_ID,"claim_id":id,"expected_revision":revision,"decision":decision,"reviewed_by":"fixture-reviewer","reason":"Synthetic exposure acceptance","evidence_refs":["case:synthetic-review"]}))?,false).await
}
fn incoming(data: &Value) -> Vec<&Value> {
    data["exposure_paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["direction"] == "RECEIVED_FROM_SEED")
        .collect()
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8"]
async fn activity_exact_legs_fees_days_filters_and_counterparty_context() -> Result<()> {
    use bitcoin_aml::activity::{self, Direction, Query};
    use tower::ServiceExt;
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_activity_test".into(),
    })?;
    db.initialize().await?;
    assert_eq!(db.state().await?.revision, 0, "fresh fixture only");
    let empty = activity::read(&db, &address(4), &Default::default()).await?;
    assert_eq!(empty["activity_coverage"]["status"], "waiting_for_blocks");
    assert_eq!(empty["activity_coverage"]["summary_complete"], false);
    assert!(empty["fingerprint"]["first_seen_unix_ms"].is_null());
    fixtures_with_clock(&db, 1704067200, 86400).await?;
    let full = activity::read(&db, &address(4), &Default::default()).await?;
    let fp = &full["fingerprint"];
    assert_eq!(fp["transaction_count"], 2);
    assert_eq!(fp["transfer_count"], 3);
    assert_eq!(fp["received_output_count"], 2);
    assert_eq!(fp["spent_input_count"], 1);
    assert_eq!(fp["both_sides_transactions"], 1);
    assert_eq!(fp["received_sats"], "69");
    assert_eq!(fp["spent_sats"], "60");
    assert_eq!(fp["net_flow_sats"], "9");
    assert_eq!(fp["fees_of_involved_transactions_sats"], "2");
    assert_eq!(fp["spending_transaction_fees_sats"], "1");
    assert!(fp["wallet_attributed_fee_sats"].is_null());
    assert_eq!(fp["first_seen_unix_ms"], 1704153600000u64);
    assert_eq!(fp["last_seen_unix_ms"], 1704240000000u64);
    assert_eq!(fp["active_days"], 2);
    assert_eq!(fp["maximum_transaction_outputs"], 2);
    assert_eq!(fp["unique_counterparties"], 2);
    assert_eq!(fp["upstream_subjects"], 1);
    assert_eq!(fp["downstream_subjects"], 1);
    assert_eq!(full["activity"].as_array().unwrap().len(), 2);
    assert_eq!(full["activity"][0]["net_flow_sats"], "-51");
    assert!(
        full["top_counterparties"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["attributed_amount_sats"].is_null() && p["ownership_claimed"] == false)
    );
    assert!(
        full["top_counterparties"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["subject_type"] == "script"
                && p["address"].is_null()
                && p["downstream_transactions"] == 1)
    );
    let limited = activity::read(
        &db,
        &address(4),
        &Query {
            limit: 1,
            activity_limit: 1,
            counterparty_limit: 1,
            ..Default::default()
        },
    )
    .await?;
    for key in [
        "activity_truncated",
        "transactions_truncated",
        "counterparties_truncated",
    ] {
        assert_eq!(limited["activity_coverage"][key], true);
    }
    assert_eq!(
        limited["fingerprint"]["transaction_count"], 2,
        "display limits do not truncate summary"
    );
    assert_eq!(limited["fingerprint"]["unique_counterparties"], 2);
    let query = Query {
        from_block: Some(2),
        to_block: Some(2),
        direction: Direction::Spent,
        ..Default::default()
    };
    let scoped = api::investigation_with_query(&db, &address(4), &query).await?;
    assert_eq!(scoped["fingerprint"]["transaction_count"], 1);
    assert_eq!(scoped["fingerprint"]["net_flow_sats"], "-51");
    assert_eq!(
        scoped["holdings"]["balance_sats"], "9",
        "analytics net flow is not holdings"
    );
    assert_eq!(
        scoped["query_scope"]["analytics"]["filters"]["from_block"],
        2
    );
    assert_eq!(
        scoped["query_scope"]["holdings"]["analytics_filters_applied"],
        false
    );
    assert_eq!(scoped["risk_engine"]["risk_level"], "UNKNOWN");
    let saved = bitcoin_aml::risk::inspect(
        &db,
        &address(4),
        scoped["risk_snapshot"]["assessment_id"].as_str().unwrap(),
    )
    .await?;
    let evidence: Value =
        serde_json::from_str(saved["assessment"]["evidence_json"].as_str().unwrap())?;
    assert_eq!(
        evidence["inputs"]["fingerprint"]["query_scope"]["filters"]["from_block"],
        2
    );
    let by_time = activity::read(
        &db,
        &address(4),
        &Query {
            from_unix_ms: Some(1704240000000),
            to_unix_ms: Some(1704240000000),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(by_time["fingerprint"]["transaction_count"], 1);
    let both = activity::read(
        &db,
        &address(4),
        &Query {
            direction: Direction::Both,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(both["fingerprint"]["transaction_count"], 1);
    let received = activity::read(
        &db,
        &address(4),
        &Query {
            direction: Direction::Received,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        received["fingerprint"]["transaction_count"], 2,
        "received includes transactions with both wallet sides"
    );
    let no_matches = activity::read(
        &db,
        &address(4),
        &Query {
            from_block: Some(3),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(no_matches["fingerprint"]["transaction_count"], 0);
    assert!(no_matches["fingerprint"]["first_seen_unix_ms"].is_null());
    assert!(
        no_matches["top_counterparties"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let future = activity::read(
        &db,
        &address(4),
        &Query {
            from_block: Some(100),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        future["query_scope"]["analytics"]["requested_block_bounds_ingested"],
        false
    );
    let seed = activity::read(&db, &address(1), &Default::default()).await?;
    assert_eq!(seed["fingerprint"]["transaction_count"], 6);
    assert_eq!(
        seed["fingerprint"]["fees_of_involved_transactions_sats"],
        "14"
    );
    assert_eq!(
        seed["fingerprint"]["spending_transaction_fees_sats"], "4",
        "multiple wallet inputs do not multiply a transaction fee"
    );
    assert_eq!(seed["fingerprint"]["received_sats"], "450");
    assert_eq!(seed["fingerprint"]["spent_sats"], "360");
    let coinbase = activity::read(&db, &address(15), &Default::default()).await?;
    assert_eq!(coinbase["fingerprint"]["coinbase_transactions"], 3);
    assert_eq!(coinbase["fingerprint"]["maximum_transaction_inputs"], 0);
    assert_eq!(
        coinbase["fingerprint"]["fees_of_involved_transactions_sats"],
        "0"
    );
    let exact: Vec<Value> = db
        .rows(
            "SELECT toString(sum(toUInt256(2100000000000000))) AS sats FROM numbers(10000)",
            &[],
        )
        .await?;
    assert_eq!(exact[0]["sats"], "21000000000000000000");
    // A physically inserted orphan cannot alter published canonical statistics.
    db.execute(&format!("INSERT INTO aml_bitcoin_outputs SELECT * REPLACE(toUInt64(99) AS block_number,'unpublished-activity' AS commit_id,toUInt64(999) AS revision) FROM aml_bitcoin_outputs_canonical WHERE address='{}' LIMIT 1",address(4))).await?;
    assert_eq!(
        activity::read(&db, &address(4), &Default::default()).await?["fingerprint"],
        full["fingerprint"]
    );
    let router = api::router(api::Api::new(db.clone(), "a".repeat(40))?);
    for suffix in [
        "?from_block=2&to_block=1",
        "?direction=invalid",
        "?activity_limit=367",
        "?unknown=1",
    ] {
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!(
                        "/api/bitcoin/wallet/{}/activity{suffix}",
                        address(4)
                    ))
                    .header("X-AML-Service-Key", "a".repeat(40))
                    .body(axum::body::Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }
    let response = router
        .oneshot(
            axum::http::Request::builder()
                .uri(format!(
                    "/api/bitcoin/wallet/{}/activity?from_block=2&to_block=2",
                    address(4)
                ))
                .header("X-AML-Service-Key", "a".repeat(40))
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert!(response.status().is_success());
    assert_eq!(response.headers()["Cache-Control"], "no-store");
    let body: Value =
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1024 * 1024).await?)?;
    assert_eq!(body["fingerprint"]["net_flow_sats"], "-51");
    if let Ok(file) = std::env::var("BITCOIN_ACTIVITY_ACCEPTANCE") {
        std::fs::write(
            file,
            serde_json::to_vec_pretty(
                &json!({"scope":"synthetic canonical activity acceptance","full":full,"scoped":scoped,"limited":limited,"coinbase":coinbase,"seed":seed}),
            )?,
        )?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8"]
async fn activity_repeated_txids_branch_rollback_and_partial_history() -> Result<()> {
    use bitcoin_aml::activity;
    async fn append(
        db: &Database,
        block: Value,
        resolved: &mut HashMap<String, model::ScriptOutput>,
        start: u64,
    ) -> Result<()> {
        let height = block["height"].as_u64().unwrap();
        let commit = format!("activity-occurrence-{}", block["hash"].as_str().unwrap());
        let facts = model::extract(&block, &commit, resolved)?;
        let source = LegacyBlock {
            block_height: height,
            block_hash: block["hash"].as_str().unwrap().into(),
            parent_hash: block["previousblockhash"].as_str().unwrap().into(),
            block_time: block["time"].as_u64().unwrap(),
            commit_id: commit,
            transaction_count: facts.transactions.len() as u32,
        };
        writer::write_block(
            db,
            &db.state().await?,
            &source,
            &facts,
            WriteContext {
                observed: height,
                start,
                source_id: "synthetic-activity-occurrences",
                legacy_fence: None,
            },
        )
        .await?;
        for out in facts.outputs {
            resolved.insert(model::outpoint(&out.txid, out.output_index), out.output);
        }
        Ok(())
    }
    let config = |name: &str| -> Result<DatabaseConfig> {
        Ok(DatabaseConfig {
            url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
            user: "bitcoin_test".into(),
            password: "bitcoin-test-only".into(),
            database: name.into(),
        })
    };
    let db = Database::new(config("bitcoin_activity_occurrences")?)?;
    db.initialize().await?;
    assert_eq!(db.state().await?.revision, 0, "fresh fixture only");
    let duplicate = "d".repeat(64);
    let first = format!("{:064x}", 1001);
    let second = format!("{:064x}", 1002);
    let mut resolved = HashMap::new();
    append(&db,json!({"height":0,"hash":GENESIS,"previousblockhash":"","time":1704067200,"tx":[{"txid":duplicate,"vin":[{"coinbase":"00"}],"vout":[output(0,100,4)]}]}),&mut resolved,0).await?;
    append(&db,json!({"height":1,"hash":first,"previousblockhash":GENESIS,"time":1704153600,"tx":[{"txid":"c".repeat(64),"vin":[{"coinbase":"00"}],"vout":[output(0,30,5)]},spend('e',vec![(duplicate.clone(),0)],vec![(5,99)])]}),&mut resolved,0).await?;
    append(&db,json!({"height":2,"hash":second,"previousblockhash":first,"time":1704240000,"tx":[{"txid":duplicate,"vin":[{"coinbase":"00"}],"vout":[output(0,7,4)]}]}),&mut resolved,0).await?;
    let all = activity::read(&db, &address(4), &Default::default()).await?;
    assert_eq!(all["fingerprint"]["transaction_count"], 3);
    assert_eq!(all["fingerprint"]["received_sats"], "107");
    assert_eq!(all["fingerprint"]["spent_sats"], "100");
    assert_eq!(all["fingerprint"]["net_flow_sats"], "7");
    let repeated = all["recent_transactions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["tx_hash"] == duplicate)
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2);
    assert_ne!(repeated[0]["occurrence_id"], repeated[1]["occurrence_id"]);
    let previous = db.state().await?;
    let mut rolled = previous.clone();
    rolled.revision = db.next_revision(previous.revision).await?;
    rolled.tip_height = 0;
    rolled.tip_hash = GENESIS.into();
    db.publish_after(&previous, &rolled).await?;
    let after = activity::read(&db, &address(4), &Default::default()).await?;
    assert_eq!(after["fingerprint"]["transaction_count"], 1);
    assert_eq!(after["fingerprint"]["net_flow_sats"], "100");
    assert_ne!(
        after["source_snapshot"]["snapshot_id"],
        all["source_snapshot"]["snapshot_id"]
    );
    append(&db,json!({"height":1,"hash":"f".repeat(64),"previousblockhash":GENESIS,"time":1704153600,"tx":[{"txid":"a".repeat(64),"vin":[{"coinbase":"00"}],"vout":[output(0,30,5)]}]}),&mut resolved,0).await?;
    assert_eq!(
        activity::read(&db, &address(4), &Default::default()).await?["fingerprint"]["transaction_count"],
        1,
        "orphaned old branch must not reappear"
    );
    let partial = Database::new(config("bitcoin_activity_partial")?)?;
    partial.initialize().await?;
    append(&partial,json!({"height":10,"hash":format!("{:064x}",10),"previousblockhash":format!("{:064x}",9),"time":1704067200,"tx":[{"txid":"b".repeat(64),"vin":[{"coinbase":"00"}],"vout":[output(0,1,4)]}]}),&mut HashMap::new(),10).await?;
    let scope = activity::read(&partial, &address(4), &Default::default()).await?;
    assert_eq!(
        scope["query_scope"]["analytics"]["history_from_genesis"],
        false
    );
    assert_eq!(scope["source_snapshot"]["start_block"], 10);
    assert_eq!(scope["fingerprint"]["received_sats"], "1");
    assert_eq!(scope["fingerprint"]["received_btc"], "0.00000001");
    Ok(())
}
// Explicit fault injection only into this test's fixed disposable database.
async fn audit_insert(table: &str, rows: &[Value]) -> Result<()> {
    anyhow::ensure!(
        [
            "aml_wallet_risk_assessments",
            "aml_wallet_risk_signals",
            "aml_risk_policy_rules"
        ]
        .contains(&table)
    );
    let body = rows
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n");
    let response = reqwest::Client::new()
        .post(std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?)
        .query(&[
            ("query", format!("INSERT INTO {table} FORMAT JSONEachRow")),
            ("database", "bitcoin_risk_test".into()),
        ])
        .header("X-ClickHouse-User", "bitcoin_test")
        .header("X-ClickHouse-Key", "bitcoin-test-only")
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    anyhow::ensure!(status.is_success(), "fixture insert: {text}");
    Ok(())
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8"]
async fn risk_policy_assessments_signals_seals_and_freshness() -> Result<()> {
    use bitcoin_aml::db;
    use bitcoin_aml::risk;
    use tower::ServiceExt;
    let config = DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_risk_test".into(),
    };
    let db = Database::new(config.clone())?;
    db.initialize().await?;
    assert_eq!(db.state().await?.revision, 0, "fresh fixture only");
    let waiting = api::investigation(&db, &address(4), 40).await?;
    assert_eq!(waiting["risk_engine"]["risk_level"], "UNKNOWN");
    assert!(waiting["risk_snapshot"].is_null());
    assert_eq!(waiting["risk_persistence"]["persisted"], false);
    fixtures(&db).await?;
    let unknown = api::investigation(&db, &address(4), 40).await?;
    let unknown_id = unknown["risk_snapshot"]["assessment_id"].as_str().unwrap();
    let stored_unknown = risk::inspect(&db, &address(4), unknown_id).await?;
    assert_eq!(stored_unknown["current"], true);
    assert_eq!(stored_unknown["assessment"]["signal_count"], 0);
    assert!(stored_unknown["assessment"]["risk_score"].is_null());
    assert_eq!(stored_unknown["risk_engine"]["risk_level"], "UNKNOWN");
    let registered = intelligence::register(&db, source(), false).await?;
    let pending = intelligence::import(&db, vec![claim("risk-seed", 1, true)], false).await?;
    let approved = review(
        &db,
        "risk-seed",
        pending["claims"][0]["revision"].clone(),
        "approved",
    )
    .await?;
    assert_eq!(
        risk::inspect(&db, &address(4), unknown_id).await?["current"],
        false,
        "new seed invalidates previously negative assessment"
    );
    let direct = api::investigation(&db, &address(4), 40).await?;
    let id = direct["risk_snapshot"]["assessment_id"].as_str().unwrap();
    let stored = risk::inspect(&db, &address(4), id).await?;
    assert_eq!(stored["current"], true);
    assert_eq!(
        stored["risk_engine"],
        aml_risk_policy::assess("bitcoin", &direct)
    );
    assert!(stored["risk_engine"]["risk_score"].as_f64().unwrap() > 0.0);
    assert_eq!(stored["policy"]["rules"].as_array().unwrap().len(), 6);
    assert_eq!(
        stored["policy"]["policy"]["policy_version"],
        aml_risk_policy::POLICY
    );
    assert_eq!(stored["signals"][0]["signal_type"], "ILLICIT_SEED_EXPOSURE");
    assert!(
        stored["signals"][0]["evidence_refs"]
            .as_array()
            .unwrap()
            .len()
            > 0
    );
    assert!(risk::inspect(&db, &address(5), id).await.is_err());
    let restarted = Database::new(config)?;
    assert_eq!(
        risk::inspect(&restarted, &address(4), id).await?["risk_snapshot"],
        direct["risk_snapshot"]
    );
    let (one, two) = tokio::join!(risk::record(&db, &direct), risk::record(&db, &direct));
    let (one, two) = (one?, two?);
    assert_ne!(
        one["risk_snapshot"]["assessment_id"],
        two["risk_snapshot"]["assessment_id"]
    );
    assert_eq!(one["risk_engine"], two["risk_engine"]);
    let labelled = api::investigation(&db, &address(1), 40).await?;
    assert!(
        labelled["risk_engine"]["signals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["signal_type"] == "APPROVED_RISK_LABEL"
                && s["contribution"].as_f64() == Some(70.0))
    );
    let mixed = api::investigation(&db, &address(6), 40).await?;
    assert!(mixed["risk_engine"]["risk_score"].is_null());
    let router = api::router(api::Api::new(db.clone(), "r".repeat(40))?);
    let route = format!("/api/bitcoin/wallet/{}/risk/{id}", address(4));
    let unauth = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(&route)
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(unauth.status(), axum::http::StatusCode::UNAUTHORIZED);
    let read = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("{route}/snapshot"))
                .header("X-AML-Service-Key", "r".repeat(40))
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert!(read.status().is_success());
    assert_eq!(read.headers()["Cache-Control"], "no-store");
    let read: Value =
        serde_json::from_slice(&axum::body::to_bytes(read.into_body(), 65536).await?)?;
    assert_eq!(read["risk_snapshot"], direct["risk_snapshot"]);
    let post = router
        .oneshot(
            axum::http::Request::builder()
                .uri(&route)
                .method("POST")
                .header("X-AML-Service-Key", "r".repeat(40))
                .body(axum::body::Body::from("{\"risk_score\":100}"))?,
        )
        .await?;
    assert_eq!(post.status(), axum::http::StatusCode::METHOD_NOT_ALLOWED);
    // A complete-looking parent and its signal rows without a seal remain audit-only.
    let partial_id = format!("btc-risk-{}", "f".repeat(64));
    let mut parent = stored["assessment"].clone();
    let mut blob: Value = serde_json::from_str(parent["evidence_json"].as_str().unwrap())?;
    parent["assessment_id"] = json!(partial_id);
    blob["snapshot"]["assessment_id"] = json!(partial_id);
    let mut body = blob["snapshot"].clone();
    body.as_object_mut().unwrap().remove("current");
    body.as_object_mut().unwrap().remove("snapshot_id");
    blob["snapshot"]["snapshot_id"] = json!(aml_risk_policy::digest(&body));
    parent["evidence_json"] = json!(blob.to_string());
    let mut signals = stored["signals"].as_array().unwrap().clone();
    for (i, s) in signals.iter_mut().enumerate() {
        s["assessment_id"] = json!(partial_id);
        s["signal_id"] = json!(aml_risk_policy::digest(&json!([
            partial_id,
            i,
            direct["risk_engine"]["signals"][i]
        ])));
    }
    audit_insert("aml_wallet_risk_assessments", &[parent]).await?;
    audit_insert("aml_wallet_risk_signals", &signals).await?;
    let partial = risk::inspect(&db, &address(4), &partial_id).await?;
    assert_eq!(partial["current"], false);
    assert!(partial["risk_engine"]["risk_score"].is_null());
    let mut inactive = source();
    inactive.is_active = false;
    inactive.expected_revision = Some(registered["revision"].as_str().unwrap().into());
    let disabled = intelligence::register(&db, inactive, false).await?;
    let stale = risk::inspect(&db, &address(4), id).await?;
    assert_eq!(stale["current"], false);
    assert_eq!(stale["historical_risk_engine"], direct["risk_engine"]);
    let mut active = source();
    active.expected_revision = Some(disabled["revision"].as_str().unwrap().into());
    intelligence::register(&db, active, false).await?;
    let before_revoke = api::investigation(&db, &address(4), 40).await?;
    review(&db, "risk-seed", approved["revision"].clone(), "revoked").await?;
    let revoked = risk::inspect(
        &db,
        &address(4),
        before_revoke["risk_snapshot"]["assessment_id"]
            .as_str()
            .unwrap(),
    )
    .await?;
    assert_eq!(revoked["current"], false);
    let after = api::investigation(&db, &address(4), 40).await?;
    let mut state = db.state().await?;
    state.revision = db.next_revision(state.revision).await?;
    state.observed_rpc_height += 1;
    db.publish(&state).await?;
    assert_eq!(
        risk::inspect(
            &db,
            &address(4),
            after["risk_snapshot"]["assessment_id"].as_str().unwrap()
        )
        .await?["current"],
        false
    );
    let fresh = api::investigation(&db, &address(4), 40).await?;
    let registry = risk::read_policy(&db, aml_risk_policy::POLICY).await?;
    let mut changed = registry["rules"][0].clone();
    changed["max_contribution"] = json!(69.0);
    changed["revision"] = json!(db::timestamp() + 1);
    audit_insert("aml_risk_policy_rules", &[changed]).await?;
    assert_eq!(
        risk::inspect(
            &db,
            &address(4),
            fresh["risk_snapshot"]["assessment_id"].as_str().unwrap()
        )
        .await?["current"],
        false
    );
    assert!(
        risk::record(&db, &fresh).await.is_err(),
        "conflicting policy cannot silently overwrite a version"
    );
    if let Ok(file) = std::env::var("BITCOIN_RISK_ACCEPTANCE") {
        std::fs::write(
            file,
            serde_json::to_vec_pretty(
                &json!({"scope":"synthetic ClickHouse risk acceptance","unknown":unknown,"direct":direct,"labelled":labelled,"mixed":mixed,"stored":stored,"partial":partial,"stale":stale,"revoked":revoked}),
            )?,
        )?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires fresh disposable ClickHouse 23.8"]
async fn exact_outpoint_bounds_publication_freshness_and_investigation() -> Result<()> {
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_exposure_test".into(),
    })?;
    db.initialize().await?;
    assert_eq!(db.state().await?.revision, 0, "fresh fixture only");
    fixtures(&db).await?;
    let registered = intelligence::register(&db, source(), false).await?;
    let pending = intelligence::import(
        &db,
        vec![claim("risk-seed", 1, true), claim("service", 8, false)],
        false,
    )
    .await?;
    let none = exposure::compute(&db, &address(4), Options::default()).await?;
    assert!(none.paths.is_empty());
    let risk = review(
        &db,
        "risk-seed",
        pending["claims"][0]["revision"].clone(),
        "approved",
    )
    .await?;
    review(
        &db,
        "service",
        pending["claims"][1]["revision"].clone(),
        "approved",
    )
    .await?;
    assert_eq!(
        exposure::inspect(&db, &address(4), none.run_id.as_ref().unwrap()).await?["current"],
        false,
        "negative attribution dependencies must invalidate on a new seed"
    );
    let direct = api::investigation(&db, &address(4), 40).await?;
    assert!(
        incoming(&direct)
            .iter()
            .any(|p| p["allocation"]["minimum_sats"] == "60"
                && p["allocation"]["maximum_sats"] == "60"
                && p["score_eligible"] == true)
    );
    let run = direct["exposure_snapshot"]["run_id"].as_str().unwrap();
    assert_eq!(
        exposure::inspect(&db, &address(4), run).await?["current"],
        true
    );
    let mixed = api::investigation(&db, &address(6), 40).await?;
    assert!(
        incoming(&mixed)
            .iter()
            .any(|p| p["allocation"]["minimum_sats"] == "0"
                && p["allocation"]["maximum_sats"] == "99"
                && p["score_eligible"] == false)
    );
    let indirect = api::investigation(&db, &address(7), 40).await?;
    assert!(
        incoming(&indirect)
            .iter()
            .any(|p| p["hop_count"] == 2 && p["allocation"]["minimum_sats"] == "38")
    );
    let script = api::investigation(&db, &address(13), 40).await?;
    assert!(incoming(&script).iter().any(|p| {
        p["hop_count"] == 3
            && p["allocation"]["minimum_sats"] == "49"
            && p["path_subjects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s.as_str().unwrap().starts_with("script:"))
    }));
    let unrelated = api::investigation(&db, &address(10), 40).await?;
    assert!(
        incoming(&unrelated).is_empty(),
        "an unrelated UTXO at a reused address cannot bridge the seed's output"
    );
    let outgoing = api::investigation(&db, &address(5), 40).await?;
    assert!(
        outgoing["exposure_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["direction"] == "SENT_TO_SEED" && p["allocation"]["minimum_sats"] == "90")
    );
    let service = api::investigation(&db, &address(8), 40).await?;
    assert!(
        service["exposure_paths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["service_mediated"] == true && p["score_eligible"] == false)
    );
    let behind_service = api::investigation(&db, &address(11), 40).await?;
    assert!(incoming(&behind_service).is_empty());
    assert!(
        behind_service["exposure_coverage"]["service_boundaries"]
            .as_u64()
            .unwrap()
            > 0
    );
    let grouped = api::investigation(&db, &address(12), 40).await?;
    let grouped_paths = incoming(&grouped);
    assert_eq!(grouped_paths.len(), 1);
    assert_eq!(grouped_paths[0]["allocation"]["minimum_sats"], "99");
    assert_eq!(
        grouped_paths[0]["hops"][0]["inputs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let capped = exposure::compute(
        &db,
        &address(4),
        Options {
            max_transactions: 1,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(capped.coverage["truncated"], true);
    assert!(capped.snapshot.is_null());
    assert!(capped.paths.iter().all(|p| p["score_eligible"] == false));
    let cap = exposure::inspect(&db, &address(4), capped.run_id.as_ref().unwrap()).await?;
    assert_eq!(cap["current"], false);
    assert_eq!(cap["run_status"], "truncated");
    let subjects = vec![address(1), address(4), address(8), address(10)];
    let batch = intelligence::read_many(&db, &subjects).await?;
    for subject in subjects {
        assert_eq!(
            batch[&subject].snapshot,
            intelligence::read(&db, &subject).await?.snapshot
        );
    }
    let mut disabled = source();
    disabled.is_active = false;
    disabled.expected_revision = Some(registered["revision"].as_str().unwrap().into());
    let disabled = intelligence::register(&db, disabled, false).await?;
    let stale = exposure::inspect(&db, &address(4), run).await?;
    assert_eq!(stale["current"], false);
    assert!(
        stale["exposure_paths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["score_eligible"] == false)
    );
    let mut enabled = source();
    enabled.expected_revision = Some(disabled["revision"].as_str().unwrap().into());
    intelligence::register(&db, enabled, false).await?;
    let fresh = api::investigation(&db, &address(4), 40).await?;
    review(&db, "risk-seed", risk["revision"].clone(), "revoked").await?;
    let revoked = exposure::inspect(
        &db,
        &address(4),
        fresh["exposure_snapshot"]["run_id"].as_str().unwrap(),
    )
    .await?;
    assert_eq!(revoked["current"], false);
    let after = api::investigation(&db, &address(4), 40).await?;
    assert!(after["exposure_paths"].as_array().unwrap().is_empty());
    let mut state = db.state().await?;
    state.revision = db.next_revision(state.revision).await?;
    state.observed_rpc_height += 1;
    db.publish(&state).await?;
    let changed = exposure::inspect(
        &db,
        &address(4),
        after["exposure_snapshot"]["run_id"].as_str().unwrap(),
    )
    .await?;
    assert_eq!(changed["current"], false);
    if let Ok(file) = std::env::var("BITCOIN_EXPOSURE_ACCEPTANCE") {
        std::fs::write(
            file,
            serde_json::to_vec_pretty(
                &json!({"scope":"synthetic canonical ClickHouse UTXO acceptance","direct":direct,"mixed":mixed,"indirect":indirect,"script":script,"unrelated":unrelated,"outgoing":outgoing,"service":service,"behind_service":behind_service,"grouped":grouped,"truncated":capped.value(),"source_inactive":stale,"revoked":revoked,"after_revocation":after,"source_changed":changed}),
            )?,
        )?;
    }
    Ok(())
}
