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
    fault: Option<(String, u16)>,
    calls: std::collections::BTreeMap<String, usize>,
    chain_id: u64,
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
fn marketplace_log(emitter: &str, index: u64) -> Value {
    let mut event = log(emitter, 7, index, false);
    event["topics"] = json!([
        "0x9d9af8e38d66c62e2c12f0225249fd9d721c54b83f48d9352c97c6cacdcb6f31",
        topic(A),
        hash(0)
    ]);
    let words = [
        hash(99),
        topic(B),
        hash(128),
        hash(288),
        hash(1),
        hash(2),
        topic(NFT),
        format!("0x{}", "f".repeat(64)),
        hash(1),
        hash(2),
        hash(0),
        hash(0),
        hash(0),
        hash(11),
        topic(A),
        hash(0),
        hash(0),
        hash(0),
        hash(1),
        topic(C),
    ];
    event["data"] = json!(format!(
        "0x{}",
        words.iter().map(|word| &word[2..]).collect::<String>()
    ));
    event
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
        fault: None,
        calls: Default::default(),
        chain_id: 1,
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
async fn rpc(
    State(chain): State<Arc<RwLock<Chain>>>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    {
        let mut state = chain.write().unwrap();
        let requests = request
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![request.clone()]);
        let mut status = None;
        for r in requests {
            let method = r["method"].as_str().unwrap_or_default();
            *state.calls.entry(method.into()).or_default() += 1;
            if let Some((fault, code)) = &state.fault {
                if fault == method {
                    status = Some(*code);
                }
            }
        }
        if let Some(status) = status {
            return (
                axum::http::StatusCode::from_u16(status).unwrap(),
                "synthetic RPC outage",
            )
                .into_response();
        }
    }
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
        let i = chain
            .blocks
            .iter()
            .position(|b| b["number"] == format!("0x{height:x}"))
            .unwrap_or(0);
        let value = match method {
            "eth_chainId" => json!(format!("0x{:x}", chain.chain_id)),
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
    .into_response()
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
async fn tbtc_mint_reads_one_successful_canonical_receipt() -> Result<()> {
    let evidence: Value = serde_json::from_str(include_str!(
        "../../shared/chain_api/tests/fixtures/tbtc-mint-mainnet.json"
    ))?;
    let mut chain = fixture()?;
    let mint = evidence["mint"].as_array().context("missing mint logs")?;
    chain.receipts[0][0]["logs"]
        .as_array_mut()
        .unwrap()
        .extend(mint.iter().enumerate().map(|(i, log)| {
            json!({
                "address":log["address"],"topics":log["topics"],"data":log["data"],
                "blockNumber":"0x7","blockHash":hash(7),"transactionHash":hash(70),
                "transactionIndex":"0x0","logIndex":format!("0x{:x}", i + 2),"removed":false
            })
        }));
    let mut failed = chain.receipts[0][0]["logs"][4].clone();
    failed["logIndex"] = json!("0x5");
    failed["transactionHash"] = json!(hash(71));
    failed["transactionIndex"] = json!("0x1");
    chain.receipts[0][1]["logs"] = json!([failed]);
    let timestamp = u64::from_str_radix(
        chain.blocks[0]["timestamp"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
        16,
    )? * 1000;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let router = Router::new()
        .route("/", post(rpc))
        .with_state(Arc::new(RwLock::new(chain)));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut cfg = config(endpoint)?;
    cfg.eth_trace_mode = TraceMode::Disabled;
    cfg.clickhouse_database = "ethereum_tbtc_mint_fixture".into();
    db::initialize_ethereum_schema(&cfg).await?;
    IngestionService::connect(cfg.clone())
        .await?
        .ingest_range(7, 7)
        .await?;
    let app = ethereum_aml::api::build_router(cfg).await?;
    let key = mint[2]["topics"][2].as_str().unwrap();
    let path =
        |key| format!("/internal/bridge/tbtc/mints/{key}?from_ms={timestamp}&to_ms={timestamp}");
    let observed = response(app.clone(), &path(key)).await?;
    let logs = observed["finalizations"]["logs"]
        .as_array()
        .context("missing finalizations")?;
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["log_index"], 4);
    assert_eq!(logs[0]["tx_hash"], hash(70));
    let mut neighbors: Vec<_> = observed["preceding_logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|log| {
            assert_eq!(log["tx_hash"], hash(70));
            log["log_index"].as_u64().unwrap()
        })
        .collect();
    neighbors.sort_unstable();
    assert_eq!(neighbors, [2, 3]);
    let missing = response(app, &path(&hash(1))).await?;
    assert_eq!(missing["finalizations"]["logs"], json!([]));
    assert_eq!(missing["preceding_logs"], json!([]));
    server.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "requires a new disposable ClickHouse database and fixture service-key environment"]
async fn rpc_to_common_storage_workers_and_investigation() -> Result<()> {
    ethereum_aml::init_tracing();
    let mut fixture = fixture()?;
    let tbtc_fixture: Value = serde_json::from_str(include_str!(
        "../../shared/chain_api/tests/fixtures/tbtc-mainnet.json"
    ))?;
    let sweep_fixture: Value = serde_json::from_str(include_str!(
        "../../shared/chain_api/tests/fixtures/tbtc-sweep-mainnet.json"
    ))?;
    let reveal_log = json!({"address":tbtc_fixture["eth"]["address"],"topics":tbtc_fixture["eth"]["topics"],
        "data":tbtc_fixture["eth"]["data"],"blockNumber":"0x7","blockHash":hash(7),
        "transactionHash":hash(70),"transactionIndex":"0x0","logIndex":"0x4","removed":false});
    let mut failed_reveal = reveal_log.clone();
    failed_reveal["logIndex"] = json!("0x5");
    failed_reveal["transactionHash"] = json!(hash(71));
    failed_reveal["transactionIndex"] = json!("0x1");
    let sweep_log = json!({"address":sweep_fixture["proof"]["address"],"topics":sweep_fixture["proof"]["topics"],
        "data":sweep_fixture["proof"]["data"],"blockNumber":"0x7","blockHash":hash(7),
        "transactionHash":hash(70),"transactionIndex":"0x0","logIndex":"0x6","removed":false});
    let mut failed_sweep = sweep_log.clone();
    failed_sweep["logIndex"] = json!("0x7");
    failed_sweep["transactionHash"] = json!(hash(71));
    failed_sweep["transactionIndex"] = json!("0x1");
    fixture.receipts[0][1]["logs"] = json!([failed_reveal, failed_sweep]);
    fixture.receipts[0][0]["logs"]
        .as_array_mut()
        .unwrap()
        .extend([
            marketplace_log("0x0000000000000068f116a894984e2db1123eb395", 2),
            marketplace_log(C, 3),
            reveal_log,
            sweep_log,
        ]);
    let chain = Arc::new(RwLock::new(fixture));
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
    let timestamp = u64::from_str_radix(
        chain.read().unwrap().blocks[0]["timestamp"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
        16,
    )? * 1000;
    let funding = tbtc_fixture["btc"]["tx_hash"].as_str().unwrap();
    let reveal_path =
        format!("/internal/bridge/tbtc/reveals/{funding}?from_ms={timestamp}&to_ms={timestamp}");
    let found = response(app.clone(), &format!("{reveal_path}&output_index=1")).await?;
    assert_eq!(found["logs"].as_array().unwrap().len(), 1);
    assert_eq!(found["logs"][0]["log_index"], 4);
    assert_eq!(
        found["logs"][0]["data"],
        tbtc_fixture["eth"]["data"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x")
    );
    let missed = response(app.clone(), &format!("{reveal_path}&output_index=0")).await?;
    assert_eq!(missed["logs"], json!([]));
    let sweep_tx = sweep_fixture["spend"]["tx_hash"].as_str().unwrap();
    let sweeps_path =
        |txid| format!("/internal/bridge/tbtc/sweeps/{txid}?from_ms={timestamp}&to_ms={timestamp}");
    let found = response(app.clone(), &sweeps_path(sweep_tx)).await?;
    assert_eq!(found["logs"].as_array().unwrap().len(), 1);
    assert_eq!(found["logs"][0]["log_index"], 6);
    assert_eq!(
        response(app.clone(), &sweeps_path(funding)).await?["logs"],
        json!([])
    );
    let first = response(
        app.clone(),
        &format!("/api/ethereum/wallet/{B}/transfers?limit=1"),
    )
    .await?;
    let cursor = first["pagination"]["next_cursor"].as_str().unwrap();
    let second = response(
        app.clone(),
        &format!("/api/ethereum/wallet/{B}/transfers?limit=1&cursor={cursor}"),
    )
    .await?;
    assert_eq!(first["source_snapshot"], second["source_snapshot"]);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
    let filtered=response(app.clone(),&format!("/api/ethereum/wallet/{B}/transfers?limit=100&from_block=7&to_block=7&asset_id=native:ETH")).await?;
    assert!(!filtered["items"].as_array().unwrap().is_empty());
    assert!(
        filtered["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["asset_id"] == "native:ETH" && r["amount"].is_string())
    );
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
    let marketplace = &wallet["nft_marketplace_evidence"];
    assert_eq!(marketplace["status"], "available");
    assert_eq!(marketplace["searched_transactions"], 1);
    assert_eq!(marketplace["items"].as_array().unwrap().len(), 1);
    let order = &marketplace["items"][0];
    assert_eq!(order["receipt"]["log_index"], 2);
    assert_eq!(
        order["order"]["offer"][0]["identifier"],
        aml_warehouse::RawAmount::MAX
    );
    assert_eq!(order["order"]["consideration"][1]["recipient"], C);
    assert_eq!(order["order"]["consideration"][0]["amount_raw"], "11");
    assert_eq!(order["related_nft_transfers"].as_array().unwrap().len(), 1);
    aml_chain_api::response::validate(
        aml_warehouse::Network::Ethereum,
        aml_chain_api::response::Mode::Wallet,
        &wallet,
    )?;
    let mut forged = wallet.clone();
    forged["nft_marketplace_evidence"]["items"][0]["order"]["consideration"][0]["amount_raw"] =
        json!("12");
    assert!(
        aml_chain_api::response::validate(
            aml_warehouse::Network::Ethereum,
            aml_chain_api::response::Mode::Wallet,
            &forged
        )
        .is_err()
    );
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
            license: "test fixture".into(),
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
    if let Ok(path) = std::env::var("CHAIN_RESPONSE_PROOF") {
        let paths = response(
            app.clone(),
            &format!("/api/ethereum/wallet/{A}/paths/{C}?max_hops=3"),
        )
        .await?;
        let status = response(app.clone(), "/status").await?;
        assert!(
            !assessed["evidence"]["attribution"]["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        std::fs::write(
            path,
            serde_json::to_vec(
                &json!({"wallet":assessed,"paths":paths,"receipts":{"/status":status}}),
            )?,
        )?;
    }
    // A reviewed service is an endpoint, not a bridge for inferred fund continuity.
    std::fs::write(
        &path,
        format!(
            "address,entity_id,entity_name,entity_type,address_role,confidence,risk_level,is_exposure_seed,seed_category,source_record_id,evidence_refs,review_status,reviewed_by,review_reason\n{B},fixture-service,Fixture service,exchange,HOT_WALLET,0.99,0,false,,case-2,https://example.invalid/case-2,APPROVED,fixture-analyst,Reviewed synthetic service\n"
        ),
    )?;
    intelligence::import_entity_labels_csv(&cfg, &path, options.clone()).await?;
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
    let revision = claim["revision"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| claim["revision"].to_string());
    let revoke_csv = |expected: &str| {
        format!(
            "address,entity_id,entity_name,entity_type,address_role,confidence,risk_level,is_exposure_seed,seed_category,source_record_id,evidence_refs,review_status,reviewed_by,review_reason,expected_revision\n{A},fixture-illicit,Fixture illicit,illicit,HOT_WALLET,0.95,95,true,illicit,case-1,https://example.invalid/revoke,REVOKED,fixture-analyst,Withdraw fixture claim,{expected}\n"
        )
    };
    std::fs::write(&path, revoke_csv("1"))?;
    assert!(
        intelligence::import_entity_labels_csv(&cfg, &path, options.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("matching revision")
    );
    std::fs::write(&path, revoke_csv(&revision))?;
    let revoked = intelligence::import_entity_labels_csv(&cfg, &path, options).await?;
    assert_eq!((revoked.submitted, revoked.revoked), (0, 1));
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
#[ignore = "requires a disposable ETHEREUM_TEST_CLICKHOUSE_URL"]
async fn follow_recovers_rpc_failures_catches_new_head_and_resumes_checkpoint() -> Result<()> {
    use std::time::Duration;
    async fn wait_height(client: &db::Client, height: u64) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                if client
                    .query("SELECT max(block_number) FROM aml_ingested_blocks_canonical")
                    .fetch_one::<u64>()
                    .await?
                    == height
                {
                    return Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await??;
        Ok(())
    }
    let chain = Arc::new(RwLock::new(fixture()?));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let mut cfg = config(format!("http://{}", listener.local_addr()?))?;
    cfg.clickhouse_database = "ethereum_aml_ing1".into();
    cfg.eth_trace_mode = TraceMode::Disabled;
    let app = Router::new()
        .route("/", post(rpc))
        .with_state(chain.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    db::initialize_ethereum_schema(&cfg).await?;
    let client = db::database_client(&cfg);
    ensure!(
        client
            .query("SELECT count() FROM aml_ingested_blocks_canonical")
            .fetch_one::<u64>()
            .await?
            == 0,
        "follow fixture must be fresh"
    );
    let service = IngestionService::connect(cfg.clone()).await?;
    chain.write().unwrap().fault = Some(("eth_getBlockByNumber".into(), 503));
    let worker = tokio::spawn(async move { service.follow_finalized(None, Some(2)).await });
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if chain
                .read()
                .unwrap()
                .calls
                .get("eth_getBlockByNumber")
                .copied()
                .unwrap_or(0)
                >= 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    ensure!(
        !worker.is_finished(),
        "follow exited on a temporary head outage"
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_ingested_blocks_canonical")
            .fetch_one::<u64>()
            .await?,
        0
    );
    chain.write().unwrap().fault = None;
    wait_height(&client, 7).await?;
    let receipts = chain
        .read()
        .unwrap()
        .calls
        .get("eth_getBlockReceipts")
        .copied()
        .unwrap_or(0);
    {
        let mut state = chain.write().unwrap();
        state.head = 8;
        state.fault = Some(("eth_getBlockReceipts".into(), 503));
    }
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if chain
                .read()
                .unwrap()
                .calls
                .get("eth_getBlockReceipts")
                .copied()
                .unwrap_or(0)
                >= receipts + 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    ensure!(
        !worker.is_finished(),
        "follow exited on a temporary receipt outage"
    );
    assert_eq!(
        client
            .query("SELECT max(block_number) FROM aml_ingested_blocks_canonical")
            .fetch_one::<u64>()
            .await?,
        7
    );
    chain.write().unwrap().fault = None;
    let report = tokio::time::timeout(Duration::from_secs(45), worker).await???;
    assert_eq!(report.completed_blocks, 2);
    assert_eq!(report.to_block, 8);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_transactions_canonical")
            .fetch_one::<u64>()
            .await?,
        3
    );
    let frame = aml_warehouse::visibility::capture(
        &client,
        &cfg.clickhouse_database,
        aml_warehouse::Network::Ethereum,
    )
    .await?;
    assert_eq!(frame.value()["history_complete"], false);
    {
        let mut state = chain.write().unwrap();
        let mut next = state.blocks[1].clone();
        next["number"] = json!("0x9");
        next["hash"] = json!(hash(9));
        next["parentHash"] = json!(hash(8));
        next["transactions"] = json!([]);
        next["gasUsed"] = json!("0x0");
        state.blocks.push(next);
        state.receipts.push(vec![]);
        state.traces.push(vec![]);
        state.head = 9;
    }
    let restarted = IngestionService::connect(cfg.clone()).await?;
    let report = tokio::time::timeout(
        Duration::from_secs(45),
        restarted.follow_finalized(None, Some(1)),
    )
    .await??;
    assert_eq!(report.completed_blocks, 1);
    assert_eq!(report.from_block, 9);
    assert_eq!(
        client
            .query("SELECT count() FROM aml_ingested_blocks_canonical")
            .fetch_one::<u64>()
            .await?,
        3
    );
    chain.write().unwrap().chain_id = 56;
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        restarted.follow_finalized(None, Some(1)),
    )
    .await?
    .unwrap_err();
    ensure!(
        error.to_string().contains("chain id"),
        "wrong-network RPC was retried: {error:#}"
    );
    assert_eq!(
        client
            .query("SELECT count() FROM aml_ingested_blocks_canonical")
            .fetch_one::<u64>()
            .await?,
        3
    );
    drop(restarted);
    server.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "requires disposable ETHEREUM_TEST_CLICKHOUSE_URL and ETHEREUM_FAILURE_MAX_ATTEMPTS=2"]
async fn maintenance_probe_gap_refresh_and_explicit_dead_letter_repair() -> Result<()> {
    use aml_warehouse::{
        Network,
        maintenance::{self, Command},
        publication,
    };
    ensure!(
        maintenance::failure_limit("ETHEREUM_FAILURE_MAX_ATTEMPTS")? == 2,
        "fixture requires ETHEREUM_FAILURE_MAX_ATTEMPTS=2"
    );
    let chain = Arc::new(RwLock::new(fixture()?));
    chain.write().unwrap().head = 8;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let mut cfg = config(format!("http://{}", listener.local_addr()?))?;
    cfg.clickhouse_database = "ethereum_aml_ing2".into();
    let app = Router::new()
        .route("/", post(rpc))
        .with_state(chain.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    db::initialize_ethereum_schema(&cfg).await?;
    let client = db::database_client(&cfg);
    let probe = ethereum_aml::ethereum::probe_node(&cfg).await?;
    assert_eq!(probe.status, "ready");
    assert!(probe.block_receipts_available);
    assert_eq!(probe.history_samples.len(), 2);
    assert!(!probe.full_history_verified);
    let service = IngestionService::connect(cfg.clone()).await?;
    service.ingest_range(7, 8).await?;
    let before = publication::state(&client, &cfg.clickhouse_database, Network::Ethereum).await?;
    assert_eq!(
        maintenance::scan(
            &client,
            &cfg.clickhouse_database,
            Network::Ethereum,
            7,
            8,
            2,
            true
        )
        .await?["gaps"],
        0
    );
    aml_warehouse::Connection::execute(&client,"ALTER TABLE aml_transactions DELETE WHERE block_number=7 AND transaction_index=0 SETTINGS mutations_sync=2").await?;
    assert_eq!(
        maintenance::scan(
            &client,
            &cfg.clickhouse_database,
            Network::Ethereum,
            7,
            8,
            2,
            true
        )
        .await?["gaps"],
        1
    );
    let command = Command::parse(&[
        "repair".into(),
        "--from-block".into(),
        "7".into(),
        "--to-block".into(),
        "7".into(),
        "--max-blocks".into(),
        "1".into(),
    ])?;
    assert_eq!(service.maintenance(&command).await?["status"], "complete");
    let after = publication::state(&client, &cfg.clickhouse_database, Network::Ethereum).await?;
    assert_eq!(before.last_block_number, after.last_block_number);
    assert_eq!(before.last_block_hash, after.last_block_hash);
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
            .query("SELECT count() FROM aml_address_relationships_canonical WHERE token_id=?")
            .bind(aml_warehouse::RawAmount::MAX)
            .fetch_one::<u64>()
            .await?,
        1
    );
    {
        let mut data = chain.write().unwrap();
        let mut block = data.blocks[1].clone();
        block["number"] = json!("0x9");
        block["hash"] = json!(hash(9));
        block["parentHash"] = json!(hash(8));
        block["transactions"] = json!([]);
        block["gasUsed"] = json!("0x0");
        data.blocks.push(block);
        data.receipts.push(vec![]);
        data.traces.push(vec![]);
        data.head = 9;
        data.fault = Some(("eth_getBlockReceipts".into(), 503));
    }
    assert!(ethereum_aml::ethereum::probe_node(&cfg).await.is_err());
    for _ in 0..2 {
        assert!(service.ingest_range(9, 9).await.is_err());
    }
    assert!(maintenance::blocked(&client, &cfg.clickhouse_database, Network::Ethereum, 9).await?);
    chain.write().unwrap().fault = None;
    assert!(
        service.ingest_range(9, 9).await.is_err(),
        "normal ingestion bypassed a dead letter"
    );
    let mut repair = command.clone();
    repair.from = Some(9);
    repair.to = Some(9);
    assert!(service.maintenance(&repair).await.is_err());
    repair.include_dead = true;
    assert_eq!(service.maintenance(&repair).await?["status"], "complete");
    assert!(!maintenance::blocked(&client, &cfg.clickhouse_database, Network::Ethereum, 9).await?);
    assert_eq!(
        publication::state(&client, &cfg.clickhouse_database, Network::Ethereum)
            .await?
            .last_block_number,
        9
    );
    chain.write().unwrap().chain_id = 56;
    assert!(ethereum_aml::ethereum::probe_node(&cfg).await.is_err());
    let mut replay = command.clone();
    replay.action = "replay".into();
    assert!(service.maintenance(&replay).await.is_err());
    assert_eq!(
        publication::state(&client, &cfg.clickhouse_database, Network::Ethereum)
            .await?
            .last_block_number,
        9
    );
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
