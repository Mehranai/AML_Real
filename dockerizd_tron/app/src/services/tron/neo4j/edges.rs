use super::client::Neo4jClient;
use super::types::FlowEdge;
use neo4rs::query;

pub async fn merge_transfer_edge(neo4j: &Neo4jClient, edge: &FlowEdge) -> anyhow::Result<()> {
    let relationship_type = safe_relationship_type(&edge.relationship_type);

    let delete_previous_edge = query(
        "
        MATCH (a:Wallet { chain: 'tron', address: $from })-[old { id: $edge_id }]->(b:Wallet { chain: 'tron', address: $to })
        DELETE old
        ",
    )
    .param("from", edge.from.as_str())
    .param("to", edge.to.as_str())
    .param("edge_id", edge.id.as_str());

    neo4j
        .graph
        .run(delete_previous_edge)
        .await
        .map_err(|err| anyhow::anyhow!("failed to remove stale Neo4j flow edge: {:?}", err))?;

    let q = query(&format!(
        "
        MERGE (a:Wallet {{ chain: 'tron', address: $from }})
        SET a:TronAddress
        MERGE (b:Wallet {{ chain: 'tron', address: $to }})
        SET b:TronAddress
        MERGE (a)-[t:{relationship_type} {{ id: $edge_id }}]->(b)
        SET t.tx_hash = $tx_hash,
            t.token = $token,
            t.amount = $amount,
            t.block_number = $block_number,
            t.timestamp = $timestamp,
            t.transfer_type = $transfer_type,
            t.operation_type = $operation_type,
            t.relationship_type = $relationship_type,
            t.protocol = $protocol,
            t.initiator_address = $initiator_address,
            t.target_address = $target_address,
            t.contract_address = $contract_address,
            t.contract_type = $contract_type,
            t.transaction_fee = $transaction_fee,
            t.energy_usage_total = $energy_usage_total,
            t.net_usage = $net_usage,
            t.execution_status = $execution_status,
            t.transaction_type = $transaction_type,
            t.transaction_subtype = $transaction_subtype,
            t.classification_confidence_bps = $classification_confidence_bps,
            t.classification_source = $classification_source,
            t.method_id = $method_id,
            t.is_contract_call = $is_contract_call,
            t.exchange_flow_type = $exchange_flow_type,
            t.exchange_name = $exchange_name,
            t.exchange_confidence_bps = $exchange_confidence_bps,
            t.chain = 'tron'
        ",
    ))
    .param("from", edge.from.as_str())
    .param("to", edge.to.as_str())
    .param("edge_id", edge.id.as_str())
    .param("tx_hash", edge.tx_hash.as_str())
    .param("token", edge.token_address.as_str())
    .param("amount", edge.amount.as_str())
    .param("block_number", edge.block_number as i64)
    .param("timestamp", edge.timestamp as i64)
    .param("transfer_type", edge.transfer_type.as_str())
    .param("operation_type", edge.operation_type.as_str())
    .param("relationship_type", edge.relationship_type.as_str())
    .param("protocol", edge.protocol.as_str())
    .param("initiator_address", edge.initiator_address.as_str())
    .param("target_address", edge.target_address.as_str())
    .param("contract_address", edge.contract_address.as_str())
    .param("contract_type", edge.contract_type.as_str())
    .param("transaction_fee", edge.transaction_fee.as_str())
    .param("energy_usage_total", edge.energy_usage_total as i64)
    .param("net_usage", edge.net_usage as i64)
    .param("execution_status", i64::from(edge.execution_status))
    .param("transaction_type", edge.transaction_type.as_str())
    .param("transaction_subtype", edge.transaction_subtype.as_str())
    .param(
        "classification_confidence_bps",
        (edge.classification_confidence * 10_000.0) as i64,
    )
    .param("classification_source", edge.classification_source.as_str())
    .param("method_id", edge.method_id.as_str())
    .param(
        "is_contract_call",
        if edge.is_contract_call { 1_i64 } else { 0_i64 },
    )
    .param(
        "exchange_flow_type",
        edge.exchange_flow_type.as_deref().unwrap_or(""),
    )
    .param("exchange_name", edge.exchange_name.as_deref().unwrap_or(""))
    .param(
        "exchange_confidence_bps",
        (edge.exchange_confidence.unwrap_or(0.0) * 10_000.0) as i64,
    );

    neo4j
        .graph
        .run(q)
        .await
        .map_err(|err| anyhow::anyhow!("{:?}", err))?;

    Ok(())
}

