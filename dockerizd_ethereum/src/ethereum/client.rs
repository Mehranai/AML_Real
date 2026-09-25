use std::time::Duration;

use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    network::TransactionResponse as _,
    primitives::{Address, B256, U256},
    providers::{DynProvider, Provider, ProviderBuilder, ext::DebugApi},
    rpc::types::{
        Block, TransactionReceipt,
        trace::geth::{CallConfig, CallFrame, GethDebugTracingOptions, GethTrace, TraceResult},
    },
};
use anyhow::{Context, anyhow, ensure};
use serde::Serialize;
use tokio::time::{sleep, timeout};

use crate::config::{AppConfig, FinalityTag, TraceMode};

#[derive(Debug, Clone, Serialize)]
pub struct EthereumNodeStatus {
    pub network_id: String,
    pub rpc_provider: String,
    pub chain_id: u64,
    pub client_version: String,
    pub latest_block: u64,
    pub finalized_block: u64,
    pub block_receipts_available: bool,
    pub internal_traces_available: bool,
    pub trace_mode: String,
}

#[derive(Debug, Clone)]
pub struct TransactionTrace {
    pub tx_hash: alloy::primitives::B256,
    pub transaction_index: u32,
    pub root: CallFrame,
}

#[derive(Debug)]
pub struct FetchedRpcBlock {
    pub block: Block,
    pub receipts: Vec<TransactionReceipt>,
    pub traces: Option<Vec<TransactionTrace>>,
    pub attempts: u32,
}

#[derive(Clone)]
pub struct EthereumRpc {
    provider: DynProvider,
    network_id: String,
    rpc_provider: String,
    expected_chain_id: u64,
    finality_tag: FinalityTag,
    trace_mode: TraceMode,
    deadline: Duration,
    max_retries: u32,
    retry_base_delay: Duration,
}

impl EthereumRpc {
    pub async fn connect(config: &AppConfig) -> anyhow::Result<Self> {
        let provider = ProviderBuilder::new()
            .connect(&config.eth_rpc_url)
            .await
            .with_context(|| {
                format!(
                    "failed to connect to configured {} Ethereum RPC",
                    config.eth_rpc_provider
                )
            })?
            .erased();
        let client = Self {
            provider,
            network_id: config.eth_network_id.clone(),
            rpc_provider: config.eth_rpc_provider.clone(),
            expected_chain_id: config.eth_expected_chain_id,
            finality_tag: config.eth_finality_tag,
            trace_mode: config.eth_trace_mode,
            deadline: Duration::from_secs(config.rpc_timeout_seconds),
            max_retries: config.eth_rpc_max_retries,
            retry_base_delay: Duration::from_millis(config.eth_rpc_retry_base_delay_ms),
        };
        client.validate_chain_id().await?;
        Ok(client)
    }

    pub async fn status(&self) -> anyhow::Result<EthereumNodeStatus> {
        let chain_id = self.chain_id().await?;
        let client_version = timeout(self.deadline, self.provider.get_client_version())
            .await
            .context("Ethereum client-version request timed out")??;
        let latest_block = timeout(self.deadline, self.provider.get_block_number())
            .await
            .context("Ethereum latest-block request timed out")??;
        let finalized_block = self.finalized_block_number().await?;
        let internal_traces_available = if !self.trace_mode.enabled() {
            false
        } else {
            match self.fetch_trace_results(finalized_block).await {
                Ok(_) => true,
                Err(error) if self.trace_mode.required() => {
                    return Err(error).context("required Ethereum trace capability is unavailable");
                }
                Err(error) => {
                    tracing::warn!(
                        block_number = finalized_block,
                        error = %error,
                        "Ethereum trace probe failed; receipt-only ingestion remains available"
                    );
                    false
                }
            }
        };

        Ok(EthereumNodeStatus {
            network_id: self.network_id.clone(),
            rpc_provider: self.rpc_provider.clone(),
            chain_id,
            client_version,
            latest_block,
            finalized_block,
            block_receipts_available: true,
            internal_traces_available,
            trace_mode: trace_mode_name(self.trace_mode).to_string(),
        })
    }

