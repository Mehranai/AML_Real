use axum::{
    extract::{Path, Query},
    response::Json,
};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{
    TronApiError, clickhouse_client, neo4j_client, normalize_wallet_address,
};
use crate::services::tron::neo4j::{flow_graph::build_wallet_flow_graph, types::WalletFlowGraph};

#[derive(Debug, Deserialize)]
pub struct WalletGraphQuery {
    pub depth: Option<u8>,
    pub limit: Option<u64>,
}

pub async fn tron_wallet_graph(
    Path(address): Path<String>,
    Query(params): Query<WalletGraphQuery>,
) -> Result<Json<WalletFlowGraph>, TronApiError> {
    let config = AppConfig::from_env();
    let address = normalize_wallet_address(&address)?;
    let clickhouse = clickhouse_client(&config);

    let graph = build_wallet_flow_graph(
        clickhouse,
        None,
        &address,
        params.depth.unwrap_or(3),
        params.limit.unwrap_or(500),
    )
    .await
    .map_err(TronApiError::internal)?;

    Ok(Json(graph))
}

pub async fn tron_wallet_graph_import(
    Path(address): Path<String>,
    Query(params): Query<WalletGraphQuery>,
) -> Result<Json<WalletFlowGraph>, TronApiError> {
    let config = AppConfig::from_env();
    let address = normalize_wallet_address(&address)?;
    let clickhouse = clickhouse_client(&config);
    let neo4j = neo4j_client(&config).await?;

    let graph = build_wallet_flow_graph(
        clickhouse,
        Some(&neo4j),
        &address,
        params.depth.unwrap_or(3),
        params.limit.unwrap_or(500),
    )
    .await
    .map_err(TronApiError::internal)?;

    Ok(Json(graph))
}