fn safe_relationship_type(relationship_type: &str) -> &'static str {
    match relationship_type {
        "SWAP" => "SWAP",
        "BRIDGE" => "BRIDGE",
        "EXCHANGE_DEPOSIT" => "EXCHANGE_DEPOSIT",
        "EXCHANGE_WITHDRAWAL" => "EXCHANGE_WITHDRAWAL",
        "EXCHANGE_SWEEP" => "EXCHANGE_SWEEP",
        "EXCHANGE_TRANSFER" => "EXCHANGE_TRANSFER",
        "INTERNAL_TRANSFER" => "INTERNAL_TRANSFER",
        "LIQUIDITY_ADD" => "LIQUIDITY_ADD",
        "LIQUIDITY_REMOVE" => "LIQUIDITY_REMOVE",
        "MINT" => "MINT",
        "BURN" => "BURN",
        "NATIVE_TRANSFER" => "NATIVE_TRANSFER",
        "TRC20_TRANSFER" => "TRC20_TRANSFER",
        _ => "MONEY_FLOW",
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn merge_exchange_interaction(
    neo4j: &Neo4jClient,
    wallet_address: &str,
    exchange_name: &str,
    exchange_address: &str,
    exchange_role: &str,
    direction: &str,
    tx_hash: &str,
    token: &str,
    amount: &str,
    block_number: u64,
    confidence: f32,
) -> anyhow::Result<()> {
    let interaction_id = format!(
        "{}:{}:{}:{}:{}",
        wallet_address, exchange_name, exchange_address, tx_hash, direction
    );

    let q = query(
        "
        MERGE (w:Wallet { chain: 'tron', address: $wallet_address })
        SET w:TronAddress

        MERGE (exchange_wallet:Wallet { chain: 'tron', address: $exchange_address })
        SET exchange_wallet:TronAddress,
            exchange_wallet.node_type = 'exchange_wallet',
            exchange_wallet.exchange_name = $exchange_name,
            exchange_wallet.exchange_role = $exchange_role,
            exchange_wallet.exchange_confidence_bps = $confidence_bps

        MERGE (e:Exchange { name: $exchange_name })
        SET e.entity_type = 'exchange',
            e.chain = 'tron'

        MERGE (exchange_wallet)-[belongs:BELONGS_TO]->(e)
        SET belongs.role = $exchange_role,
            belongs.confidence_bps = $confidence_bps,
            belongs.chain = 'tron'

        MERGE (w)-[i:INTERACTED_WITH { id: $interaction_id }]->(e)
        SET i.direction = $direction,
            i.exchange_address = $exchange_address,
            i.exchange_role = $exchange_role,
            i.tx_hash = $tx_hash,
            i.token = $token,
            i.amount = $amount,
            i.block_number = $block_number,
            i.confidence_bps = $confidence_bps,
            i.chain = 'tron'
        ",
    )
    .param("wallet_address", wallet_address)
    .param("exchange_name", exchange_name)
    .param("exchange_address", exchange_address)
    .param("exchange_role", exchange_role)
    .param("direction", direction)
    .param("tx_hash", tx_hash)
    .param("token", token)
    .param("amount", amount)
    .param("block_number", block_number as i64)
    .param("confidence_bps", (confidence * 10_000.0) as i64)
    .param("interaction_id", interaction_id);

    neo4j
        .graph
        .run(q)
        .await
        .map_err(|err| anyhow::anyhow!("failed to merge Neo4j exchange interaction: {:?}", err))?;

    Ok(())
}