    pub async fn call_contract(
        &self,
        address: Address,
        calldata: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        self.call_contract_at_tag(address, calldata, "latest").await
    }

    pub async fn call_contract_at_block(
        &self,
        address: Address,
        calldata: &[u8],
        block: u64,
    ) -> anyhow::Result<Vec<u8>> {
        self.call_contract_at_tag(address, calldata, &format!("0x{block:x}"))
            .await
    }

    async fn call_contract_at_tag(
        &self,
        address: Address,
        calldata: &[u8],
        block: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let request = serde_json::json!({
            "to": format!("{address:#x}"),
            "data": format!("0x{}", alloy::hex::encode(calldata)),
        });
        let encoded: String = timeout(
            self.deadline,
            self.provider
                .raw_request("eth_call".into(), (request, block)),
        )
        .await
        .context("Ethereum eth_call request timed out")?
        .with_context(|| format!("eth_call failed for {address:#x}"))?;
        alloy::hex::decode(encoded.strip_prefix("0x").unwrap_or(&encoded))
            .with_context(|| format!("eth_call returned invalid hex for {address:#x}"))
    }

    pub async fn holdings_block(&self) -> anyhow::Result<(u64, B256)> {
        let block = timeout(
            self.deadline,
            self.provider
                .get_block_by_number(BlockNumberOrTag::Finalized),
        )
        .await
        .context("finalized balance block timed out")??
        .context("finalized balance block unavailable")?;
        Ok((block.header.number, block.header.hash))
    }

    pub async fn balance_at_block(&self, address: Address, block: u64) -> anyhow::Result<U256> {
        let raw: String = timeout(
            self.deadline,
            self.provider.raw_request(
                "eth_getBalance".into(),
                (format!("{address:#x}"), format!("0x{block:x}")),
            ),
        )
        .await
        .context("balance request timed out")??;
        let value = raw
            .strip_prefix("0x")
            .context("balance missing quantity prefix")?;
        ensure!(
            !value.is_empty() && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid balance quantity"
        );
        U256::from_str_radix(value, 16).context("invalid balance")
    }

    pub async fn verify_block_hash(&self, height: u64, hash: B256) -> anyhow::Result<()> {
        let block = timeout(
            self.deadline,
            self.provider
                .get_block_by_number(BlockNumberOrTag::Number(height)),
        )
        .await
        .context("balance block verification timed out")??
        .context("balance block disappeared")?;
        ensure!(
            block.header.hash == hash,
            "balance block changed during reads; retry"
        );
        Ok(())
    }

    pub async fn code_at(&self, address: Address) -> anyhow::Result<Vec<u8>> {
        timeout(self.deadline, self.provider.get_code_at(address))
            .await
            .context("Ethereum eth_getCode request timed out")?
            .with_context(|| format!("eth_getCode failed for {address:#x}"))
            .map(|code| code.to_vec())
    }

    pub async fn finalized_block_number(&self) -> anyhow::Result<u64> {
        let finality_tag = match self.finality_tag {
            FinalityTag::Finalized => BlockNumberOrTag::Finalized,
            FinalityTag::Safe => BlockNumberOrTag::Safe,
        };
        let finalized = timeout(
            self.deadline,
            self.provider.get_block_by_number(finality_tag),
        )
        .await
        .context("Ethereum finalized-block request timed out")??
        .ok_or_else(|| anyhow!("Ethereum RPC did not return a finalized block"))?;
        Ok(finalized.header.number)
    }

