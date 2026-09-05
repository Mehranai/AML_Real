use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use clickhouse::Client;
use serde::Deserialize;

use super::client::Neo4jClient;
use super::edges::{merge_exchange_interaction, merge_transfer_edge};
use super::nodes::upsert_wallet_with_metadata;
use super::types::{
    ExchangeFlowSummary, FlowEdge, FlowNode, Neo4jVisualization, WalletFlowGraph, WalletPath,
    WalletPathGraph,
};

#[derive(Debug, Clone)]
struct ExchangeMetadata {
    exchange_name: String,
    exchange_role: String,
    confidence: f32,
}

#[derive(Debug, Clone)]
struct EntityMetadata {
    entity_name: String,
    entity_type: String,
    confidence: f32,
}

#[derive(Debug, Clone)]
struct ClusterMetadata {
    cluster_id: String,
    address_role: String,
    confidence: f32,
}

#[derive(Debug, Clone, Copy)]
enum PathSearchDirection {
    Outgoing,
    Incoming,
    Any,
}

impl PathSearchDirection {
    fn from_param(value: Option<&str>) -> Self {
        match value
            .unwrap_or("outgoing")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "incoming" | "reverse" => Self::Incoming,
            "any" | "both" | "undirected" => Self::Any,
            _ => Self::Outgoing,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Outgoing => "outgoing",
            Self::Incoming => "incoming",
            Self::Any => "any",
        }
    }
}

#[derive(Debug, Clone)]
struct PathSearchState {
    current: String,
    node_ids: Vec<String>,
    edges: Vec<FlowEdge>,
}

#[derive(Debug, Clone)]
struct FoundPath {
    node_ids: Vec<String>,
    edges: Vec<FlowEdge>,
}

