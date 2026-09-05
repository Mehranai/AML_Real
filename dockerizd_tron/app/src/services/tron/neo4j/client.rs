use neo4rs::{Graph, query};
use tokio::sync::OnceCell;

static NEO4J_SCHEMA_READY: OnceCell<()> = OnceCell::const_new();
use std::sync::Arc;

#[derive(Clone)]
pub struct Neo4jClient {
    pub graph: Arc<Graph>,
}

impl Neo4jClient {
    pub async fn new(uri: &str, username: &str, password: &str) -> anyhow::Result<Self> {
        let bolt_endpoint = normalize_bolt_endpoint(uri);

        let graph = Graph::new(&bolt_endpoint, username, password)
            .await
            .map_err(|err| {
                anyhow::anyhow!(
                    "failed to connect to Neo4j at {} (configured as {}): {:?}",
                    bolt_endpoint,
                    uri,
                    err
                )
            })?;

        Ok(Self {
            graph: Arc::new(graph),
        })
    }

    pub async fn ensure_schema(&self) -> anyhow::Result<()> {
        NEO4J_SCHEMA_READY
            .get_or_try_init(|| self.create_schema())
            .await
            .map(|_| ())
    }

    async fn create_schema(&self) -> anyhow::Result<()> {
        let statements = [
            "CREATE CONSTRAINT wallet_chain_address IF NOT EXISTS FOR (w:Wallet) REQUIRE (w.chain, w.address) IS UNIQUE",
            "CREATE CONSTRAINT exchange_name IF NOT EXISTS FOR (e:Exchange) REQUIRE e.name IS UNIQUE",
            "CREATE CONSTRAINT address_cluster_chain_id IF NOT EXISTS FOR (c:AddressCluster) REQUIRE (c.chain, c.cluster_id) IS UNIQUE",
            "CREATE INDEX wallet_exchange_role IF NOT EXISTS FOR (w:Wallet) ON (w.exchange_role)",
            "CREATE INDEX wallet_node_type IF NOT EXISTS FOR (w:Wallet) ON (w.node_type)",
            "CREATE INDEX wallet_entity_type IF NOT EXISTS FOR (w:Wallet) ON (w.entity_type)",
            "CREATE INDEX wallet_cluster_id IF NOT EXISTS FOR (w:Wallet) ON (w.cluster_id)",
            "CREATE INDEX native_transfer_id IF NOT EXISTS FOR ()-[r:NATIVE_TRANSFER]-() ON (r.id)",
            "CREATE INDEX trc10_transfer_id IF NOT EXISTS FOR ()-[r:TRC10_TRANSFER]-() ON (r.id)",
            "CREATE INDEX trc20_transfer_id IF NOT EXISTS FOR ()-[r:TRC20_TRANSFER]-() ON (r.id)",
            "CREATE INDEX swap_id IF NOT EXISTS FOR ()-[r:SWAP]-() ON (r.id)",
            "CREATE INDEX bridge_id IF NOT EXISTS FOR ()-[r:BRIDGE]-() ON (r.id)",
            "CREATE INDEX liquidity_add_id IF NOT EXISTS FOR ()-[r:LIQUIDITY_ADD]-() ON (r.id)",
            "CREATE INDEX liquidity_remove_id IF NOT EXISTS FOR ()-[r:LIQUIDITY_REMOVE]-() ON (r.id)",
            "CREATE INDEX mint_id IF NOT EXISTS FOR ()-[r:MINT]-() ON (r.id)",
            "CREATE INDEX burn_id IF NOT EXISTS FOR ()-[r:BURN]-() ON (r.id)",
            "CREATE INDEX exchange_deposit_id IF NOT EXISTS FOR ()-[r:EXCHANGE_DEPOSIT]-() ON (r.id)",
            "CREATE INDEX exchange_withdrawal_id IF NOT EXISTS FOR ()-[r:EXCHANGE_WITHDRAWAL]-() ON (r.id)",
            "CREATE INDEX exchange_sweep_id IF NOT EXISTS FOR ()-[r:EXCHANGE_SWEEP]-() ON (r.id)",
            "CREATE INDEX exchange_transfer_id IF NOT EXISTS FOR ()-[r:EXCHANGE_TRANSFER]-() ON (r.id)",
            "CREATE INDEX internal_transfer_id IF NOT EXISTS FOR ()-[r:INTERNAL_TRANSFER]-() ON (r.id)",
            "CREATE INDEX money_flow_id IF NOT EXISTS FOR ()-[r:MONEY_FLOW]-() ON (r.id)",
        ];

        for statement in statements {
            self.graph.run(query(statement)).await.map_err(|err| {
                anyhow::anyhow!(
                    "failed to apply Neo4j schema statement `{}`: {:?}",
                    statement,
                    err
                )
            })?;
        }

        Ok(())
    }
}

fn normalize_bolt_endpoint(uri: &str) -> String {
    let endpoint = uri
        .trim()
        .strip_prefix("bolt://")
        .or_else(|| uri.trim().strip_prefix("neo4j://"))
        .unwrap_or_else(|| uri.trim());

    if endpoint.contains(':') {
        endpoint.to_string()
    } else {
        format!("{}:7687", endpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_bolt_endpoint;

    #[test]
    fn strips_bolt_scheme_for_neo4rs() {
        assert_eq!(
            normalize_bolt_endpoint("bolt://localhost:7687"),
            "localhost:7687"
        );
    }

    #[test]
    fn adds_default_bolt_port() {
        assert_eq!(normalize_bolt_endpoint("localhost"), "localhost:7687");
    }
}