    pub async fn fetch_block(&self, block_number: u64) -> anyhow::Result<FetchedRpcBlock> {
        let mut last_error = None;
        for attempt in 1..=self.max_retries {
            match self.fetch_block_once(block_number).await {
                Ok((block, receipts, traces)) => {
                    return Ok(FetchedRpcBlock {
                        block,
                        receipts,
                        traces,
                        attempts: attempt,
                    });
                }
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.max_retries {
                        let multiplier = 1_u32 << (attempt - 1).min(5);
                        sleep(self.retry_base_delay * multiplier).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Ethereum RPC retry loop did not execute")))
            .with_context(|| {
                format!(
                    "failed to fetch Ethereum block {block_number} after {} attempts",
                    self.max_retries
                )
            })
    }

    async fn validate_chain_id(&self) -> anyhow::Result<()> {
        let chain_id = self.chain_id().await?;
        ensure!(
            chain_id == self.expected_chain_id,
            "Ethereum RPC returned chain id {chain_id}, expected {}",
            self.expected_chain_id
        );
        Ok(())
    }

    async fn chain_id(&self) -> anyhow::Result<u64> {
        timeout(self.deadline, self.provider.get_chain_id())
            .await
            .context("Ethereum chain-id request timed out")?
            .context("Ethereum chain-id request failed")
    }

    async fn fetch_block_once(
        &self,
        block_number: u64,
    ) -> anyhow::Result<(
        Block,
        Vec<TransactionReceipt>,
        Option<Vec<TransactionTrace>>,
    )> {
        let block_id = BlockId::Number(BlockNumberOrTag::Number(block_number));
        let request = async {
            tokio::try_join!(
                self.provider
                    .get_block_by_number(BlockNumberOrTag::Number(block_number))
                    .full(),
                self.provider.get_block_receipts(block_id),
            )
        };
        let (block, receipts) = timeout(self.deadline, request)
            .await
            .with_context(|| format!("Ethereum block {block_number} request timed out"))??;
        let block = block.ok_or_else(|| anyhow!("Ethereum block {block_number} was not found"))?;
        let receipts = receipts
            .ok_or_else(|| anyhow!("Ethereum receipts for block {block_number} were not found"))?;
        ensure!(
            block.header.number == block_number,
            "Ethereum RPC returned block {} for requested block {block_number}",
            block.header.number
        );
        let traces = self.fetch_traces(&block).await?;
        Ok((block, receipts, traces))
    }

    async fn fetch_traces(&self, block: &Block) -> anyhow::Result<Option<Vec<TransactionTrace>>> {
        if !self.trace_mode.enabled() {
            return Ok(None);
        }

        match self.fetch_trace_results(block.header.number).await {
            Ok(results) => normalize_trace_results(block, results).map(Some),
            Err(error) if self.trace_mode.required() => Err(error).with_context(|| {
                format!(
                    "trace data is required but unavailable for Ethereum block {}",
                    block.header.number
                )
            }),
            Err(error) => {
                tracing::warn!(
                    block_number = block.header.number,
                    error = %error,
                    "trace data unavailable; storing block with trace_data_complete=0"
                );
                Ok(None)
            }
        }
    }

    async fn fetch_trace_results(&self, block_number: u64) -> anyhow::Result<Vec<TraceResult>> {
        let options = GethDebugTracingOptions::call_tracer(CallConfig::default());
        timeout(
            self.deadline,
            self.provider
                .debug_trace_block_by_number(BlockNumberOrTag::Number(block_number), options),
        )
        .await
        .with_context(|| format!("Ethereum block {block_number} trace request timed out"))?
        .with_context(|| {
            format!("debug_traceBlockByNumber failed for Ethereum block {block_number}")
        })
    }
}

fn normalize_trace_results(
    block: &Block,
    results: Vec<TraceResult>,
) -> anyhow::Result<Vec<TransactionTrace>> {
    let transactions = block
        .transactions
        .as_transactions()
        .ok_or_else(|| anyhow!("trace normalization requires full transactions"))?;
    ensure!(
        results.len() == transactions.len(),
        "block {} returned {} traces for {} transactions",
        block.header.number,
        results.len(),
        transactions.len()
    );

    results
        .into_iter()
        .zip(transactions)
        .enumerate()
        .map(|(position, (result, transaction))| {
            let expected_hash = transaction.tx_hash();
            let transaction_index = transaction
                .transaction_index
                .unwrap_or(position as u64)
                .try_into()
                .context("transaction index exceeds UInt32")?;

            match result {
                TraceResult::Success { result, tx_hash } => {
                    if let Some(tx_hash) = tx_hash {
                        ensure!(
                            tx_hash == expected_hash,
                            "trace hash {tx_hash:#x} does not match transaction {expected_hash:#x}"
                        );
                    }
                    let GethTrace::CallTracer(root) = result else {
                        return Err(anyhow!(
                            "debug trace for {expected_hash:#x} did not return callTracer output"
                        ));
                    };
                    Ok(TransactionTrace {
                        tx_hash: expected_hash,
                        transaction_index,
                        root,
                    })
                }
                TraceResult::Error { error, .. } => Err(anyhow!(
                    "debug trace failed for transaction {expected_hash:#x}: {error}"
                )),
            }
        })
        .collect()
}

fn trace_mode_name(mode: TraceMode) -> &'static str {
    match mode {
        TraceMode::Disabled => "disabled",
        TraceMode::Auto => "auto",
        TraceMode::Required => "required",
    }
}

pub async fn probe_node(config: &AppConfig) -> anyhow::Result<EthereumNodeStatus> {
    EthereumRpc::connect(config).await?.status().await
}

#[cfg(test)]
mod holdings_tests {
    use super::*;
    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::{Value, json};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    #[derive(Clone)]
    struct RpcFixture {
        requests: Arc<Mutex<Vec<Value>>>,
        block: Value,
        changed: Arc<AtomicBool>,
    }

    async fn respond(State(state): State<RpcFixture>, Json(request): Json<Value>) -> Json<Value> {
        state.requests.lock().unwrap().push(request.clone());
        let result = match request["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x1"),
            "eth_getBalance" => json!(format!("0x{:x}", U256::MAX)),
            "eth_call" => json!(format!("0x{:064x}", 5)),
            "eth_getBlockByNumber" => {
                let mut block = state.block.clone();
                if state.changed.load(Ordering::SeqCst) {
                    block["hash"] = json!(format!("{:#x}", B256::repeat_byte(2)));
                }
                block
            }
            _ => panic!("unexpected RPC method"),
        };
        Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
    }

    #[tokio::test]
    async fn balances_use_fixed_block_and_detect_wrong_chain_and_changed_hash() {
        let mut block: Block = Block::default();
        block.header.number = 42;
        block.header.hash = B256::repeat_byte(1);
        let state = RpcFixture {
            requests: Arc::new(Mutex::new(Vec::new())),
            block: serde_json::to_value(block).unwrap(),
            changed: Arc::new(AtomicBool::new(false)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/", post(respond))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let provider = ProviderBuilder::new()
            .connect(&format!("http://{addr}"))
            .await
            .unwrap()
            .erased();
        let rpc = EthereumRpc {
            provider,
            network_id: "eip155:1".into(),
            rpc_provider: "test".into(),
            expected_chain_id: 1,
            finality_tag: FinalityTag::Finalized,
            trace_mode: TraceMode::Disabled,
            deadline: Duration::from_secs(3),
            max_retries: 1,
            retry_base_delay: Duration::from_millis(1),
        };
        rpc.validate_chain_id().await.unwrap();
        let (height, hash) = rpc.holdings_block().await.unwrap();
        assert_eq!(height, 42);
        assert_eq!(
            rpc.balance_at_block(Address::repeat_byte(3), height)
                .await
                .unwrap(),
            U256::MAX
        );
        assert_eq!(
            rpc.call_contract_at_block(Address::repeat_byte(4), &[1, 2, 3, 4], height)
                .await
                .unwrap()[31],
            5
        );
        rpc.verify_block_hash(height, hash).await.unwrap();
        for request in state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "eth_call" || r["method"] == "eth_getBalance")
        {
            assert_eq!(request["params"][1], "0x2a");
        }
        state.changed.store(true, Ordering::SeqCst);
        assert!(rpc.verify_block_hash(height, hash).await.is_err());
        let wrong_chain = EthereumRpc {
            expected_chain_id: 56,
            ..rpc
        };
        assert!(wrong_chain.validate_chain_id().await.is_err());
        server.abort();
        let _ = server.await;
    }
}