#[derive(Debug, Clone)]
struct PathSearchResult {
    paths: Vec<FoundPath>,
    searched_node_count: usize,
    truncated: bool,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct RelationshipReadRow {
    relationship_id: String,
    from_address: String,
    to_address: String,
    token_address: String,
    tx_hash: String,
    block_number: u64,
    timestamp_unix: u64,
    amount_string: String,
    transfer_type: String,
    operation_type: String,
    protocol: String,
    initiator_address: String,
    target_address: String,
    contract_address: String,
    contract_type: String,
    transaction_fee_string: String,
    energy_usage_total: u64,
    net_usage: u64,
    execution_status: u8,
    transaction_type: String,
    transaction_subtype: String,
    classification_confidence: f32,
    classification_source: String,
    method_id: String,
    is_contract_call: u8,
    exchange_flow_type: String,
    exchange_name: String,
    exchange_confidence: f32,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct ExchangeMetadataRow {
    address: String,
    exchange_name: String,
    address_role: String,
    confidence: f32,
    #[serde(rename = "last_seen_block")]
    _last_seen_block: u64,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct EntityMetadataRow {
    address: String,
    entity_name: String,
    entity_type: String,
    confidence: f32,
}

#[derive(Debug, Clone, Deserialize, clickhouse::Row)]
struct ClusterMetadataRow {
    address: String,
    cluster_id: String,
    address_role: String,
    confidence: f32,
}

pub async fn build_wallet_flow_graph(
    clickhouse: Arc<Client>,
    neo4j: Option<&Neo4jClient>,
    address: &str,
    depth: u8,
    per_address_limit: u64,
) -> anyhow::Result<WalletFlowGraph> {
    if let Some(neo4j) = neo4j {
        neo4j.ensure_schema().await?;
    }

    let depth = depth.clamp(1, 6);
    let per_address_limit = per_address_limit.clamp(1, 2_000);

    let edges =
        load_relationship_neighborhood(clickhouse.clone(), address, depth, per_address_limit)
            .await?;

    let mut node_ids = HashSet::<String>::new();
    for edge in &edges {
        node_ids.insert(edge.from.clone());
        node_ids.insert(edge.to.clone());
    }
    node_ids.insert(address.to_string());

    let (exchange_metadata, entity_metadata, cluster_metadata) =
        load_node_metadata(&clickhouse, &node_ids).await?;

    // Write Nodes to Neo4j
    let mut nodes = Vec::<FlowNode>::new();
    for node_id in node_ids {
        let node = build_flow_node(
            &node_id,
            exchange_metadata.get(&node_id),
            entity_metadata.get(&node_id),
            cluster_metadata.get(&node_id),
            &edges,
        );

        if let Some(neo4j) = neo4j {
            upsert_wallet_with_metadata(
                neo4j,
                &node.id,
                &node.label,
                &node.node_type,
                node.entity_name.as_deref(),
                node.entity_type.as_deref(),
                node.exchange_name.as_deref(),
                node.exchange_role.as_deref(),
                node.cluster_id.as_deref(),
                node.cluster_role.as_deref(),
                node.confidence,
            )
            .await?;
        }

        nodes.push(node);
    }

    // Write Transfer Edge to Neo4j
    if let Some(neo4j) = neo4j {
        for edge in &edges {
            merge_transfer_edge(neo4j, edge).await?;
        }
    }

    let incoming_origins = incoming_origin_nodes(address, &nodes, &edges);

    let exchange_interactions = exchange_summaries(address, &edges, &exchange_metadata);

    if let Some(neo4j) = neo4j {
        for interaction in &exchange_interactions {
            merge_exchange_interaction(
                neo4j,
                address,
                &interaction.exchange_name,
                &interaction.address,
                &interaction.exchange_role,
                &interaction.direction,
                &interaction.tx_hash,
                &interaction.token_address,
                &interaction.amount,
                interaction.block_number,
                interaction.confidence,
            )
            .await?;
        }
    }

    let was_projected = neo4j.is_some();
    let neo4j_visualization = Neo4jVisualization {
        browser_url: "http://localhost:8474/browser/".to_string(),
        cypher: neo4j_browser_cypher(address, depth, per_address_limit),
        imported_wallet_nodes: if was_projected { nodes.len() } else { 0 },
        imported_transfer_edges: if was_projected { edges.len() } else { 0 },
        imported_exchange_interactions: if was_projected {
            exchange_interactions.len()
        } else {
            0
        },
    };

    Ok(WalletFlowGraph {
        address: address.to_string(),
        depth,
        nodes,
        edges,
        incoming_origins,
        exchange_interactions,
        neo4j: neo4j_visualization,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn build_wallet_path_graph(
    clickhouse: Arc<Client>,
    neo4j: Option<&Neo4jClient>,
    source_address: &str,
    target_address: &str,
    max_depth: Option<u8>,
    max_paths: Option<usize>,
    per_address_limit: Option<u64>,
    direction: Option<&str>,
) -> anyhow::Result<WalletPathGraph> {
    if let Some(neo4j) = neo4j {
        neo4j.ensure_schema().await?;
    }

    let max_depth = max_depth.unwrap_or(10).clamp(1, 10);
    let max_paths = max_paths.unwrap_or(20).clamp(1, 50);
    let per_address_limit = per_address_limit.unwrap_or(500).clamp(1, 2_000);
    let direction = PathSearchDirection::from_param(direction);

    let search_result = find_wallet_paths(
        clickhouse.clone(),
        source_address,
        target_address,
        max_depth,
        max_paths,
        per_address_limit,
        direction,
    )
    .await?;

    let mut edge_by_id = HashMap::<String, FlowEdge>::new();
    let mut node_ids =
        HashSet::<String>::from([source_address.to_string(), target_address.to_string()]);
    let mut paths = Vec::<WalletPath>::new();

    for (index, path) in search_result.paths.iter().enumerate() {
        for node_id in &path.node_ids {
            node_ids.insert(node_id.clone());
        }

        for edge in &path.edges {
            node_ids.insert(edge.from.clone());
            node_ids.insert(edge.to.clone());
            edge_by_id
                .entry(edge.id.clone())
                .or_insert_with(|| edge.clone());
        }

        paths.push(WalletPath {
            path_index: index + 1,
            hop_count: path.edges.len(),
            node_ids: path.node_ids.clone(),
            edge_ids: path.edges.iter().map(|edge| edge.id.clone()).collect(),
        });
    }

    let edges = edge_by_id.into_values().collect::<Vec<_>>();
    let (exchange_metadata, entity_metadata, cluster_metadata) =
        load_node_metadata(&clickhouse, &node_ids).await?;

    let mut nodes = Vec::<FlowNode>::new();
    for node_id in node_ids {
        let node = build_flow_node(
            &node_id,
            exchange_metadata.get(&node_id),
            entity_metadata.get(&node_id),
            cluster_metadata.get(&node_id),
            &edges,
        );

        if let Some(neo4j) = neo4j {
            upsert_wallet_with_metadata(
                neo4j,
                &node.id,
                &node.label,
                &node.node_type,
                node.entity_name.as_deref(),
                node.entity_type.as_deref(),
                node.exchange_name.as_deref(),
                node.exchange_role.as_deref(),
                node.cluster_id.as_deref(),
                node.cluster_role.as_deref(),
                node.confidence,
            )
            .await?;
        }

        nodes.push(node);
    }

    if let Some(neo4j) = neo4j {
        for edge in &edges {
            merge_transfer_edge(neo4j, edge).await?;
        }
    }

    let was_projected = neo4j.is_some();
    let neo4j_visualization = Neo4jVisualization {
        browser_url: "http://localhost:8474/browser/".to_string(),
        cypher: neo4j_path_cypher(
            source_address,
            target_address,
            max_depth,
            max_paths,
            direction,
        ),
        imported_wallet_nodes: if was_projected { nodes.len() } else { 0 },
        imported_transfer_edges: if was_projected { edges.len() } else { 0 },
        imported_exchange_interactions: 0,
    };

    Ok(WalletPathGraph {
        address: source_address.to_string(),
        source_address: source_address.to_string(),
        target_address: target_address.to_string(),
        max_depth,
        direction: direction.as_str().to_string(),
        path_count: paths.len(),
        searched_node_count: search_result.searched_node_count,
        truncated: search_result.truncated,
        nodes,
        edges,
        paths,
        exchange_interactions: Vec::new(),
        neo4j: neo4j_visualization,
    })
}

async fn find_wallet_paths(
    clickhouse: Arc<Client>,
    source_address: &str,
    target_address: &str,
    max_depth: u8,
    max_paths: usize,
    per_address_limit: u64,
    direction: PathSearchDirection,
) -> anyhow::Result<PathSearchResult> {
    let mut queue = VecDeque::<PathSearchState>::from([PathSearchState {
        current: source_address.to_string(),
        node_ids: vec![source_address.to_string()],
        edges: Vec::new(),
    }]);
    let mut found_paths = Vec::<FoundPath>::new();
    let mut searched_node_count = 0usize;
    let mut truncated = false;
    let max_expanded_nodes = 25_000usize;

    while let Some(state) = queue.pop_front() {
        if state.edges.len() >= usize::from(max_depth) {
            continue;
        }

        if searched_node_count >= max_expanded_nodes {
            truncated = true;
            break;
        }

        searched_node_count += 1;
        let rows = load_path_relationships_for_address(
            &clickhouse,
            &state.current,
            direction,
            per_address_limit,
        )
        .await?;
        if rows.len() as u64 >= per_address_limit {
            truncated = true;
        }

        for row in rows {
            let edge = relationship_row_to_edge(row);

            let Some(next_address) = next_path_address(&edge, &state.current, direction) else {
                continue;
            };

            if state.node_ids.iter().any(|node| node == &next_address) {
                continue;
            }

            let mut next_node_ids = state.node_ids.clone();
            next_node_ids.push(next_address.clone());

            let mut next_edges = state.edges.clone();
            next_edges.push(edge);

            if next_address == target_address {
                found_paths.push(FoundPath {
                    node_ids: next_node_ids,
                    edges: next_edges,
                });

                if found_paths.len() >= max_paths {
                    truncated = true;
                    break;
                }
            } else {
                queue.push_back(PathSearchState {
                    current: next_address,
                    node_ids: next_node_ids,
                    edges: next_edges,
                });
            }
        }

        if found_paths.len() >= max_paths {
            break;
        }
    }

    Ok(PathSearchResult {
        paths: found_paths,
        searched_node_count,
        truncated,
    })
}

fn next_path_address(
    edge: &FlowEdge,
    current: &str,
    direction: PathSearchDirection,
) -> Option<String> {
    match direction {
        PathSearchDirection::Outgoing => {
            if edge.from == current {
                Some(edge.to.clone())
            } else {
                None
            }
        }
        PathSearchDirection::Incoming => {
            if edge.to == current {
                Some(edge.from.clone())
            } else {
                None
            }
        }
        PathSearchDirection::Any => {
            if edge.from == current {
                Some(edge.to.clone())
            } else if edge.to == current {
                Some(edge.from.clone())
            } else {
                None
            }
        }
    }
}

// frontier: آدرس‌هایی که در عمق فعلی باید query شوند.
// visited: جلوگیری از پردازش دوباره node.
// edge_ids: جلوگیری از edge تکراری.
// next_frontier: آدرس‌های عمق بعد.

async fn load_relationship_neighborhood(
    clickhouse: Arc<Client>,
    address: &str,
    depth: u8,
    edge_limit: u64,
) -> anyhow::Result<Vec<FlowEdge>> {
    let mut frontier = vec![address.to_string()];
    let mut visited = HashSet::<String>::new();
    let mut edge_ids = HashSet::<String>::new();
    let mut edges = Vec::<FlowEdge>::new();

    for current_depth in 0..depth {
        frontier.retain(|address| visited.insert(address.clone()));
        if frontier.is_empty() || edges.len() as u64 >= edge_limit {
            break;
        }

        let remaining = edge_limit.saturating_sub(edges.len() as u64);
        let rows = load_relationships_for_addresses(&clickhouse, &frontier, remaining).await?;
        let mut next_frontier = Vec::new();
        let mut next_seen = HashSet::new();

        for row in rows {
            let edge = relationship_row_to_edge(row);

            if edge_ids.insert(edge.id.clone()) {
                if current_depth + 1 < depth {
                    if !visited.contains(&edge.from) && next_seen.insert(edge.from.clone()) {
                        next_frontier.push(edge.from.clone());
                    }

                    if !visited.contains(&edge.to) && next_seen.insert(edge.to.clone()) {
                        next_frontier.push(edge.to.clone());
                    }
                }

                edges.push(edge);
                if edges.len() as u64 >= edge_limit {
                    break;
                }
            }
        }

        frontier = next_frontier;
    }

    Ok(edges)
}

async fn load_relationships_for_addresses(
    clickhouse: &Client,
    addresses: &[String],
    limit: u64,
) -> anyhow::Result<Vec<RelationshipReadRow>> {
    if addresses.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let rows = clickhouse
        .query(
            r#"
            SELECT
                ar.relationship_id AS relationship_id,
                ar.from_address AS from_address,
                ar.to_address AS to_address,
                ar.token_address AS token_address,
                ar.tx_hash AS tx_hash,
                ar.block_number AS block_number,
                toUInt64(ar.timestamp) AS timestamp_unix,
                toString(ar.amount) AS amount_string,
                ar.transfer_type AS transfer_type,
                ar.operation_type AS operation_type,
                ar.protocol AS protocol,
                tx.initiator_address AS initiator_address,
                tx.target_address AS target_address,
                tx.contract_address AS contract_address,
                tx.contract_type AS contract_type,
                toString(tx.transaction_fee) AS transaction_fee_string,
                tx.energy_usage_total AS energy_usage_total,
                tx.net_usage AS net_usage,
                tx.execution_status AS execution_status,
                ar.transaction_type AS transaction_type,
                ar.transaction_subtype AS transaction_subtype,
                ar.classification_confidence AS classification_confidence,
                ar.classification_source AS classification_source,
                ar.method_id AS method_id,
                ar.is_contract_call AS is_contract_call,
                ifNull(ef.exchange_flow_type, '') AS exchange_flow_type,
                ifNull(ef.exchange_name, '') AS exchange_name,
                ifNull(ef.exchange_confidence, toFloat32(0)) AS exchange_confidence
            FROM address_relationships_canonical AS ar
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    initiator_address,
                    target_address,
                    contract_address,
                    contract_type,
                    fee AS transaction_fee,
                    energy_usage_total,
                    net_usage,
                    status AS execution_status
                FROM transactions_canonical
            ) AS tx ON tx.tx_hash = ar.tx_hash
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount,
                    any(exchange_name) AS exchange_name,
                    any(flow_type) AS exchange_flow_type,
                    max(confidence) AS exchange_confidence
                FROM exchange_flows_canonical
                GROUP BY
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount
            ) AS ef
                ON ar.tx_hash = ef.tx_hash
                AND ar.from_address = ef.from_address
                AND ar.to_address = ef.to_address
                AND ar.token_address = ef.token_address
                AND ar.amount = ef.amount
            WHERE ar.from_address IN ? OR ar.to_address IN ?
            ORDER BY ar.block_number DESC
            LIMIT ?
            "#,
        )
        .bind(addresses)
        .bind(addresses)
        .bind(limit)
        .fetch_all::<RelationshipReadRow>()
        .await?;

    Ok(rows)
}

async fn load_path_relationships_for_address(
    clickhouse: &Client,
    address: &str,
    direction: PathSearchDirection,
    limit: u64,
) -> anyhow::Result<Vec<RelationshipReadRow>> {
    let query = match direction {
        PathSearchDirection::Outgoing => {
            r#"
            SELECT
                ar.relationship_id AS relationship_id,
                ar.from_address AS from_address,
                ar.to_address AS to_address,
                ar.token_address AS token_address,
                ar.tx_hash AS tx_hash,
                ar.block_number AS block_number,
                toUInt64(ar.timestamp) AS timestamp_unix,
                toString(ar.amount) AS amount_string,
                ar.transfer_type AS transfer_type,
                ar.operation_type AS operation_type,
                ar.protocol AS protocol,
                tx.initiator_address AS initiator_address,
                tx.target_address AS target_address,
                tx.contract_address AS contract_address,
                tx.contract_type AS contract_type,
                toString(tx.transaction_fee) AS transaction_fee_string,
                tx.energy_usage_total AS energy_usage_total,
                tx.net_usage AS net_usage,
                tx.execution_status AS execution_status,
                ar.transaction_type AS transaction_type,
                ar.transaction_subtype AS transaction_subtype,
                ar.classification_confidence AS classification_confidence,
                ar.classification_source AS classification_source,
                ar.method_id AS method_id,
                ar.is_contract_call AS is_contract_call,
                ifNull(ef.exchange_flow_type, '') AS exchange_flow_type,
                ifNull(ef.exchange_name, '') AS exchange_name,
                ifNull(ef.exchange_confidence, toFloat32(0)) AS exchange_confidence
            FROM address_relationships_canonical AS ar
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    initiator_address,
                    target_address,
                    contract_address,
                    contract_type,
                    fee AS transaction_fee,
                    energy_usage_total,
                    net_usage,
                    status AS execution_status
                FROM transactions_canonical
            ) AS tx ON tx.tx_hash = ar.tx_hash
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount,
                    any(exchange_name) AS exchange_name,
                    any(flow_type) AS exchange_flow_type,
                    max(confidence) AS exchange_confidence
                FROM exchange_flows_canonical
                GROUP BY
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount
            ) AS ef
                ON ar.tx_hash = ef.tx_hash
                AND ar.from_address = ef.from_address
                AND ar.to_address = ef.to_address
                AND ar.token_address = ef.token_address
                AND ar.amount = ef.amount
            WHERE ar.from_address = ?
            ORDER BY ar.block_number DESC
            LIMIT ?
            "#
        }
        PathSearchDirection::Incoming => {
            r#"
            SELECT
                ar.relationship_id AS relationship_id,
                ar.from_address AS from_address,
                ar.to_address AS to_address,
                ar.token_address AS token_address,
                ar.tx_hash AS tx_hash,
                ar.block_number AS block_number,
                toUInt64(ar.timestamp) AS timestamp_unix,
                toString(ar.amount) AS amount_string,
                ar.transfer_type AS transfer_type,
                ar.operation_type AS operation_type,
                ar.protocol AS protocol,
                tx.initiator_address AS initiator_address,
                tx.target_address AS target_address,
                tx.contract_address AS contract_address,
                tx.contract_type AS contract_type,
                toString(tx.transaction_fee) AS transaction_fee_string,
                tx.energy_usage_total AS energy_usage_total,
                tx.net_usage AS net_usage,
                tx.execution_status AS execution_status,
                ar.transaction_type AS transaction_type,
                ar.transaction_subtype AS transaction_subtype,
                ar.classification_confidence AS classification_confidence,
                ar.classification_source AS classification_source,
                ar.method_id AS method_id,
                ar.is_contract_call AS is_contract_call,
                ifNull(ef.exchange_flow_type, '') AS exchange_flow_type,
                ifNull(ef.exchange_name, '') AS exchange_name,
                ifNull(ef.exchange_confidence, toFloat32(0)) AS exchange_confidence
            FROM address_relationships_canonical AS ar
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    initiator_address,
                    target_address,
                    contract_address,
                    contract_type,
                    fee AS transaction_fee,
                    energy_usage_total,
                    net_usage,
                    status AS execution_status
                FROM transactions_canonical
            ) AS tx ON tx.tx_hash = ar.tx_hash
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount,
                    any(exchange_name) AS exchange_name,
                    any(flow_type) AS exchange_flow_type,
                    max(confidence) AS exchange_confidence
                FROM exchange_flows_canonical
                GROUP BY
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount
            ) AS ef
                ON ar.tx_hash = ef.tx_hash
                AND ar.from_address = ef.from_address
                AND ar.to_address = ef.to_address
                AND ar.token_address = ef.token_address
                AND ar.amount = ef.amount
            WHERE ar.to_address = ?
            ORDER BY ar.block_number DESC
            LIMIT ?
            "#
        }
        PathSearchDirection::Any => {
            r#"
            SELECT
                ar.relationship_id AS relationship_id,
                ar.from_address AS from_address,
                ar.to_address AS to_address,
                ar.token_address AS token_address,
                ar.tx_hash AS tx_hash,
                ar.block_number AS block_number,
                toUInt64(ar.timestamp) AS timestamp_unix,
                toString(ar.amount) AS amount_string,
                ar.transfer_type AS transfer_type,
                ar.operation_type AS operation_type,
                ar.protocol AS protocol,
                tx.initiator_address AS initiator_address,
                tx.target_address AS target_address,
                tx.contract_address AS contract_address,
                tx.contract_type AS contract_type,
                toString(tx.transaction_fee) AS transaction_fee_string,
                tx.energy_usage_total AS energy_usage_total,
                tx.net_usage AS net_usage,
                tx.execution_status AS execution_status,
                ar.transaction_type AS transaction_type,
                ar.transaction_subtype AS transaction_subtype,
                ar.classification_confidence AS classification_confidence,
                ar.classification_source AS classification_source,
                ar.method_id AS method_id,
                ar.is_contract_call AS is_contract_call,
                ifNull(ef.exchange_flow_type, '') AS exchange_flow_type,
                ifNull(ef.exchange_name, '') AS exchange_name,
                ifNull(ef.exchange_confidence, toFloat32(0)) AS exchange_confidence
            FROM address_relationships_canonical AS ar
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    initiator_address,
                    target_address,
                    contract_address,
                    contract_type,
                    fee AS transaction_fee,
                    energy_usage_total,
                    net_usage,
                    status AS execution_status
                FROM transactions_canonical
            ) AS tx ON tx.tx_hash = ar.tx_hash
            LEFT JOIN
            (
                SELECT
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount,
                    any(exchange_name) AS exchange_name,
                    any(flow_type) AS exchange_flow_type,
                    max(confidence) AS exchange_confidence
                FROM exchange_flows_canonical
                GROUP BY
                    tx_hash,
                    from_address,
                    to_address,
                    token_address,
                    amount
            ) AS ef
                ON ar.tx_hash = ef.tx_hash
                AND ar.from_address = ef.from_address
                AND ar.to_address = ef.to_address
                AND ar.token_address = ef.token_address
                AND ar.amount = ef.amount
            WHERE ar.from_address = ? OR ar.to_address = ?
            ORDER BY ar.block_number DESC
            LIMIT ?
            "#
        }
    };

    let mut query = clickhouse.query(query).bind(address);
    if matches!(direction, PathSearchDirection::Any) {
        query = query.bind(address);
    }

    let rows = query.bind(limit).fetch_all::<RelationshipReadRow>().await?;

    Ok(rows)
}

