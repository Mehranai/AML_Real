use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub exchange_name: Option<String>,
    pub exchange_role: Option<String>,
    pub cluster_id: Option<String>,
    pub cluster_role: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub tx_hash: String,
    pub token_address: String,
    pub amount: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub transfer_type: String,
    pub operation_type: String,
    pub relationship_type: String,
    pub protocol: String,
    pub initiator_address: String,
    pub target_address: String,
    pub contract_address: String,
    pub contract_type: String,
    pub transaction_fee: String,
    pub energy_usage_total: u64,
    pub net_usage: u64,
    pub execution_status: u8,
    pub transaction_type: String,
    pub transaction_subtype: String,
    pub classification_confidence: f32,
    pub classification_source: String,
    pub method_id: String,
    pub is_contract_call: bool,
    pub exchange_flow_type: Option<String>,
    pub exchange_name: Option<String>,
    pub exchange_confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExchangeFlowSummary {
    pub exchange_name: String,
    pub exchange_role: String,
    pub address: String,
    pub direction: String,
    pub tx_hash: String,
    pub token_address: String,
    pub amount: String,
    pub block_number: u64,
    pub operation_type: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletFlowGraph {
    pub address: String,
    pub depth: u8,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub incoming_origins: Vec<FlowNode>,
    pub exchange_interactions: Vec<ExchangeFlowSummary>,
    pub neo4j: Neo4jVisualization,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletPathGraph {
    pub address: String,
    pub source_address: String,
    pub target_address: String,
    pub max_depth: u8,
    pub direction: String,
    pub path_count: usize,
    pub searched_node_count: usize,
    pub truncated: bool,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub paths: Vec<WalletPath>,
    pub exchange_interactions: Vec<ExchangeFlowSummary>,
    pub neo4j: Neo4jVisualization,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletPath {
    pub path_index: usize,
    pub hop_count: usize,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Neo4jVisualization {
    pub browser_url: String,
    pub cypher: String,
    pub imported_wallet_nodes: usize,
    pub imported_transfer_edges: usize,
    pub imported_exchange_interactions: usize,
}
