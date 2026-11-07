//! Real ingestor fixtures. RPC data is synthetic, original protobuf hashes are real.
#![allow(dead_code)]
use aml_warehouse::{Connection, Network};
use anyhow::{Result, ensure};
use arz_axum_for_services::{
    config::{AppConfig, SyncMode},
    db::Client,
};
use axum::{Json, Router, extract::State, routing::post};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
#[path = "source_evidence.rs"]
pub mod source;

#[derive(Default)]
pub struct RpcState {
    pub blocks: RwLock<BTreeMap<u64, Value>>,
    pub receipts: RwLock<BTreeMap<String, Value>>,
    pub catalog: RwLock<BTreeMap<String, Value>>,
    pub head: AtomicU64,
    pub receipt_calls: AtomicUsize,
    pub delays: RwLock<BTreeMap<String, u64>>,
    pub constant_fail: std::sync::atomic::AtomicBool,
    pub constant_calls: AtomicUsize,
    pub advance_constant_head: AtomicU64,
}
pub struct Rpc {
    pub url: String,
    pub state: Arc<RpcState>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Rpc {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub fn hash(height: u64) -> String {
    format!("{height:064x}")
}
pub fn salted(mut tx: Value, salt: u64) -> Value {
    let mut bytes = hex::decode(tx["raw_data_hex"].as_str().unwrap()).unwrap();
    let data = salt.to_be_bytes();
    bytes.extend(source::bytes(10, &data));
    tx["raw_data"]["data"] = json!(hex::encode(data));
    tx["raw_data_hex"] = json!(hex::encode(&bytes));
    tx["txID"] = json!(format!("{:x}", Sha256::digest(bytes)));
    tx
}
pub fn simple(amount: u64, salt: u64) -> Value {
    salted(
        source::transaction(vec![source::contract(
            1,
            "TransferContract",
            &[
                source::bytes(1, &hex::decode(source::OWNER).unwrap()),
                source::bytes(2, &hex::decode(source::TARGET).unwrap()),
                source::integer(3, amount),
            ]
            .concat(),
            json!({"owner_address":source::OWNER,"to_address":source::TARGET,"amount":amount}),
        )]),
        salt,
    )
}
impl Rpc {
    pub async fn start() -> Result<Self> {
        async fn head(State(state): State<Arc<RpcState>>) -> Json<Value> {
            let height = state.head.load(Ordering::SeqCst);
            Json(
                state
                    .blocks
                    .read()
                    .unwrap()
                    .get(&height)
                    .cloned()
                    .unwrap_or(json!({"block_header":{"raw_data":{"number":height}}})),
            )
        }
        async fn block(State(state): State<Arc<RpcState>>, Json(body): Json<Value>) -> Json<Value> {
            Json(
                state
                    .blocks
                    .read()
                    .unwrap()
                    .get(&body["num"].as_u64().unwrap())
                    .cloned()
                    .unwrap_or(json!({})),
            )
        }
        async fn receipt(
            State(state): State<Arc<RpcState>>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.receipt_calls.fetch_add(1, Ordering::SeqCst);
            let value = state
                .receipts
                .read()
                .unwrap()
                .get(body["value"].as_str().unwrap())
                .cloned()
                .unwrap_or(json!({}));
            let delay = state
                .delays
                .read()
                .unwrap()
                .get(body["value"].as_str().unwrap())
                .copied()
                .unwrap_or(0);
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            Json(value)
        }
        async fn constant(
            State(state): State<Arc<RpcState>>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.constant_calls.fetch_add(1, Ordering::SeqCst);
            let next = state.advance_constant_head.swap(0, Ordering::SeqCst);
            if next > 0 {
                state.head.store(next, Ordering::SeqCst);
            }
            if state.constant_fail.load(Ordering::SeqCst) {
                return Json(json!({"result":{"result":false,"message":"fixture rejected"}}));
            }
            let value = match body["function_selector"].as_str().unwrap_or("") {
                "decimals()" => format!("{:064x}", 6),
                "symbol()" | "name()" => format!(
                    "{}{}{}{}",
                    format!("{:064x}", 32),
                    format!("{:064x}", 4),
                    hex::encode(b"TEST"),
                    "0".repeat(56)
                ),
                _ => return Json(json!({"result":{"result":false}})),
            };
            Json(json!({"result":{"result":true},"constant_result":[value]}))
        }
        async fn catalog(
            State(state): State<Arc<RpcState>>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            Json(state.catalog.read().unwrap().get(body["value"].as_str().unwrap()).cloned().unwrap_or_else(||{
            let literal=hex::decode(body["value"].as_str().unwrap()).ok().and_then(|v|String::from_utf8(v).ok()).unwrap_or_default();
            if literal=="1000001"{json!({"id":literal,"name":hex::encode("TEST_COIN"),"owner_address":source::OWNER})}else{json!({"Error":"non-unique or unavailable"})}
        }))
        }
        let state = Arc::new(RpcState::default());
        let app = Router::new()
            .route("/walletsolidity/getnowblock", post(head))
            .route("/wallet/getnowblock", post(head))
            .route("/wallet/getblockbynum", post(block))
            .route("/wallet/gettransactioninfobyid", post(receipt))
            .route("/wallet/getassetissuebyname", post(catalog))
            .route("/walletsolidity/triggerconstantcontract", post(constant))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Ok(Self { url, state, task })
    }
    pub fn block(&self, height: u64, transactions: Vec<Value>) {
        let block = json!({"blockID":hash(height),"block_header":{"raw_data":{"number":height,"timestamp":source::TIME+height*1000,"parentHash":if height==0{String::new()}else{hash(height-1)}}},"transactions":transactions});
        self.state.blocks.write().unwrap().insert(height, block);
        self.state.head.fetch_max(height, Ordering::SeqCst);
    }
    pub fn receipt(&self, height: u64, tx: &Value, rich: bool) {
        let mut receipt = if rich {
            source::receipt(tx)
        } else {
            json!({"id":tx["txID"],"fee":0,"receipt":{}})
        };
        receipt["blockNumber"] = json!(height);
        receipt["blockTimeStamp"] = json!(source::TIME + height * 1000);
        self.state
            .receipts
            .write()
            .unwrap()
            .insert(tx["txID"].as_str().unwrap().into(), receipt);
    }
}
pub async fn setup(tag: &str, rpc: &Rpc) -> Result<(AppConfig, Arc<Client>)> {
    let database = format!("tron_pub_{tag}_{}", std::process::id());
    let url = std::env::var("WAREHOUSE_TEST_URL")?;
    let admin = Client::default()
        .with_url(&url)
        .with_user("warehouse_test")
        .with_password("warehouse-test-only");
    ensure!(
        Connection::query(
            &admin,
            &format!("SELECT toString(count()) FROM system.tables WHERE database='{database}'")
        )
        .await?
            == vec!["0"],
        "live fixture must be fresh"
    );
    Connection::execute(&admin, &format!("CREATE DATABASE {database}")).await?;
    aml_warehouse::initialize(&admin, &database, Network::Tron).await?;
    let client = Arc::new(
        admin
            .with_database(&database)
            .with_option("join_algorithm", "auto"),
    );
    let config = AppConfig {
        sync_mode: SyncMode::Auto,
        clickhouse_url: url,
        clickhouse_user: "warehouse_test".into(),
        clickhouse_pass: "warehouse-test-only".into(),
        clickhouse_db_tron: database,
        tron_rpc_url: Some(rpc.url.clone()),
        tron_api_key: None,
        tron_start_block: 100,
        total_tron_txs: 0,
        tron_poll_interval_seconds: 1,
        tron_metadata_poll_interval_seconds: 1,
        tron_metadata_batch_size: 10,
        tron_metadata_max_attempts: 3,
        tron_protocol_registry_refresh_blocks: 0,
        rpc_timeout_seconds: 3,
        rpc_max_concurrency: 4,
        tx_worker_concurrency: 4,
        neo4j_uri: String::new(),
        neo4j_username: String::new(),
        neo4j_password: String::new(),
    };
    Ok((config, client))
}
pub async fn scalar(client: &Client, sql: &str) -> Result<u64> {
    Ok(client.query(sql).fetch_one::<String>().await?.parse()?)
}