async fn load_node_metadata(
    clickhouse: &Client,
    node_ids: &HashSet<String>,
) -> anyhow::Result<(
    HashMap<String, ExchangeMetadata>,
    HashMap<String, EntityMetadata>,
    HashMap<String, ClusterMetadata>,
)> {
    if node_ids.is_empty() {
        return Ok((HashMap::new(), HashMap::new(), HashMap::new()));
    }

    let addresses = node_ids.iter().cloned().collect::<Vec<_>>();
    let exchange_rows = clickhouse
        .query(
            r#"
            SELECT
                address,
                exchange_name,
                address_role,
                confidence,
                last_seen_block
            FROM exchange_addresses FINAL
            WHERE address IN ? AND is_active = 1
            ORDER BY address, confidence DESC, last_seen_block DESC
            LIMIT 1 BY address
            "#,
        )
        .bind(&addresses)
        .fetch_all::<ExchangeMetadataRow>()
        .await?;
    let entity_rows = clickhouse
        .query(
            r#"
            SELECT
                address,
                entity_name,
                entity_type,
                confidence
            FROM address_entity FINAL
            WHERE address IN ?
              AND is_active = 1
            ORDER BY address, confidence DESC, created_at DESC
            LIMIT 1 BY address
            "#,
        )
        .bind(&addresses)
        .fetch_all::<EntityMetadataRow>()
        .await?;
    let cluster_rows = clickhouse
        .query(
            r#"
            SELECT address, cluster_id, address_role, confidence
            FROM address_cluster_memberships FINAL
            WHERE chain = 'tron'
              AND address IN ?
              AND is_active = 1
            ORDER BY address, confidence DESC, cluster_version DESC
            LIMIT 1 BY address
            "#,
        )
        .bind(&addresses)
        .fetch_all::<ClusterMetadataRow>()
        .await?;

    let exchange_metadata = exchange_rows
        .into_iter()
        .map(|row| {
            (
                row.address,
                ExchangeMetadata {
                    exchange_name: row.exchange_name,
                    exchange_role: row.address_role,
                    confidence: row.confidence,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let entity_metadata = entity_rows
        .into_iter()
        .map(|row| {
            (
                row.address,
                EntityMetadata {
                    entity_name: row.entity_name,
                    entity_type: row.entity_type,
                    confidence: row.confidence,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let cluster_metadata = cluster_rows
        .into_iter()
        .map(|row| {
            (
                row.address,
                ClusterMetadata {
                    cluster_id: row.cluster_id,
                    address_role: row.address_role,
                    confidence: row.confidence,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    Ok((exchange_metadata, entity_metadata, cluster_metadata))
}

fn relationship_row_to_edge(row: RelationshipReadRow) -> FlowEdge {
    let exchange_flow_type = non_empty_string(row.exchange_flow_type);
    let exchange_name = non_empty_string(row.exchange_name);
    let exchange_confidence = if row.exchange_confidence > 0.0 {
        Some(row.exchange_confidence)
    } else {
        None
    };
    let operation_type = non_empty_string(row.operation_type)
        .or_else(|| exchange_flow_type.clone())
        .unwrap_or_else(|| row.transfer_type.clone());
    let relationship_type = neo4j_relationship_type(&operation_type, &row.transfer_type);

    FlowEdge {
        id: row.relationship_id,
        from: row.from_address,
        to: row.to_address,
        token_address: row.token_address,
        tx_hash: row.tx_hash,
        block_number: row.block_number,
        timestamp: row.timestamp_unix,
        amount: row.amount_string,
        transfer_type: row.transfer_type,
        operation_type,
        relationship_type,
        protocol: row.protocol,
        initiator_address: row.initiator_address,
        target_address: row.target_address,
        contract_address: row.contract_address,
        contract_type: row.contract_type,
        transaction_fee: row.transaction_fee_string,
        energy_usage_total: row.energy_usage_total,
        net_usage: row.net_usage,
        execution_status: row.execution_status,
        transaction_type: row.transaction_type,
        transaction_subtype: row.transaction_subtype,
        classification_confidence: row.classification_confidence,
        classification_source: row.classification_source,
        method_id: row.method_id,
        is_contract_call: row.is_contract_call == 1,
        exchange_flow_type,
        exchange_name,
        exchange_confidence,
    }
}

fn build_flow_node(
    node_id: &str,
    exchange: Option<&ExchangeMetadata>,
    entity: Option<&EntityMetadata>,
    cluster: Option<&ClusterMetadata>,
    edges: &[FlowEdge],
) -> FlowNode {
    let cluster_id = cluster.map(|metadata| metadata.cluster_id.clone());
    let cluster_role = cluster.map(|metadata| metadata.address_role.clone());
    let cluster_confidence = cluster.map(|metadata| metadata.confidence);
    if let Some(exchange) = exchange {
        return FlowNode {
            id: node_id.to_string(),
            label: format!("{} ({})", exchange.exchange_name, exchange.exchange_role),
            node_type: "exchange_wallet".to_string(),
            entity_name: Some(
                entity
                    .map(|metadata| metadata.entity_name.clone())
                    .unwrap_or_else(|| exchange.exchange_name.clone()),
            ),
            entity_type: Some(
                entity
                    .map(|metadata| metadata.entity_type.clone())
                    .unwrap_or_else(|| {
                        format!("exchange_{}", exchange.exchange_role.to_lowercase())
                    }),
            ),
            exchange_name: Some(exchange.exchange_name.clone()),
            exchange_role: Some(exchange.exchange_role.clone()),
            cluster_id,
            cluster_role,
            confidence: Some(
                exchange
                    .confidence
                    .max(entity.map_or(0.0, |metadata| metadata.confidence))
                    .max(cluster_confidence.unwrap_or(0.0)),
            ),
        };
    }

    if let Some(entity) = entity {
        return FlowNode {
            id: node_id.to_string(),
            label: format!("{} ({})", entity.entity_name, entity.entity_type),
            node_type: entity.entity_type.clone(),
            entity_name: Some(entity.entity_name.clone()),
            entity_type: Some(entity.entity_type.clone()),
            exchange_name: None,
            exchange_role: None,
            cluster_id,
            cluster_role,
            confidence: Some(entity.confidence.max(cluster_confidence.unwrap_or(0.0))),
        };
    }

    let node_type = infer_node_type(node_id, edges);
    FlowNode {
        id: node_id.to_string(),
        label: node_label(node_id, &node_type),
        node_type,
        entity_name: None,
        entity_type: None,
        exchange_name: None,
        exchange_role: None,
        cluster_id,
        cluster_role,
        confidence: cluster_confidence,
    }
}

fn infer_node_type(node_id: &str, edges: &[FlowEdge]) -> String {
    if node_id.eq_ignore_ascii_case("bridge") {
        return "bridge".to_string();
    }

    if node_id.eq_ignore_ascii_case("mint") {
        return "mint".to_string();
    }

    if node_id.eq_ignore_ascii_case("burn") {
        return "burn".to_string();
    }

    if edges
        .iter()
        .any(|edge| edge.transfer_type == "bridge" && edge.protocol == node_id)
    {
        return "bridge".to_string();
    }

    if edges
        .iter()
        .any(|edge| edge.transfer_type == "swap" && edge.to == node_id)
    {
        return "protocol".to_string();
    }

    "wallet".to_string()
}

fn node_label(node_id: &str, node_type: &str) -> String {
    match node_type {
        "bridge" => "Bridge".to_string(),
        "mint" => "Mint".to_string(),
        "burn" => "Burn".to_string(),
        "protocol" => {
            if node_id.is_empty() {
                "Protocol".to_string()
            } else {
                node_id.to_string()
            }
        }
        _ => short_address(node_id),
    }
}

fn non_empty_string(value: String) -> Option<String> {
    let value = value.trim().to_string();

    if value.is_empty() { None } else { Some(value) }
}

fn neo4j_relationship_type(operation_type: &str, transfer_type: &str) -> String {
    match operation_type {
        "swap" => "SWAP",
        "bridge" => "BRIDGE",
        "deposit" => "EXCHANGE_DEPOSIT",
        "withdrawal" => "EXCHANGE_WITHDRAWAL",
        "sweep" => "EXCHANGE_SWEEP",
        "internal_transfer" => "INTERNAL_TRANSFER",
        "liquidity_add" => "LIQUIDITY_ADD",
        "liquidity_remove" => "LIQUIDITY_REMOVE",
        "mint" => "MINT",
        "burn" => "BURN",
        operation if operation.starts_with("exchange_to_exchange") => "EXCHANGE_TRANSFER",
        _ => match transfer_type {
            "native_transfer" => "NATIVE_TRANSFER",
            "trc10_transfer" => "TRC10_TRANSFER",
            "trc20_transfer" => "TRC20_TRANSFER",
            "internal_transfer" => "INTERNAL_TRANSFER",
            "mint" => "MINT",
            "burn" => "BURN",
            _ => "MONEY_FLOW",
        },
    }
    .to_string()
}

fn incoming_origin_nodes(address: &str, nodes: &[FlowNode], edges: &[FlowEdge]) -> Vec<FlowNode> {
    let direct_senders = edges
        .iter()
        .filter(|edge| edge.to == address)
        .map(|edge| edge.from.as_str())
        .collect::<HashSet<_>>();

    nodes
        .iter()
        .filter(|node| direct_senders.contains(node.id.as_str()))
        .cloned()
        .collect()
}

fn exchange_summaries(
    address: &str,
    edges: &[FlowEdge],
    metadata: &HashMap<String, ExchangeMetadata>,
) -> Vec<ExchangeFlowSummary> {
    let mut summaries = Vec::<ExchangeFlowSummary>::new();

    for edge in edges {
        if edge.from == address
            && let Some(exchange) = metadata.get(&edge.to)
        {
            summaries.push(summary_from_edge(edge, &edge.to, exchange, "outgoing"));
        }

        if edge.to == address
            && let Some(exchange) = metadata.get(&edge.from)
        {
            summaries.push(summary_from_edge(edge, &edge.from, exchange, "incoming"));
        }
    }

    summaries
}

fn summary_from_edge(
    edge: &FlowEdge,
    exchange_address: &str,
    exchange: &ExchangeMetadata,
    direction: &str,
) -> ExchangeFlowSummary {
    ExchangeFlowSummary {
        exchange_name: exchange.exchange_name.clone(),
        exchange_role: exchange.exchange_role.clone(),
        address: exchange_address.to_string(),
        direction: direction.to_string(),
        tx_hash: edge.tx_hash.clone(),
        token_address: edge.token_address.clone(),
        amount: edge.amount.clone(),
        block_number: edge.block_number,
        operation_type: edge.operation_type.clone(),
        confidence: exchange.confidence,
    }
}

fn short_address(address: &str) -> String {
    if address.len() <= 12 {
        return address.to_string();
    }

    format!("{}...{}", &address[..6], &address[address.len() - 4..])
}

const FLOW_RELATIONSHIP_TYPES: &str = "NATIVE_TRANSFER|TRC10_TRANSFER|TRC20_TRANSFER|INTERNAL_TRANSFER|MONEY_FLOW|SWAP|BRIDGE|LIQUIDITY_ADD|LIQUIDITY_REMOVE|MINT|BURN|EXCHANGE_DEPOSIT|EXCHANGE_WITHDRAWAL|EXCHANGE_SWEEP|EXCHANGE_TRANSFER";

pub fn neo4j_browser_cypher(address: &str, depth: u8, limit: u64) -> String {
    let safe_depth = depth.clamp(1, 6);
    let safe_limit = limit.clamp(1, 2_000);
    let escaped_address = address.replace('\\', "\\\\").replace('\'', "\\'");

    format!(
        "MATCH p = (w:Wallet {{ chain: 'tron', address: '{}' }})-[:{}*1..{}]-(n:Wallet) RETURN p LIMIT {}",
        escaped_address, FLOW_RELATIONSHIP_TYPES, safe_depth, safe_limit
    )
}

fn neo4j_path_cypher(
    source_address: &str,
    target_address: &str,
    max_depth: u8,
    limit: usize,
    direction: PathSearchDirection,
) -> String {
    let safe_depth = max_depth.clamp(1, 10);
    let safe_limit = limit.clamp(1, 50);
    let source = source_address.replace('\\', "\\\\").replace('\'', "\\'");
    let target = target_address.replace('\\', "\\\\").replace('\'', "\\'");

    match direction {
        PathSearchDirection::Incoming => format!(
            "MATCH p = (source:Wallet {{ chain: 'tron', address: '{}' }})<-[:{}*1..{}]-(target:Wallet {{ chain: 'tron', address: '{}' }}) RETURN p LIMIT {}",
            source, FLOW_RELATIONSHIP_TYPES, safe_depth, target, safe_limit
        ),
        PathSearchDirection::Any => format!(
            "MATCH p = (source:Wallet {{ chain: 'tron', address: '{}' }})-[:{}*1..{}]-(target:Wallet {{ chain: 'tron', address: '{}' }}) RETURN p LIMIT {}",
            source, FLOW_RELATIONSHIP_TYPES, safe_depth, target, safe_limit
        ),
        PathSearchDirection::Outgoing => format!(
            "MATCH p = (source:Wallet {{ chain: 'tron', address: '{}' }})-[:{}*1..{}]->(target:Wallet {{ chain: 'tron', address: '{}' }}) RETURN p LIMIT {}",
            source, FLOW_RELATIONSHIP_TYPES, safe_depth, target, safe_limit
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_direct_exchange_interactions() {
        let edge = FlowEdge {
            id: "edge".to_string(),
            from: "wallet".to_string(),
            to: "exchange_wallet".to_string(),
            tx_hash: "tx".to_string(),
            token_address: "TRX".to_string(),
            amount: "100".to_string(),
            block_number: 10,
            timestamp: 1,
            transfer_type: "native_transfer".to_string(),
            operation_type: "native_transfer".to_string(),
            relationship_type: "NATIVE_TRANSFER".to_string(),
            protocol: "".to_string(),
            initiator_address: "wallet".to_string(),
            target_address: "exchange_wallet".to_string(),
            contract_address: String::new(),
            contract_type: "TransferContract".to_string(),
            transaction_fee: "0".to_string(),
            energy_usage_total: 0,
            net_usage: 0,
            execution_status: 1,
            transaction_type: "transfer".to_string(),
            transaction_subtype: "native_transfer".to_string(),
            classification_confidence: 1.0,
            classification_source: "native_contract".to_string(),
            method_id: String::new(),
            is_contract_call: false,
            exchange_flow_type: None,
            exchange_name: None,
            exchange_confidence: None,
        };

        let metadata = HashMap::from([(
            "exchange_wallet".to_string(),
            ExchangeMetadata {
                exchange_name: "Binance".to_string(),
                exchange_role: "HOT".to_string(),
                confidence: 1.0,
            },
        )]);

        let summaries = exchange_summaries("wallet", &[edge], &metadata);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].direction, "outgoing");
        assert_eq!(summaries[0].exchange_name, "Binance");
        assert_eq!(summaries[0].operation_type, "native_transfer");
    }

    #[test]
    fn builds_browser_cypher_for_root_wallet() {
        let cypher = neo4j_browser_cypher("TAddress", 3, 500);

        assert!(cypher.contains("address: 'TAddress'"));
        assert!(cypher.contains("NATIVE_TRANSFER|TRC10_TRANSFER|TRC20_TRANSFER"));
        assert!(cypher.contains("SWAP|BRIDGE|LIQUIDITY_ADD|LIQUIDITY_REMOVE"));
        assert!(cypher.contains("*1..3"));
        assert!(cypher.ends_with("RETURN p LIMIT 500"));
    }
}
