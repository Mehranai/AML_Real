//! Disposable DB only: ETHEREUM_TEST_CLICKHOUSE_URL=http://... cargo test --test shared_pipeline -- --ignored
//! All RPC/attribution data are explicit synthetic fixtures; no deployment secrets.
use anyhow::{Context, Result, ensure};
use axum::{Json, Router, extract::State, routing::post};
use ethereum_aml::{
    clustering,
    config::{AppConfig, FinalityTag, TraceMode},
    db::{self},
    ethereum::{IngestionService, run_token_metadata_worker},
    exposure::{self, ExposureOptions},
    intelligence::{self, EntityLabelImportOptions, IntelligenceSourceRegistration},
    risk::EvidenceRiskEngine,
};
use serde_json::{Value, json};
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

const A: &str = "0x1111111111111111111111111111111111111111";
const B: &str = "0x2222222222222222222222222222222222222222";
const C: &str = "0x3333333333333333333333333333333333333333";
const T: &str = "0x4444444444444444444444444444444444444444";
const NFT: &str = "0x5555555555555555555555555555555555555555";
const KEY: &str = "ethereum-fixture-service-key-0123456789abcdef";
fn hash(n: u64) -> String {
    format!("0x{n:064x}")
}
fn topic(address: &str) -> String {
    format!("0x{}{}", "0".repeat(24), address.trim_start_matches("0x"))
}
#[derive(Clone)]
struct Chain {
    head: u64,
    blocks: Vec<Value>,
    receipts: Vec<Vec<Value>>,
    traces: Vec<Vec<Value>>,
}
fn tx(height: u64, index: u64, from: &str, to: &str, value: u64) -> Value {
    json!({"hash":hash(height*10+index),"nonce":format!("0x{:x}",height+index),"blockHash":hash(height),"blockNumber":format!("0x{height:x}"),"transactionIndex":format!("0x{index:x}"),"from":from,"to":to,"value":format!("0x{value:x}"),"gasPrice":"0x1","gas":"0x186a0","input":"0x","v":"0x25","r":"0x1","s":"0x1","type":"0x0","chainId":"0x1"})
}
fn receipt(
    height: u64,
    index: u64,
    from: &str,
    to: &str,
    success: bool,
    logs: Vec<Value>,
) -> Value {
    json!({"transactionHash":hash(height*10+index),"transactionIndex":format!("0x{index:x}"),"blockHash":hash(height),"blockNumber":format!("0x{height:x}"),"from":from,"to":to,"contractAddress":null,"cumulativeGasUsed":format!("0x{:x}",21000*(index+1)),"gasUsed":"0x5208","effectiveGasPrice":"0x1","status":if success{"0x1"}else{"0x0"},"type":"0x0","logs":logs,"logsBloom":format!("0x{}","0".repeat(512))})
}
fn log(contract: &str, height: u64, index: u64, nft: bool) -> Value {
    let mut topics = vec![
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".to_owned(),
        topic(A),
        topic(B),
    ];
    if nft {
        topics.push(format!("0x{}", "f".repeat(64)));
    }
    json!({"address":contract,"topics":topics,"data":if nft{"0x".to_owned()}else{format!("0x{}","f".repeat(64))},"blockNumber":format!("0x{height:x}"),"blockHash":hash(height),"transactionHash":hash(height*10),"transactionIndex":"0x0","logIndex":format!("0x{index:x}"),"removed":false})
}
fn frame(from: &str, to: &str, value: u64) -> Value {
    json!({"type":"CALL","from":from,"to":to,"value":format!("0x{value:x}"),"gas":"0x186a0","gasUsed":"0x5208","input":"0x","output":"0x"})
}
fn fixture() -> Result<Chain> {
    let mut blocks = vec![];
    let mut receipts = vec![];
    let mut traces = vec![];
    let timestamp = db::now_ms() / 1000;
    for h in [7u64, 8] {
        let empty: alloy::rpc::types::Block = Default::default();
        let mut block = serde_json::to_value(empty)?;
        block["hash"] = json!(hash(h));
        block["parentHash"] = json!(hash(h - 1));
        block["number"] = json!(format!("0x{h:x}"));
        block["timestamp"] = json!(format!("0x{:x}", timestamp + h - 7));
        if h == 7 {
            block["transactions"] = json!([tx(h, 0, A, B, 11), tx(h, 1, A, C, 5)]);
            block["withdrawals"] = json!([{"index":"0xffffffffffffffff","validatorIndex":"0xffffffffffffffff","address":B,"amount":"0x1"}]);
            receipts.push(vec![
                receipt(
                    h,
                    0,
                    A,
                    B,
                    true,
                    vec![log(T, h, 0, false), log(NFT, h, 1, true)],
                ),
                receipt(h, 1, A, C, false, vec![]),
            ]);
            let mut success = frame(A, B, 11);
            success["calls"] = json!([frame(B, C, 1)]);
            let mut failed = frame(A, C, 5);
            failed["error"] = json!("execution reverted");
            traces.push(vec![
                json!({"txHash":hash(70),"result":success}),
                json!({"txHash":hash(71),"result":failed}),
            ]);
        } else {
            block["transactions"] = json!([tx(h, 0, B, C, 3)]);
            block["withdrawals"] = json!([]);
            receipts.push(vec![receipt(h, 0, B, C, true, vec![])]);
            traces.push(vec![json!({"txHash":hash(80),"result":frame(B,C,3)})]);
        }
        // Fail fixture parsing early, before constructing or changing a database.
        let _: alloy::rpc::types::Block = serde_json::from_value(block.clone())?;
        for r in receipts.last().unwrap() {
            let _: alloy::rpc::types::TransactionReceipt = serde_json::from_value(r.clone())?;
        }
        blocks.push(block);
    }
    Ok(Chain {
        head: 7,
        blocks,
        receipts,
        traces,
    })
}
fn abi_string(s: &str) -> String {
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
async fn rpc(State(chain): State<Arc<RwLock<Chain>>>, Json(request): Json<Value>) -> Json<Value> {
    let reply = |r: &Value| {
        let chain = chain.read().unwrap();
        let method = r["method"].as_str().unwrap();
        let p = &r["params"];
        if matches!(method, "eth_getCode" | "eth_call" | "eth_getBalance")
            && !matches!(p[1].as_str(), Some("0x7" | "0x8"))
        {
            return json!({"id":r["id"],"jsonrpc":"2.0","error":{"code":-32602,"message":"fixture requires pinned state block"}});
        }
        let height = if let Some(s) = p[0].as_str() {
            if s.starts_with("0x") {
                u64::from_str_radix(&s[2..], 16).unwrap_or(chain.head)
            } else {
                chain.head
            }
        } else {
            chain.head
        };
        let i = if height == 8 { 1 } else { 0 };
        let value = match method {
            "eth_chainId" => json!("0x1"),
            "web3_clientVersion" => json!("synthetic-ethereum-fixture"),
            "eth_blockNumber" => json!(format!("0x{:x}", chain.head)),
            "eth_getBlockByNumber" => chain.blocks[i].clone(),
            "eth_getBlockReceipts" => json!(chain.receipts[i]),
            "debug_traceBlockByNumber" => json!(chain.traces[i]),
            "eth_getCode" => json!("0x6000"),
            "eth_getBalance" => json!("0x1000"),
            "eth_call" => {
                let input = p[0]
                    .get("input")
                    .or_else(|| p[0].get("data"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if input.starts_with("0x06fdde03") {
                    json!(abi_string("Fixture token"))
                } else if input.starts_with("0x95d89b41") {
                    json!(abi_string("TEST"))
                } else if input.starts_with("0x313ce567") {
                    json!(format!("0x{:064x}", 18))
                } else if input.starts_with("0x18160ddd") {
                    json!(format!("0x{}", "f".repeat(64)))
                } else if input.starts_with("0x6352211e") {
                    json!(topic(B))
                } else {
                    json!(format!("0x{:064x}", 100))
                }
            }
            _ => {
                return json!({"id":r["id"],"jsonrpc":"2.0","error":{"code":-32601,"message":format!("unexpected fixture method {method}")}});
            }
        };
        json!({"id":r["id"],"jsonrpc":"2.0","result":value})
    };
    Json(if let Some(batch) = request.as_array() {
        Value::Array(batch.iter().map(reply).collect())
    } else {
        reply(&request)
    })
}
fn config(url: String) -> Result<AppConfig> {
    Ok(AppConfig {
        eth_rpc_url: url,
        eth_expected_chain_id: 1,
        eth_network_id: "eip155:1".into(),
        eth_start_block: 7,
        eth_finality_tag: FinalityTag::Finalized,
        eth_poll_interval_seconds: 1,
        eth_ingestion_batch_max_rows: 100,
        eth_rpc_provider: "synthetic-fixture".into(),
        eth_rpc_max_retries: 1,
        eth_rpc_retry_base_delay_ms: 1,
        eth_request_delay_ms: 0,
        eth_trace_mode: TraceMode::Required,
        eth_protocol_registry_refresh_blocks: 100,
        eth_token_metadata_poll_seconds: 1,
        eth_token_metadata_batch_size: 100,
        eth_token_metadata_max_attempts: 3,
        eth_risk_engine_enabled: true,
        eth_risk_policy_version: "ethereum_evidence_policy_v1".into(),
        eth_analytics_interval_seconds: 60,
        eth_exposure_max_hops: 5,
        eth_exposure_hop_decay: 0.65,
        eth_exposure_time_half_life_days: 365.0,
        eth_exposure_max_paths_per_subject: 3,
        clickhouse_url: std::env::var("ETHEREUM_TEST_CLICKHOUSE_URL")?,
        clickhouse_user: "warehouse_test".into(),
        clickhouse_password: "warehouse-test-only".into(),
        clickhouse_database: "ethereum_aml_db2".into(),
        neo4j_uri: String::new(),
        neo4j_username: String::new(),
        neo4j_password: String::new(),
        ethereum_api_addr: "127.0.0.1:0".into(),
        eth_graph_max_edges: 1000,
        rpc_timeout_seconds: 20,
    })
}
async fn response(app: Router, path: &str) -> Result<Value> {
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri(path)
                .header("x-aml-service-key", KEY)
                .body(axum::body::Body::empty())?,
        )
        .await?;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024).await?;
    ensure!(
        status.is_success(),
        "API {path}: {status}: {}",
        String::from_utf8_lossy(&body)
    );
    Ok(serde_json::from_slice(&body)?)
}
#[tokio::test]
#[ignore = "requires a new disposable ClickHouse database and fixture service-key environment"]
async fn rpc_to_common_storage_workers_and_investigation() -> Result<()> {
    ethereum_aml::init_tracing();
    let chain = Arc::new(RwLock::new(fixture()?));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let router = Router::new()
        .route("/", post(rpc))
        .with_state(chain.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut cfg = config(endpoint)?;
    cfg.eth_trace_mode = TraceMode::Disabled;
    let admin = clickhouse::Client::default()
        .with_url(&cfg.clickhouse_url)
        .with_user(&cfg.clickhouse_user)
        .with_password(&cfg.clickhouse_password);
    let exists = admin
        .query("SELECT count() FROM system.databases WHERE name=?")
        .bind(&cfg.clickhouse_database)
        .fetch_one::<u64>()
        .await?;
    ensure!(exists == 0, "test requires a brand-new database");
    db::initialize_ethereum_schema(&cfg)
        .await
        .context("initial schema")?;
    db::initialize_ethereum_schema(&cfg)
        .await
        .context("idempotent schema")?;
    db::check_ethereum_schema(&cfg).await?;
    eprintln!("DB-2: shared schema ready");
    let client = db::database_client(&cfg);
    let foreign=client.query("SELECT name FROM system.tables WHERE database=currentDatabase() AND NOT startsWith(name,'aml_')").fetch_all::<String>().await?;
    assert!(foreign.is_empty(), "unexpected native objects: {foreign:?}");
    let service = IngestionService::connect(cfg.clone()).await?;
    assert!(
        IngestionService::connect(cfg.clone()).await.is_err(),
        "a competing ingestor must not acquire the same cursor"
    );
    service
        .ingest_range(7, 7)
        .await
        .context("first block ingest")?;
    eprintln!("DB-2: live RPC block stored");
    let state = aml_warehouse::publication::state(
        &client,
        client.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await?;
    assert_eq!(state.last_block_number, 7);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_transactions_canonical")
            .fetch_one::<u64>()
            .await?,
        2
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_address_relationships_canonical WHERE tx_hash=?")
            .bind(hash(71))
            .fetch_one::<u64>()
            .await?,
        0,
        "failed transaction must not emit a transfer"
    );
    let max = client
        .query(
            "SELECT toString(amount_raw) FROM aml_address_relationships_canonical WHERE asset_id=?",
        )
        .bind(format!("erc20:{T}"))
        .fetch_one::<String>()
        .await?;
    assert_eq!(max, aml_warehouse::RawAmount::MAX);
    let nft=client.query("SELECT ifNull(toString(token_id),'') FROM aml_address_relationships_canonical WHERE startsWith(asset_id,'erc721:')").fetch_one::<String>().await?;
    assert_eq!(nft, aml_warehouse::RawAmount::MAX);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_evm_traces_canonical")
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert_eq!(
        client
            .query(
                "SELECT count() FROM aml_raw_transactions_canonical WHERE isNotNull(receipt_json)"
            )
            .fetch_one::<u64>()
            .await?,
        2
    );
    run_token_metadata_worker(cfg.clone(), true)
        .await
        .context("metadata worker")?;
    eprintln!("DB-2: metadata worker done");
    assert_eq!(
        client
            .query("SELECT count() FROM aml_asset_metadata_visible")
            .fetch_one::<u64>()
            .await?,
        2
    );
    let app = ethereum_aml::api::build_router(cfg.clone()).await?;
    let wallet = response(
        app.clone(),
        &format!("/api/ethereum/wallet/{B}/investigation"),
    )
    .await?;
    assert_eq!(
        wallet["block_credits"][0]["withdrawal_index"],
        u64::MAX.to_string()
    );
    assert!(wallet["block_credits"][0]["tx_hash"].is_null());
    assert_eq!(wallet["block_credits"][0]["amount_raw"], "1000000000");
    assert_eq!(wallet["source_snapshot"]["as_of_block"], 7);
    eprintln!("DB-2: investigation API done");
    let holdings = response(app.clone(), &format!("/api/ethereum/wallet/{B}/holdings")).await?;
    assert_eq!(holdings["source"], "finalized_rpc");
    assert!(
        holdings["assets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["asset_id"] == "eip155:1/native:eth"
                && a["amount"] == "4096"
                && a["status"] == "available")
    );
    assert!(
        holdings["assets"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |a| a["token_id"] == alloy::primitives::U256::MAX.to_string() && a["amount"] == "1"
            )
    );

    drop(service);
    chain.write().unwrap().head = 8;
    let resumed = IngestionService::connect(cfg.clone()).await?;
    let replay = resumed.ingest_range(7, 8).await?;
    assert_eq!(replay.skipped_blocks, 1);
    assert_eq!(replay.completed_blocks, 1);
    drop(resumed);
    let before = aml_warehouse::publication::state(
        &client,
        client.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await?;
    cfg.eth_trace_mode = TraceMode::Required;
    let upgraded = IngestionService::connect(cfg.clone()).await?;
    upgraded
        .ingest_range(7, 8)
        .await
        .context("historical same-branch trace refresh")?;
    drop(upgraded);
    let after = aml_warehouse::publication::state(
        &client,
        client.database(),
        aml_warehouse::Network::Ethereum,
    )
    .await?;
    assert_eq!(after.last_block_number, 8);
    assert!(after.revision > before.revision);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_transactions_canonical")
            .fetch_one::<u64>()
            .await?,
        3
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_evm_traces_canonical")
            .fetch_one::<u64>()
            .await?,
        4
    );
    eprintln!("DB-2: restart and non-tip trace refresh done");
    reject_invalid_refresh(&client).await?;
    let status = response(app.clone(), "/status").await?;
    assert_eq!(status["transaction_count"], 3);
    assert_eq!(status["source_snapshot"]["as_of_block"], 8);
    let paths = response(
        app.clone(),
        &format!("/api/ethereum/wallet/{A}/paths/{C}?max_hops=3"),
    )
    .await?;
    assert!(
        !paths["paths"]
            .as_array()
            .context("missing graph paths")?
            .is_empty()
    );
    assert_eq!(paths["source_snapshot"]["as_of_block"], 8);

    let path = std::env::temp_dir().join("ethereum-db2-labels.csv");
    std::fs::write(
        &path,
        format!(
            "address,entity_id,entity_name,entity_type,address_role,confidence,risk_level,is_exposure_seed,seed_category,source_record_id,evidence_refs,review_status,reviewed_by,review_reason\n{A},fixture-illicit,Fixture illicit,illicit,HOT_WALLET,0.95,95,true,illicit,case-1,https://example.invalid/case-1,APPROVED,fixture-analyst,Reviewed synthetic fixture\n"
        ),
    )?;
    let options = EntityLabelImportOptions {
        source: IntelligenceSourceRegistration {
            source_id: "fixture_source".into(),
            source_name: "Synthetic labels".into(),
            source_type: "ANALYST".into(),
            trust_tier: "VERIFIED".into(),
            reference_url: "https://example.invalid/fixtures".into(),
            created_by: "fixture-analyst".into(),
        },
        submitted_by: "fixture-analyst".into(),
    };
    intelligence::import_entity_labels_csv(&cfg, &path, options.clone())
        .await
        .context("reviewed label import")?;
    eprintln!("DB-2: intelligence import done");
    let clusters = clustering::discover_address_clusters(&cfg)
        .await
        .context("clustering worker")?;
    eprintln!("DB-2: clustering done");
    assert_eq!(clusters.entity_memberships, 1);
    let exposed = exposure::propagate_exposure(&cfg, ExposureOptions::default())
        .await
        .context("exposure worker")?;
    eprintln!("DB-2: exposure done");
    assert!(exposed.path_count >= 2);
    assert!(
        client
            .query(
                "SELECT count() FROM aml_exposure_paths_visible WHERE subject_id=? AND hop_count=2"
            )
            .bind(C)
            .fetch_one::<u64>()
            .await?
            > 0
    );
    let risk = EvidenceRiskEngine::new(&cfg).assess(A).await?;
    assert!(risk.risk_score.unwrap_or_default() > 0.0);
    assert!(!risk.probability_claimed);
    assert!(
        risk.signals
            .iter()
            .any(|s| s.signal_type == "CONFIRMED_ILLICIT_SEED"
                && !s.evidence_refs.is_empty()
                && s.evidence_refs.iter().all(|r| r.starts_with("claim:")))
    );
    let indirect_risk = EvidenceRiskEngine::new(&cfg).assess(C).await?;
    assert!(indirect_risk.signals.iter().any(|s| {
        s.signal_type == "INDIRECT_EXPOSURE"
            && !s.evidence_refs.is_empty()
            && s.evidence_refs
                .iter()
                .all(|r| r.starts_with("relationship:"))
    }));

    let assessed = response(
        app.clone(),
        &format!("/api/ethereum/wallet/{A}/investigation"),
    )
    .await?;
    assert!(
        !assessed["entities"]
            .as_array()
            .context("missing entities")?
            .is_empty()
    );
    // A reviewed service is an endpoint, not a bridge for inferred fund continuity.
    std::fs::write(
        &path,
        format!(
            "address,entity_id,entity_name,entity_type,address_role,confidence,risk_level,is_exposure_seed,seed_category,source_record_id,evidence_refs,review_status,reviewed_by,review_reason\n{B},fixture-service,Fixture service,exchange,HOT_WALLET,0.99,0,false,,case-2,https://example.invalid/case-2,APPROVED,fixture-analyst,Reviewed synthetic service\n"
        ),
    )?;
    intelligence::import_entity_labels_csv(&cfg, &path, options).await?;
    clustering::discover_address_clusters(&cfg).await?;
    exposure::propagate_exposure(&cfg, ExposureOptions::default()).await?;
    assert_eq!(
        client
            .query(
                "SELECT count() FROM aml_exposure_paths_visible WHERE subject_id=? AND hop_count>1"
            )
            .bind(C)
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert!(client.query("SELECT count() FROM aml_exposure_paths_visible WHERE subject_id=? AND service_mediated=1")
        .bind(B).fetch_one::<u64>().await? > 0);
    EvidenceRiskEngine::new(&cfg).assess(A).await?;
    let claim = client
        .rows("aml_intelligence_claims", "claim_kind='label'")
        .await?
        .into_iter()
        .find(|c| c["address"] == A)
        .context("missing test claim")?;
    db::records::store(&client,"intelligence_reviews",json!({"review_id":"fixture-revoke","subject_id":claim["claim_id"],"decision":"REVOKED","reviewer":"fixture-analyst","reason":"Withdraw fixture claim","evidence_refs":["https://example.invalid/revoke"]})).await?;
    assert_eq!(
        client
            .query("SELECT count() FROM aml_exposure_paths_visible")
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_cluster_memberships_visible")
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_wallet_risk_assessments_visible")
            .fetch_one::<u64>()
            .await?,
        0
    );
    let foreign=client.query("SELECT name FROM system.tables WHERE database=currentDatabase() AND NOT startsWith(name,'aml_')").fetch_all::<String>().await?;
    assert!(foreign.is_empty());
    std::fs::remove_file(path)?;
    server.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "requires a new disposable ClickHouse database and fixture service-key environment"]
async fn failed_publication_retries_and_finalized_restart_conflicts_fail_closed() -> Result<()> {
    use aml_warehouse::{Connection, Network, publication};
    let chain = Arc::new(RwLock::new(fixture()?));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let mut cfg = config(format!("http://{}", listener.local_addr()?))?;
    cfg.clickhouse_database = "ethereum_aml_acceptance".into();
    let router = Router::new()
        .route("/", post(rpc))
        .with_state(chain.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let admin = clickhouse::Client::default()
        .with_url(&cfg.clickhouse_url)
        .with_user(&cfg.clickhouse_user)
        .with_password(&cfg.clickhouse_password);
    ensure!(
        admin
            .query("SELECT count() FROM system.databases WHERE name=?")
            .bind(&cfg.clickhouse_database)
            .fetch_one::<u64>()
            .await?
            == 0,
        "test requires a brand-new database"
    );
    db::initialize_ethereum_schema(&cfg).await?;
    let client = db::database_client(&cfg);
    let before = publication::state(&client, client.database(), Network::Ethereum).await?;

    // A real storage error after facts but before the complete marker/fence.
    Connection::execute(&client, "ALTER TABLE aml_ingested_blocks ADD CONSTRAINT acceptance_interrupt CHECK ingestion_status != 'complete'").await?;
    let first = IngestionService::connect(cfg.clone()).await?;
    assert!(first.ingest_range(7, 7).await.is_err());
    drop(first);
    assert_eq!(
        publication::state(&client, client.database(), Network::Ethereum).await?,
        before
    );
    assert!(
        client
            .query("SELECT count() FROM aml_transactions")
            .fetch_one::<u64>()
            .await?
            > 0
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_transactions_canonical")
            .fetch_one::<u64>()
            .await?,
        0
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_ingestion_failures_current WHERE status='retry'")
            .fetch_one::<u64>()
            .await?,
        1
    );
    Connection::execute(
        &client,
        "ALTER TABLE aml_ingested_blocks DROP CONSTRAINT acceptance_interrupt",
    )
    .await?;
    let restarted = IngestionService::connect(cfg.clone()).await?;
    assert_eq!(restarted.ingest_range(7, 7).await?.completed_blocks, 1);
    let published = publication::state(&client, client.database(), Network::Ethereum).await?;
    assert_eq!(published.last_block_number, 7);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_transactions_canonical")
            .fetch_one::<u64>()
            .await?,
        2
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_ingestion_failures_current WHERE status!='resolved'")
            .fetch_one::<u64>()
            .await?,
        0
    );

    // A complete block cannot bypass hash validation by taking the skip path.
    chain.write().unwrap().blocks[0]["hash"] = json!(hash(777));
    let error = restarted.ingest_range(7, 7).await.unwrap_err();
    ensure!(format!("{error:#}").contains("finalized Ethereum hash conflict"));
    drop(restarted);
    // Polling after restart with no new blocks must still notice the conflict.
    let conflicted = IngestionService::connect(cfg.clone()).await?;
    let error = conflicted
        .follow_finalized(None, Some(1))
        .await
        .unwrap_err();
    ensure!(format!("{error:#}").contains("finalized Ethereum hash conflict"));
    assert_eq!(
        publication::state(&client, client.database(), Network::Ethereum).await?,
        published
    );
    chain.write().unwrap().blocks[0]["hash"] = json!(hash(7));
    assert_eq!(conflicted.ingest_range(7, 7).await?.skipped_blocks, 1);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_ingestion_failures_current WHERE status!='resolved'")
            .fetch_one::<u64>()
            .await?,
        0
    );
    chain.write().unwrap().head = 8;
    assert_eq!(
        conflicted
            .follow_finalized(None, Some(1))
            .await?
            .completed_blocks,
        1
    );
    let advanced = publication::state(&client, client.database(), Network::Ethereum).await?;

    // The tip is unchanged, but an older cached block is no longer the RPC block.
    chain.write().unwrap().blocks[0]["hash"] = json!(hash(777));
    assert!(conflicted.ingest_range(7, 7).await.is_err());
    assert_eq!(
        publication::state(&client, client.database(), Network::Ethereum).await?,
        advanced
    );
    chain.write().unwrap().blocks[0]["hash"] = json!(hash(7));
    assert_eq!(conflicted.ingest_range(7, 7).await?.skipped_blocks, 1);
    chain.write().unwrap().head = 7;
    let error = conflicted.ingest_range(7, 7).await.unwrap_err();
    ensure!(format!("{error:#}").contains("behind published checkpoint"));
    assert_eq!(
        publication::state(&client, client.database(), Network::Ethereum).await?,
        advanced
    );
    chain.write().unwrap().head = 8;
    conflicted.ingest_range(8, 8).await?;
    drop(conflicted);

    db::check_ethereum_schema(&cfg).await?;
    assert_eq!(
        client
            .query("SELECT count() FROM aml_ingestion_failures_current WHERE status!='resolved'")
            .fetch_one::<u64>()
            .await?,
        0
    );
    let app = ethereum_aml::api::build_router(cfg).await?;
    let wallet = response(app, &format!("/api/ethereum/wallet/{B}/investigation")).await?;
    assert_eq!(wallet["source_snapshot"]["as_of_block"], 8);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_transactions_canonical")
            .fetch_one::<u64>()
            .await?,
        3
    );
    assert_eq!(client.query("SELECT count() FROM system.tables WHERE database=currentDatabase() AND NOT startsWith(name,'aml_')").fetch_one::<u64>().await?, 0);
    server.abort();
    Ok(())
}

async fn reject_invalid_refresh(client: &db::Client) -> Result<()> {
    use aml_warehouse::{Network, core, publication};
    let state = publication::state(client, client.database(), Network::Ethereum).await?;
    let old = core::read(
        client,
        client.database(),
        Network::Ethereum,
        core::View::Blocks,
        &core::Query {
            from: Some(7),
            to: Some(7),
            limit: 1,
        },
    )
    .await?
    .remove(0);
    let mut block: publication::Block = serde_json::from_value(old.clone())?;
    block.revision = state.revision + 1;
    block.commit_id = format!("rejected-refresh-{}", block.revision);
    let mut facts = std::collections::BTreeMap::new();
    for table in core::FACTS {
        let mut rows = client
            .rows_from(table, "canonical", "block_number=7")
            .await?;
        for row in &mut rows {
            row["revision"] = json!(block.revision);
            row["commit_id"] = json!(block.commit_id);
        }
        facts.insert(table.to_owned(), rows);
    }
    block.trace_coverage = Some(0);
    let error = core::refresh_observed_batch(
        client,
        client.database(),
        &state,
        &block,
        &facts,
        old["raw_evidence_json"].as_str().unwrap(),
        8,
    )
    .await
    .unwrap_err();
    ensure!(
        error
            .to_string()
            .contains("refresh cannot lower known coverage"),
        "unexpected rejection: {error:#}"
    );
    block.trace_coverage = Some(1);
    block.block_hash = hash(999);
    for rows in facts.values_mut() {
        for row in rows {
            row["block_hash"] = json!(block.block_hash);
        }
    }
    let error = core::refresh_observed_batch(
        client,
        client.database(),
        &state,
        &block,
        &facts,
        old["raw_evidence_json"].as_str().unwrap(),
        8,
    )
    .await
    .unwrap_err();
    ensure!(
        error
            .to_string()
            .contains("refresh cannot change canonical source header"),
        "unexpected rejection: {error:#}"
    );
    assert_eq!(
        publication::state(client, client.database(), Network::Ethereum).await?,
        state
    );
    Ok(())
}
