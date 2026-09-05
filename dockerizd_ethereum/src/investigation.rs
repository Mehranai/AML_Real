use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{Context, ensure};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};

use crate::{
    graph::{EthereumGraph, ProjectionSummary},
    risk::{EvidenceRiskEngine, RiskAssessmentOutput},
};

const DEFAULT_EDGE_LIMIT: u64 = 750;
const MAX_EDGE_LIMIT: u64 = 5_000;
const DEFAULT_PATH_LIMIT: usize = 5;
const MAX_PATH_LIMIT: usize = 25;
const DEFAULT_PER_ADDRESS_LIMIT: u64 = 500;
const MAX_PER_ADDRESS_LIMIT: u64 = 2_000;

#[derive(Clone)]
pub struct InvestigationService {
    clickhouse: Client,
    graph: EthereumGraph,
    network_id: String,
    graph_max_edges: usize,
    risk_engine: EvidenceRiskEngine,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowEdge {
    pub id: String,
    pub network_id: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub from_address: String,
    pub to_address: String,
    pub asset_id: String,
    pub token_id: String,
    pub amount: String,
    pub transfer_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub address: String,
    pub is_focus: bool,
    pub inbound_edges: u64,
    pub outbound_edges: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<FlowEdge>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletFingerprint {
    pub transaction_count: u64,
    pub transfer_count: u64,
    pub inbound_transfers: u64,
    pub outbound_transfers: u64,
    pub unique_counterparties: u64,
    pub failed_transactions: u64,
    pub contract_calls: u64,
    pub first_seen_unix_ms: u64,
    pub last_seen_unix_ms: u64,
    pub swap_events: u64,
    pub bridge_events: u64,
    pub mixer_events: u64,
    pub liquidity_events: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CounterpartySummary {
    pub address: String,
    pub inbound_transfers: u64,
    pub outbound_transfers: u64,
    pub total_transfers: u64,
    pub last_seen_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetFlow {
    pub asset_id: String,
    pub inbound_amount: String,
    pub outbound_amount: String,
    pub inbound_transfers: u64,
    pub outbound_transfers: u64,
    pub transfer_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SemanticEvent {
    pub event_id: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub event_type: String,
    pub protocol: String,
    pub protocol_contract: String,
    pub counterparty_address: String,
    pub asset_in: String,
    pub asset_out: String,
    pub remote_network_id: String,
    pub bridge_direction: String,
    pub amount_in: String,
    pub amount_out: String,
    pub confidence: f32,
    pub evidence_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataCoverage {
    pub last_synced_block: u64,
    pub complete_blocks: u64,
    pub receipt_complete_blocks: u64,
    pub trace_complete_blocks: u64,
    pub internal_transfer_coverage: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletEntity {
    pub entity_id: String,
    pub entity_name: String,
    pub entity_type: String,
    pub address_role: String,
    pub confidence: f32,
    pub risk_level: u8,
    pub is_exposure_seed: bool,
    pub seed_category: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletCluster {
    pub cluster_id: String,
    pub entity_id: String,
    pub membership_type: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletExposurePath {
    pub path_id: String,
    pub run_id: String,
    pub seed_address: String,
    pub seed_entity_id: String,
    pub seed_category: String,
    pub direction: String,
    pub hop_count: u8,
    pub asset_id: String,
    pub exposure_score: f64,
    pub service_mediated: bool,
    pub continuity_type: String,
    pub path_addresses: Vec<String>,
    pub relationship_ids: Vec<String>,
    pub tx_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletInvestigation {
    pub api_version: &'static str,
    pub network_id: String,
    pub address: String,
    pub fingerprint: WalletFingerprint,
    pub top_counterparties: Vec<CounterpartySummary>,
    pub asset_flows: Vec<AssetFlow>,
    pub semantic_events: Vec<SemanticEvent>,
    pub entities: Vec<WalletEntity>,
    pub clusters: Vec<WalletCluster>,
    pub exposure_paths: Vec<WalletExposurePath>,
    pub graph: WalletGraph,
    pub data_coverage: DataCoverage,
    pub neo4j_projection: ProjectionSummary,
    pub risk_engine: RiskAssessmentOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathDirection {
    Outbound,
    Both,
}

impl PathDirection {
    pub fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value
            .unwrap_or("outbound")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "outbound" | "forward" => Ok(Self::Outbound),
            "both" | "undirected" => Ok(Self::Both),
            value => anyhow::bail!("unsupported path direction {value}; use outbound or both"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Outbound => "outbound",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletPath {
    pub hop_count: usize,
    pub addresses: Vec<String>,
    pub edge_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathSearchResult {
    pub network_id: String,
    pub source: String,
    pub target: String,
    pub direction: String,
    pub max_hops: u8,
    pub paths: Vec<WalletPath>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<FlowEdge>,
    pub expanded_addresses: usize,
    pub truncated: bool,
    pub neo4j_projection: ProjectionSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformStatus {
    pub network_id: String,
    pub last_synced_block: u64,
    pub transaction_count: u64,
    pub relationship_count: u64,
    pub semantic_event_count: u64,
}

#[derive(Debug, Deserialize, Row)]
struct FlowEdgeRow {
    relationship_id: String,
    network_id: String,
    tx_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    from_address: String,
    to_address: String,
    asset_id: String,
    token_id: String,
    amount: String,
    transfer_type: String,
}

#[derive(Debug, Deserialize, Row)]
struct FingerprintRow {
    transfer_count: u64,
    inbound_transfers: u64,
    outbound_transfers: u64,
    unique_counterparties: u64,
    first_seen_unix_ms: u64,
    last_seen_unix_ms: u64,
}

#[derive(Debug, Deserialize, Row)]
struct TransactionSummaryRow {
    transaction_count: u64,
    failed_transactions: u64,
    contract_calls: u64,
}

#[derive(Debug, Deserialize, Row)]
struct SemanticCountRow {
    swap_events: u64,
    bridge_events: u64,
    mixer_events: u64,
    liquidity_events: u64,
}

#[derive(Debug, Deserialize, Row)]
struct AssetFlowRow {
    asset_id: String,
    inbound_amount: String,
    outbound_amount: String,
    inbound_transfers: u64,
    outbound_transfers: u64,
    transfer_count: u64,
}

#[derive(Debug, Deserialize, Row)]
struct SemanticEventRow {
    event_id: String,
    tx_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    event_type: String,
    protocol: String,
    protocol_contract: String,
    counterparty_address: String,
    asset_in: String,
    asset_out: String,
    remote_network_id: String,
    bridge_direction: String,
    amount_in: String,
    amount_out: String,
    confidence: f32,
    evidence_json: String,
}

#[derive(Debug, Deserialize, Row)]
struct WalletEntityRow {
    entity_id: String,
    entity_name: String,
    entity_type: String,
    address_role: String,
    confidence: f32,
    risk_level: u8,
    is_exposure_seed: u8,
    seed_category: String,
}

#[derive(Debug, Deserialize, Row)]
struct WalletClusterRow {
    cluster_id: String,
    entity_id: String,
    membership_type: String,
    confidence: f32,
}

#[derive(Debug, Deserialize, Row)]
struct WalletExposurePathRow {
    path_id: String,
    run_id: String,
    seed_address: String,
    seed_entity_id: String,
    seed_category: String,
    direction: String,
    hop_count: u8,
    asset_id: String,
    exposure_score: f64,
    service_mediated: u8,
    continuity_type: String,
    path_addresses: Vec<String>,
    relationship_ids: Vec<String>,
    tx_hashes: Vec<String>,
}

#[derive(Debug, Deserialize, Row)]
struct CoverageRow {
    complete_blocks: u64,
    receipt_complete_blocks: u64,
    trace_complete_blocks: u64,
}

#[derive(Debug, Deserialize, Row)]
struct CheckpointRow {
    last_synced_block: u64,
}

#[derive(Debug, Deserialize, Row)]
struct PlatformStatusRow {
    transaction_count: Option<u64>,
    relationship_count: Option<u64>,
    semantic_event_count: Option<u64>,
}

impl InvestigationService {
    pub fn new(
        clickhouse: Client,
        graph: EthereumGraph,
        network_id: String,
        graph_max_edges: usize,
        risk_engine: EvidenceRiskEngine,
    ) -> Self {
        Self {
            clickhouse,
            graph,
            network_id,
            graph_max_edges,
            risk_engine,
        }
    }

    pub async fn probe_clickhouse(&self) -> anyhow::Result<()> {
        #[derive(Debug, Deserialize, Row)]
        struct Probe {
            ready: u8,
        }

        let probe = self
            .clickhouse
            .query("SELECT toUInt8(1) AS ready")
            .fetch_one::<Probe>()
            .await
            .context("ClickHouse readiness probe failed")?;
        ensure!(probe.ready == 1, "ClickHouse readiness probe returned 0");
        Ok(())
    }

    pub async fn probe_neo4j(&self) -> anyhow::Result<()> {
        self.graph.probe().await
    }

    pub async fn investigate_wallet(
        &self,
        address: &str,
        requested_limit: Option<u64>,
    ) -> anyhow::Result<WalletInvestigation> {
        self.investigate_wallet_with_projection(address, requested_limit, false)
            .await
    }

    pub async fn investigate_wallet_and_project(
        &self,
        address: &str,
        requested_limit: Option<u64>,
    ) -> anyhow::Result<WalletInvestigation> {
        self.investigate_wallet_with_projection(address, requested_limit, true)
            .await
    }

    async fn investigate_wallet_with_projection(
        &self,
        address: &str,
        requested_limit: Option<u64>,
        project_to_neo4j: bool,
    ) -> anyhow::Result<WalletInvestigation> {
        let limit = requested_limit
            .unwrap_or(DEFAULT_EDGE_LIMIT)
            .clamp(1, MAX_EDGE_LIMIT);
        let (
            edges,
            fingerprint,
            asset_flows,
            semantic_events,
            entities,
            clusters,
            exposure_paths,
            data_coverage,
        ) = tokio::try_join!(
            self.load_edges_for_address(address, limit),
            self.load_fingerprint(address),
            self.load_asset_flows(address),
            self.load_semantic_events(address, 100),
            self.load_entities(address),
            self.load_clusters(address),
            self.load_exposure_paths(address, 50),
            self.load_data_coverage(),
        )?;
        let risk_engine = self.risk_engine.assess(address).await?;
        let graph = build_wallet_graph(address, edges, limit as usize);

        let projection = if project_to_neo4j {
            self.graph.project_wallet(&self.network_id, address).await?;
            self.graph.project_edges(&graph.edges).await?
        } else {
            ProjectionSummary::default()
        };
        let top_counterparties = counterparties(address, &graph.edges, 25);

        Ok(WalletInvestigation {
            api_version: "1.0",
            network_id: self.network_id.clone(),
            address: address.to_string(),
            fingerprint,
            top_counterparties,
            asset_flows,
            semantic_events,
            entities,
            clusters,
            exposure_paths,
            graph,
            data_coverage,
            neo4j_projection: projection,
            risk_engine,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn find_paths(
        &self,
        source: &str,
        target: &str,
        requested_max_hops: Option<u8>,
        requested_limit: Option<usize>,
        requested_per_address_limit: Option<u64>,
        direction: PathDirection,
    ) -> anyhow::Result<PathSearchResult> {
        self.find_paths_with_projection(
            source,
            target,
            requested_max_hops,
            requested_limit,
            requested_per_address_limit,
            direction,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn find_paths_and_project(
        &self,
        source: &str,
        target: &str,
        requested_max_hops: Option<u8>,
        requested_limit: Option<usize>,
        requested_per_address_limit: Option<u64>,
        direction: PathDirection,
    ) -> anyhow::Result<PathSearchResult> {
        self.find_paths_with_projection(
            source,
            target,
            requested_max_hops,
            requested_limit,
            requested_per_address_limit,
            direction,
            true,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn find_paths_with_projection(
        &self,
        source: &str,
        target: &str,
        requested_max_hops: Option<u8>,
        requested_limit: Option<usize>,
        requested_per_address_limit: Option<u64>,
        direction: PathDirection,
        project_to_neo4j: bool,
    ) -> anyhow::Result<PathSearchResult> {
        ensure!(source != target, "source and target addresses must differ");
        let max_hops = requested_max_hops.unwrap_or(6).clamp(1, 10);
        let path_limit = requested_limit
            .unwrap_or(DEFAULT_PATH_LIMIT)
            .clamp(1, MAX_PATH_LIMIT);
        let per_address_limit = requested_per_address_limit
            .unwrap_or(DEFAULT_PER_ADDRESS_LIMIT)
            .clamp(1, MAX_PER_ADDRESS_LIMIT);

        let mut queue = VecDeque::from([(source.to_string(), vec![source.to_string()], vec![])]);
        let mut cached_edges = HashMap::<String, Vec<FlowEdge>>::new();
        let mut discovered_edges = HashMap::<String, FlowEdge>::new();
        let mut paths = Vec::new();
        let mut expanded = HashSet::new();
        let mut truncated = false;

        while let Some((current, addresses, edge_ids)) = queue.pop_front() {
            if paths.len() >= path_limit {
                break;
            }
            if edge_ids.len() >= max_hops as usize {
                continue;
            }
            if expanded.len() >= 2_000 || discovered_edges.len() >= self.graph_max_edges {
                truncated = true;
                break;
            }

            let edges = if let Some(edges) = cached_edges.get(&current) {
                edges.clone()
            } else {
                let edges = self
                    .load_edges_for_address(&current, per_address_limit)
                    .await?;
                cached_edges.insert(current.clone(), edges.clone());
                expanded.insert(current.clone());
                edges
            };

            for edge in edges {
                let neighbor = match direction {
                    PathDirection::Outbound if edge.from_address == current => {
                        Some(edge.to_address.clone())
                    }
                    PathDirection::Outbound => None,
                    PathDirection::Both if edge.from_address == current => {
                        Some(edge.to_address.clone())
                    }
                    PathDirection::Both if edge.to_address == current => {
                        Some(edge.from_address.clone())
                    }
                    PathDirection::Both => None,
                };
                let Some(neighbor) = neighbor else {
                    continue;
                };
                if addresses.iter().any(|address| address == &neighbor) {
                    continue;
                }

                discovered_edges
                    .entry(edge.id.clone())
                    .or_insert_with(|| edge.clone());
                let mut next_addresses = addresses.clone();
                next_addresses.push(neighbor.clone());
                let mut next_edge_ids = edge_ids.clone();
                next_edge_ids.push(edge.id.clone());

                if neighbor == target {
                    paths.push(WalletPath {
                        hop_count: next_edge_ids.len(),
                        addresses: next_addresses,
                        edge_ids: next_edge_ids,
                    });
                    if paths.len() >= path_limit {
                        break;
                    }
                } else if next_edge_ids.len() < max_hops as usize {
                    if queue.len() >= 10_000 {
                        truncated = true;
                        break;
                    }
                    queue.push_back((neighbor, next_addresses, next_edge_ids));
                }
            }
        }

        let selected_edge_ids = paths
            .iter()
            .flat_map(|path| path.edge_ids.iter().cloned())
            .collect::<HashSet<_>>();
        let mut edges = discovered_edges
            .into_values()
            .filter(|edge| selected_edge_ids.is_empty() || selected_edge_ids.contains(&edge.id))
            .collect::<Vec<_>>();
        edges.sort_by_key(|edge| (edge.block_number, edge.id.clone()));
        let nodes = build_nodes(source, &edges);
        let projection = if project_to_neo4j {
            self.graph.project_edges(&edges).await?
        } else {
            ProjectionSummary::default()
        };

        Ok(PathSearchResult {
            network_id: self.network_id.clone(),
            source: source.to_string(),
            target: target.to_string(),
            direction: direction.as_str().to_string(),
            max_hops,
            paths,
            nodes,
            edges,
            expanded_addresses: expanded.len(),
            truncated,
            neo4j_projection: projection,
        })
    }

    pub async fn status(&self) -> anyhow::Result<PlatformStatus> {
        let checkpoint = self.load_checkpoint().await?;
        let row = self
            .clickhouse
            .query(
                r#"
                SELECT
                    (SELECT count() FROM transactions_canonical WHERE network_id = ?) AS transaction_count,
                    (SELECT count() FROM address_relationships_canonical WHERE network_id = ?) AS relationship_count,
                    (SELECT count() FROM semantic_aml_events_canonical WHERE network_id = ?) AS semantic_event_count
                "#,
            )
            .bind(&self.network_id)
            .bind(&self.network_id)
            .bind(&self.network_id)
            .fetch_one::<PlatformStatusRow>()
            .await
            .context("failed to load Ethereum platform status")?;

        Ok(PlatformStatus {
            network_id: self.network_id.clone(),
            last_synced_block: checkpoint,
            transaction_count: row.transaction_count.unwrap_or_default(),
            relationship_count: row.relationship_count.unwrap_or_default(),
            semantic_event_count: row.semantic_event_count.unwrap_or_default(),
        })
    }

    async fn load_edges_for_address(
        &self,
        address: &str,
        limit: u64,
    ) -> anyhow::Result<Vec<FlowEdge>> {
        let rows = self
            .clickhouse
            .query(
                r#"
                SELECT
                    relationship_id,
                    network_id,
                    tx_hash,
                    block_number,
                    block_timestamp_unix_ms,
                    from_address,
                    to_address,
                    asset_id,
                    token_id,
                    toString(amount) AS amount,
                    transfer_type
                FROM address_relationships_canonical
                WHERE network_id = ?
                  AND (from_address = ? OR to_address = ?)
                ORDER BY block_timestamp_unix_ms DESC, block_number DESC, relationship_id
                LIMIT ?
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .bind(address)
            .bind(limit)
            .fetch_all::<FlowEdgeRow>()
            .await
            .with_context(|| format!("failed to load Ethereum graph edges for {address}"))?;

        Ok(rows.into_iter().map(FlowEdge::from).collect())
    }

    async fn load_fingerprint(&self, address: &str) -> anyhow::Result<WalletFingerprint> {
        let transfer = self
            .clickhouse
            .query(
                r#"
                SELECT
                    count() AS transfer_count,
                    countIf(to_address = ?) AS inbound_transfers,
                    countIf(from_address = ?) AS outbound_transfers,
                    uniqExact(if(from_address = ?, to_address, from_address)) AS unique_counterparties,
                    min(block_timestamp_unix_ms) AS first_seen_unix_ms,
                    max(block_timestamp_unix_ms) AS last_seen_unix_ms
                FROM address_relationships_canonical
                WHERE network_id = ?
                  AND (from_address = ? OR to_address = ?)
                "#,
            )
            .bind(address)
            .bind(address)
            .bind(address)
            .bind(&self.network_id)
            .bind(address)
            .bind(address)
            .fetch_one::<FingerprintRow>()
            .await
            .with_context(|| format!("failed to build Ethereum transfer fingerprint for {address}"))?;

        let transactions = self
            .clickhouse
            .query(
                r#"
                SELECT
                    count() AS transaction_count,
                    countIf(status_known = 1 AND status = 0) AS failed_transactions,
                    countIf(input_data != '' AND input_data != '0x') AS contract_calls
                FROM transactions_canonical
                WHERE network_id = ?
                  AND (from_address = ? OR to_address = ? OR contract_address = ?)
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .bind(address)
            .bind(address)
            .fetch_one::<TransactionSummaryRow>()
            .await
            .with_context(|| {
                format!("failed to build Ethereum transaction fingerprint for {address}")
            })?;

        let semantic = self
            .clickhouse
            .query(
                r#"
                SELECT
                    countIf(event_type = 'swap') AS swap_events,
                    countIf(event_type = 'bridge_transfer') AS bridge_events,
                    countIf(startsWith(event_type, 'mixer_')) AS mixer_events,
                    countIf(startsWith(event_type, 'liquidity_')) AS liquidity_events
                FROM semantic_aml_events_canonical
                WHERE network_id = ? AND subject_address = ?
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .fetch_one::<SemanticCountRow>()
            .await
            .with_context(|| format!("failed to load Ethereum semantic counts for {address}"))?;

        Ok(WalletFingerprint {
            transaction_count: transactions.transaction_count,
            transfer_count: transfer.transfer_count,
            inbound_transfers: transfer.inbound_transfers,
            outbound_transfers: transfer.outbound_transfers,
            unique_counterparties: transfer.unique_counterparties,
            failed_transactions: transactions.failed_transactions,
            contract_calls: transactions.contract_calls,
            first_seen_unix_ms: transfer.first_seen_unix_ms,
            last_seen_unix_ms: transfer.last_seen_unix_ms,
            swap_events: semantic.swap_events,
            bridge_events: semantic.bridge_events,
            mixer_events: semantic.mixer_events,
            liquidity_events: semantic.liquidity_events,
        })
    }

    async fn load_asset_flows(&self, address: &str) -> anyhow::Result<Vec<AssetFlow>> {
        let rows = self
            .clickhouse
            .query(
                r#"
                SELECT
                    asset_id,
                    toString(sumIf(amount, to_address = ?)) AS inbound_amount,
                    toString(sumIf(amount, from_address = ?)) AS outbound_amount,
                    countIf(to_address = ?) AS inbound_transfers,
                    countIf(from_address = ?) AS outbound_transfers,
                    count() AS transfer_count
                FROM address_relationships_canonical
                WHERE network_id = ?
                  AND (from_address = ? OR to_address = ?)
                GROUP BY asset_id
                ORDER BY transfer_count DESC, asset_id
                LIMIT 100
                "#,
            )
            .bind(address)
            .bind(address)
            .bind(address)
            .bind(address)
            .bind(&self.network_id)
            .bind(address)
            .bind(address)
            .fetch_all::<AssetFlowRow>()
            .await
            .with_context(|| format!("failed to aggregate Ethereum asset flows for {address}"))?;

        Ok(rows
            .into_iter()
            .map(|row| AssetFlow {
                asset_id: row.asset_id,
                inbound_amount: row.inbound_amount,
                outbound_amount: row.outbound_amount,
                inbound_transfers: row.inbound_transfers,
                outbound_transfers: row.outbound_transfers,
                transfer_count: row.transfer_count,
            })
            .collect())
    }

    async fn load_semantic_events(
        &self,
        address: &str,
        limit: u64,
    ) -> anyhow::Result<Vec<SemanticEvent>> {
        let rows = self
            .clickhouse
            .query(
                r#"
                SELECT
                    event_id,
                    tx_hash,
                    block_number,
                    block_timestamp_unix_ms,
                    event_type,
                    protocol,
                    protocol_contract,
                    counterparty_address,
                    asset_in,
                    asset_out,
                    remote_network_id,
                    bridge_direction,
                    amount_in,
                    amount_out,
                    confidence,
                    evidence_json
                FROM semantic_aml_events_canonical
                WHERE network_id = ? AND subject_address = ?
                ORDER BY block_timestamp_unix_ms DESC, event_id
                LIMIT ?
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .bind(limit)
            .fetch_all::<SemanticEventRow>()
            .await
            .with_context(|| format!("failed to load Ethereum semantic events for {address}"))?;

        Ok(rows
            .into_iter()
            .map(|row| SemanticEvent {
                event_id: row.event_id,
                tx_hash: row.tx_hash,
                block_number: row.block_number,
                block_timestamp_unix_ms: row.block_timestamp_unix_ms,
                event_type: row.event_type,
                protocol: row.protocol,
                protocol_contract: row.protocol_contract,
                counterparty_address: row.counterparty_address,
                asset_in: row.asset_in,
                asset_out: row.asset_out,
                remote_network_id: row.remote_network_id,
                bridge_direction: row.bridge_direction,
                amount_in: row.amount_in,
                amount_out: row.amount_out,
                confidence: row.confidence,
                evidence_json: row.evidence_json,
            })
            .collect())
    }

    async fn load_entities(&self, address: &str) -> anyhow::Result<Vec<WalletEntity>> {
        let rows = self
            .clickhouse
            .query(
                r#"
                SELECT
                    entity_id,
                    entity_name,
                    entity_type,
                    address_role,
                    confidence,
                    risk_level,
                    is_exposure_seed,
                    seed_category
                FROM address_entities_active
                WHERE network_id = ? AND address = ?
                ORDER BY confidence DESC, entity_id
                LIMIT 25
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .fetch_all::<WalletEntityRow>()
            .await
            .with_context(|| format!("failed to load Ethereum entity context for {address}"))?;

        Ok(rows
            .into_iter()
            .map(|row| WalletEntity {
                entity_id: row.entity_id,
                entity_name: row.entity_name,
                entity_type: row.entity_type,
                address_role: row.address_role,
                confidence: row.confidence,
                risk_level: row.risk_level,
                is_exposure_seed: row.is_exposure_seed == 1,
                seed_category: row.seed_category,
            })
            .collect())
    }

    async fn load_clusters(&self, address: &str) -> anyhow::Result<Vec<WalletCluster>> {
        let rows = self
            .clickhouse
            .query(
                r#"
                SELECT cluster_id, entity_id, membership_type, confidence
                FROM address_cluster_memberships_active
                WHERE network_id = ? AND address = ?
                ORDER BY confidence DESC, cluster_id
                LIMIT 25
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .fetch_all::<WalletClusterRow>()
            .await
            .with_context(|| format!("failed to load Ethereum clusters for {address}"))?;

        Ok(rows
            .into_iter()
            .map(|row| WalletCluster {
                cluster_id: row.cluster_id,
                entity_id: row.entity_id,
                membership_type: row.membership_type,
                confidence: row.confidence,
            })
            .collect())
    }

    async fn load_exposure_paths(
        &self,
        address: &str,
        limit: u64,
    ) -> anyhow::Result<Vec<WalletExposurePath>> {
        let rows = self
            .clickhouse
            .query(
                r#"
                SELECT
                    path_id,
                    run_id,
                    seed_address,
                    seed_entity_id,
                    seed_category,
                    direction,
                    hop_count,
                    asset_id,
                    exposure_score,
                    service_mediated,
                    continuity_type,
                    path_addresses,
                    relationship_ids,
                    tx_hashes
                FROM address_exposure_best_paths
                WHERE network_id = ?
                  AND subject_address = ?
                  AND direction != 'SEED'
                  AND run_id =
                  (
                      SELECT argMax(run_id, completed_at_unix_ms)
                      FROM exposure_runs
                      WHERE network_id = ? AND status = 'COMPLETE'
                  )
                ORDER BY exposure_score DESC, hop_count
                LIMIT ?
                "#,
            )
            .bind(&self.network_id)
            .bind(address)
            .bind(&self.network_id)
            .bind(limit)
            .fetch_all::<WalletExposurePathRow>()
            .await
            .with_context(|| format!("failed to load Ethereum exposure paths for {address}"))?;

        Ok(rows
            .into_iter()
            .map(|row| WalletExposurePath {
                path_id: row.path_id,
                run_id: row.run_id,
                seed_address: row.seed_address,
                seed_entity_id: row.seed_entity_id,
                seed_category: row.seed_category,
                direction: row.direction,
                hop_count: row.hop_count,
                asset_id: row.asset_id,
                exposure_score: row.exposure_score,
                service_mediated: row.service_mediated == 1,
                continuity_type: row.continuity_type,
                path_addresses: row.path_addresses,
                relationship_ids: row.relationship_ids,
                tx_hashes: row.tx_hashes,
            })
            .collect())
    }

    async fn load_data_coverage(&self) -> anyhow::Result<DataCoverage> {
        let coverage = self
            .clickhouse
            .query(
                r#"
                SELECT
                    count() AS complete_blocks,
                    countIf(receipt_data_complete = 1) AS receipt_complete_blocks,
                    countIf(trace_data_complete = 1) AS trace_complete_blocks
                FROM
                (
                    SELECT *
                    FROM ingested_blocks
                    WHERE network_id = ? AND ingestion_status = 'complete'
                    ORDER BY updated_at DESC
                    LIMIT 1 BY network_id, block_number
                )
                "#,
            )
            .bind(&self.network_id)
            .fetch_one::<CoverageRow>()
            .await
            .context("failed to load Ethereum ingestion coverage")?;
        let last_synced_block = self.load_checkpoint().await?;

        Ok(DataCoverage {
            last_synced_block,
            complete_blocks: coverage.complete_blocks,
            receipt_complete_blocks: coverage.receipt_complete_blocks,
            trace_complete_blocks: coverage.trace_complete_blocks,
            internal_transfer_coverage: coverage.complete_blocks > 0
                && coverage.trace_complete_blocks == coverage.complete_blocks,
        })
    }

    async fn load_checkpoint(&self) -> anyhow::Result<u64> {
        let checkpoint = self
            .clickhouse
            .query(
                r#"
                SELECT last_synced_block
                FROM sync_state
                WHERE network_id = ?
                ORDER BY updated_at DESC
                LIMIT 1
                "#,
            )
            .bind(&self.network_id)
            .fetch_optional::<CheckpointRow>()
            .await
            .context("failed to load Ethereum checkpoint")?;
        Ok(checkpoint.map_or(0, |row| row.last_synced_block))
    }
}

impl From<FlowEdgeRow> for FlowEdge {
    fn from(row: FlowEdgeRow) -> Self {
        Self {
            id: row.relationship_id,
            network_id: row.network_id,
            tx_hash: row.tx_hash,
            block_number: row.block_number,
            block_timestamp_unix_ms: row.block_timestamp_unix_ms,
            from_address: row.from_address,
            to_address: row.to_address,
            asset_id: row.asset_id,
            token_id: row.token_id,
            amount: row.amount,
            transfer_type: row.transfer_type,
        }
    }
}

fn build_wallet_graph(address: &str, edges: Vec<FlowEdge>, limit: usize) -> WalletGraph {
    let nodes = build_nodes(address, &edges);
    WalletGraph {
        truncated: edges.len() >= limit,
        nodes,
        edges,
    }
}

fn build_nodes(focus: &str, edges: &[FlowEdge]) -> Vec<GraphNode> {
    let mut counts = HashMap::<String, (u64, u64)>::new();
    counts.entry(focus.to_string()).or_default();
    for edge in edges {
        counts.entry(edge.from_address.clone()).or_default().1 += 1;
        counts.entry(edge.to_address.clone()).or_default().0 += 1;
    }

    let mut nodes = counts
        .into_iter()
        .map(|(address, (inbound_edges, outbound_edges))| GraphNode {
            id: address.clone(),
            is_focus: address == focus,
            address,
            inbound_edges,
            outbound_edges,
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        right
            .is_focus
            .cmp(&left.is_focus)
            .then_with(|| {
                (right.inbound_edges + right.outbound_edges)
                    .cmp(&(left.inbound_edges + left.outbound_edges))
            })
            .then_with(|| left.address.cmp(&right.address))
    });
    nodes
}

fn counterparties(address: &str, edges: &[FlowEdge], limit: usize) -> Vec<CounterpartySummary> {
    let mut summaries = HashMap::<String, CounterpartySummary>::new();
    for edge in edges {
        let (counterparty, inbound) = if edge.to_address == address {
            (&edge.from_address, true)
        } else if edge.from_address == address {
            (&edge.to_address, false)
        } else {
            continue;
        };
        let summary =
            summaries
                .entry(counterparty.clone())
                .or_insert_with(|| CounterpartySummary {
                    address: counterparty.clone(),
                    inbound_transfers: 0,
                    outbound_transfers: 0,
                    total_transfers: 0,
                    last_seen_unix_ms: 0,
                });
        if inbound {
            summary.inbound_transfers += 1;
        } else {
            summary.outbound_transfers += 1;
        }
        summary.total_transfers += 1;
        summary.last_seen_unix_ms = summary.last_seen_unix_ms.max(edge.block_timestamp_unix_ms);
    }

    let mut values = summaries.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .total_transfers
            .cmp(&left.total_transfers)
            .then_with(|| right.last_seen_unix_ms.cmp(&left.last_seen_unix_ms))
    });
    values.truncate(limit);
    values
}

#[cfg(test)]
mod tests {
    use super::{FlowEdge, PathDirection, build_nodes, counterparties};

    fn edge(id: &str, from: &str, to: &str) -> FlowEdge {
        FlowEdge {
            id: id.to_string(),
            network_id: "eip155:1".to_string(),
            tx_hash: format!("0x{id}"),
            block_number: 1,
            block_timestamp_unix_ms: 1,
            from_address: from.to_string(),
            to_address: to.to_string(),
            asset_id: "eip155:1/native:eth".to_string(),
            token_id: String::new(),
            amount: "1".to_string(),
            transfer_type: "native_external".to_string(),
        }
    }

    #[test]
    fn graph_nodes_keep_directional_counts() {
        let nodes = build_nodes("a", &[edge("1", "a", "b"), edge("2", "c", "a")]);
        let focus = nodes.iter().find(|node| node.address == "a").unwrap();
        assert_eq!(focus.inbound_edges, 1);
        assert_eq!(focus.outbound_edges, 1);
    }

    #[test]
    fn counterparties_are_ranked_by_activity() {
        let values = counterparties(
            "a",
            &[
                edge("1", "a", "b"),
                edge("2", "b", "a"),
                edge("3", "a", "c"),
            ],
            10,
        );
        assert_eq!(values[0].address, "b");
        assert_eq!(values[0].total_transfers, 2);
    }

    #[test]
    fn path_direction_defaults_to_outbound() {
        assert_eq!(PathDirection::parse(None).unwrap(), PathDirection::Outbound);
        assert!(PathDirection::parse(Some("sideways")).is_err());
    }
}
