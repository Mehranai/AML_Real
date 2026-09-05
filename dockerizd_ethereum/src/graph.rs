use std::{collections::HashSet, sync::Arc};

use anyhow::Context;
use neo4rs::{Graph, query};
use serde::Serialize;
use tokio::sync::OnceCell;

use crate::investigation::FlowEdge;

static NEO4J_SCHEMA_READY: OnceCell<()> = OnceCell::const_new();

#[derive(Clone)]
pub struct EthereumGraph {
    graph: Arc<Graph>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectionSummary {
    pub projected: bool,
    pub node_count: usize,
    pub edge_count: usize,
}

impl EthereumGraph {
    pub async fn connect(uri: &str, username: &str, password: &str) -> anyhow::Result<Self> {
        let endpoint = normalize_bolt_endpoint(uri);
        let graph = Graph::new(&endpoint, username, password)
            .await
            .with_context(|| format!("failed to connect to Neo4j at {endpoint}"))?;

        let client = Self {
            graph: Arc::new(graph),
        };
        client.ensure_schema().await?;
        Ok(client)
    }

    pub async fn probe(&self) -> anyhow::Result<()> {
        self.graph
            .run(query("RETURN 1 AS ready"))
            .await
            .context("Neo4j readiness probe failed")
    }

    pub async fn project_wallet(&self, network_id: &str, address: &str) -> anyhow::Result<()> {
        self.graph
            .run(
                query(
                    r#"
                    MERGE (wallet:Wallet {network_id: $network_id, address: $address})
                    ON CREATE SET wallet.created_at_unix_ms = timestamp()
                    SET wallet.chain = 'ethereum', wallet.updated_at_unix_ms = timestamp()
                    "#,
                )
                .param("network_id", network_id.to_string())
                .param("address", address.to_string()),
            )
            .await
            .with_context(|| format!("failed to project Ethereum wallet {address}"))
    }

    pub async fn project_edges(&self, edges: &[FlowEdge]) -> anyhow::Result<ProjectionSummary> {
        let mut nodes = HashSet::new();

        for edge in edges {
            nodes.insert(edge.from_address.clone());
            nodes.insert(edge.to_address.clone());
            let block_number = i64::try_from(edge.block_number)
                .context("Ethereum block number does not fit Neo4j INTEGER")?;
            let timestamp = i64::try_from(edge.block_timestamp_unix_ms)
                .context("Ethereum timestamp does not fit Neo4j INTEGER")?;

            self.graph
                .run(
                    query(
                        r#"
                        MERGE (source:Wallet {
                            network_id: $network_id,
                            address: $from_address
                        })
                        ON CREATE SET source.created_at_unix_ms = timestamp()
                        SET source.chain = 'ethereum', source.updated_at_unix_ms = timestamp()
                        MERGE (target:Wallet {
                            network_id: $network_id,
                            address: $to_address
                        })
                        ON CREATE SET target.created_at_unix_ms = timestamp()
                        SET target.chain = 'ethereum', target.updated_at_unix_ms = timestamp()
                        MERGE (source)-[flow:TRANSFER {id: $relationship_id}]->(target)
                        SET flow.tx_hash = $tx_hash,
                            flow.asset_id = $asset_id,
                            flow.token_id = $token_id,
                            flow.amount = $amount,
                            flow.transfer_type = $transfer_type,
                            flow.block_number = $block_number,
                            flow.block_timestamp_unix_ms = $block_timestamp_unix_ms,
                            flow.updated_at_unix_ms = timestamp()
                        "#,
                    )
                    .param("network_id", edge.network_id.clone())
                    .param("from_address", edge.from_address.clone())
                    .param("to_address", edge.to_address.clone())
                    .param("relationship_id", edge.id.clone())
                    .param("tx_hash", edge.tx_hash.clone())
                    .param("asset_id", edge.asset_id.clone())
                    .param("token_id", edge.token_id.clone())
                    .param("amount", edge.amount.clone())
                    .param("transfer_type", edge.transfer_type.clone())
                    .param("block_number", block_number)
                    .param("block_timestamp_unix_ms", timestamp),
                )
                .await
                .with_context(|| format!("failed to project Ethereum relationship {}", edge.id))?;
        }

        Ok(ProjectionSummary {
            projected: true,
            node_count: nodes.len(),
            edge_count: edges.len(),
        })
    }

    async fn ensure_schema(&self) -> anyhow::Result<()> {
        NEO4J_SCHEMA_READY
            .get_or_try_init(|| async {
                for statement in [
                    "CREATE CONSTRAINT wallet_network_address IF NOT EXISTS FOR (wallet:Wallet) REQUIRE (wallet.network_id, wallet.address) IS UNIQUE",
                    "CREATE INDEX wallet_chain IF NOT EXISTS FOR (wallet:Wallet) ON (wallet.chain)",
                    "CREATE INDEX ethereum_transfer_id IF NOT EXISTS FOR ()-[flow:TRANSFER]-() ON (flow.id)",
                    "CREATE INDEX ethereum_transfer_tx_hash IF NOT EXISTS FOR ()-[flow:TRANSFER]-() ON (flow.tx_hash)",
                    "CREATE INDEX ethereum_transfer_block IF NOT EXISTS FOR ()-[flow:TRANSFER]-() ON (flow.block_number)",
                ] {
                    self.graph
                        .run(query(statement))
                        .await
                        .with_context(|| format!("failed Neo4j schema statement: {statement}"))?;
                }
                Ok::<(), anyhow::Error>(())
            })
            .await
            .map(|_| ())
    }
}

fn normalize_bolt_endpoint(uri: &str) -> String {
    let endpoint = uri.trim();
    if endpoint.starts_with("bolt://")
        || endpoint.starts_with("bolt+s://")
        || endpoint.starts_with("neo4j://")
        || endpoint.starts_with("neo4j+s://")
    {
        endpoint.to_string()
    } else if endpoint.contains(':') {
        format!("bolt://{endpoint}")
    } else {
        format!("bolt://{endpoint}:7687")
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_bolt_endpoint;

    #[test]
    fn normalizes_neo4j_endpoints() {
        assert_eq!(
            normalize_bolt_endpoint("neo4j://localhost:7687"),
            "neo4j://localhost:7687"
        );
        assert_eq!(normalize_bolt_endpoint("neo4j"), "bolt://neo4j:7687");
        assert_eq!(
            normalize_bolt_endpoint("127.0.0.1:27687"),
            "bolt://127.0.0.1:27687"
        );
    }
}
