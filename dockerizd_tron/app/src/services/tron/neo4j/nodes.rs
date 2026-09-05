use super::client::Neo4jClient;
use neo4rs::query;

#[allow(clippy::too_many_arguments)]
pub async fn upsert_wallet_with_metadata(
    neo4j: &Neo4jClient,
    address: &str,
    label: &str,
    node_type: &str,
    entity_name: Option<&str>,
    entity_type: Option<&str>,
    exchange_name: Option<&str>,
    exchange_role: Option<&str>,
    cluster_id: Option<&str>,
    cluster_role: Option<&str>,
    confidence: Option<f32>,
) -> anyhow::Result<()> {
    let q = query(
        "
        MERGE (w:Wallet { chain: 'tron', address: $address })
        SET w:TronAddress,
            w.label = $label,
            w.node_type = $node_type,
            w.entity_name = $entity_name,
            w.entity_type = $entity_type,
            w.exchange_name = $exchange_name,
            w.exchange_role = $exchange_role,
            w.cluster_id = $cluster_id,
            w.cluster_role = $cluster_role,
            w.exchange_confidence_bps = $confidence_bps
        ",
    )
    .param("address", address)
    .param("label", label)
    .param("node_type", node_type)
    .param("entity_name", entity_name.unwrap_or(""))
    .param("entity_type", entity_type.unwrap_or(""))
    .param("exchange_name", exchange_name.unwrap_or(""))
    .param("exchange_role", exchange_role.unwrap_or(""))
    .param("cluster_id", cluster_id.unwrap_or(""))
    .param("cluster_role", cluster_role.unwrap_or(""))
    .param(
        "confidence_bps",
        (confidence.unwrap_or(0.0) * 10_000.0) as i64,
    );

    neo4j
        .graph
        .run(q)
        .await
        .map_err(|err| anyhow::anyhow!("{:?}", err))?;

    apply_node_type_label(neo4j, address, node_type).await?;

    if let Some(exchange) = exchange_name
        && !exchange.is_empty()
    {
        upsert_exchange(
            neo4j,
            address,
            exchange,
            exchange_role.unwrap_or(""),
            confidence,
        )
        .await?;
    }

    if let Some(cluster_id) = cluster_id
        && !cluster_id.is_empty()
    {
        upsert_cluster_membership(
            neo4j,
            address,
            cluster_id,
            cluster_role.unwrap_or("UNKNOWN"),
            confidence,
        )
        .await?;
    }

    Ok(())
}

async fn upsert_cluster_membership(
    neo4j: &Neo4jClient,
    address: &str,
    cluster_id: &str,
    role: &str,
    confidence: Option<f32>,
) -> anyhow::Result<()> {
    let q = query(
        "
        MERGE (c:AddressCluster { chain: 'tron', cluster_id: $cluster_id })
        MERGE (w:Wallet { chain: 'tron', address: $address })
        MERGE (w)-[r:MEMBER_OF]->(c)
        SET r.role = $role,
            r.confidence_bps = $confidence_bps,
            r.chain = 'tron'
        ",
    )
    .param("cluster_id", cluster_id)
    .param("address", address)
    .param("role", role)
    .param(
        "confidence_bps",
        (confidence.unwrap_or(0.0) * 10_000.0) as i64,
    );
    neo4j
        .graph
        .run(q)
        .await
        .map_err(|err| anyhow::anyhow!("failed to upsert Neo4j cluster membership: {:?}", err))?;
    Ok(())
}

async fn apply_node_type_label(
    neo4j: &Neo4jClient,
    address: &str,
    node_type: &str,
) -> anyhow::Result<()> {
    let label_query = match node_type {
        "exchange_wallet" => {
            "MATCH (w:Wallet { chain: 'tron', address: $address }) REMOVE w:ExternalWallet:Bridge:Protocol:TokenLifecycle:Mint:Burn SET w:ExchangeWallet"
        }
        "bridge" => {
            "MATCH (w:Wallet { chain: 'tron', address: $address }) REMOVE w:ExternalWallet:ExchangeWallet:TokenLifecycle:Mint:Burn SET w:Bridge:Protocol"
        }
        "mint" => {
            "MATCH (w:Wallet { chain: 'tron', address: $address }) REMOVE w:ExternalWallet:ExchangeWallet:Bridge:Protocol:Burn SET w:TokenLifecycle:Mint"
        }
        "burn" => {
            "MATCH (w:Wallet { chain: 'tron', address: $address }) REMOVE w:ExternalWallet:ExchangeWallet:Bridge:Protocol:Mint SET w:TokenLifecycle:Burn"
        }
        "protocol" => {
            "MATCH (w:Wallet { chain: 'tron', address: $address }) REMOVE w:ExternalWallet:ExchangeWallet:Bridge:TokenLifecycle:Mint:Burn SET w:Protocol"
        }
        entity if entity.starts_with("exchange_") => {
            "MATCH (w:Wallet { chain: 'tron', address: $address }) REMOVE w:ExternalWallet:Bridge:Protocol:TokenLifecycle:Mint:Burn SET w:ExchangeWallet"
        }
        _ => {
            "MATCH (w:Wallet { chain: 'tron', address: $address }) REMOVE w:ExchangeWallet:Bridge:Protocol:TokenLifecycle:Mint:Burn SET w:ExternalWallet"
        }
    };

    neo4j
        .graph
        .run(query(label_query).param("address", address))
        .await
        .map_err(|err| anyhow::anyhow!("failed to label Neo4j node type: {:?}", err))?;

    Ok(())
}

pub async fn upsert_exchange(
    neo4j: &Neo4jClient,
    address: &str,
    exchange: &str,
    role: &str,
    confidence: Option<f32>,
) -> anyhow::Result<()> {
    let q = query(
        "
        MERGE (e:Exchange { name: $exchange })
        SET e.entity_type = 'exchange',
            e.chain = 'tron'

        MERGE (w:Wallet { chain: 'tron', address: $address })
        SET w:TronAddress:ExchangeWallet

        MERGE (w)-[r:BELONGS_TO]->(e)
        SET r.role = $role,
            r.confidence_bps = $confidence_bps,
            r.chain = 'tron'
        ",
    )
    .param("address", address)
    .param("exchange", exchange)
    .param("role", role)
    .param(
        "confidence_bps",
        (confidence.unwrap_or(0.0) * 10_000.0) as i64,
    );

    neo4j
        .graph
        .run(q)
        .await
        .map_err(|err| anyhow::anyhow!("failed to upsert Neo4j exchange attribution: {:?}", err))?;

    Ok(())
}
