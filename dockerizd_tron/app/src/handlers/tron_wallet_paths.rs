use axum::{
    Json,
    extract::{Path, Query},
};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{
    TronApiError, clickhouse_client, neo4j_client, normalize_wallet_address,
};
use crate::services::tron::neo4j::{flow_graph::build_wallet_path_graph, types::WalletPathGraph};

#[derive(Debug, Deserialize)]
pub struct WalletPathQuery {
    pub max_depth: Option<u8>,
    pub limit: Option<usize>,
    pub per_address_limit: Option<u64>,
    pub direction: Option<String>,
}

pub async fn tron_wallet_paths(
    Path((source, target)): Path<(String, String)>,
    Query(params): Query<WalletPathQuery>,
) -> Result<Json<WalletPathGraph>, TronApiError> {
    let config = AppConfig::from_env();
    let source = normalize_wallet_address(&source)?;
    let target = normalize_wallet_address(&target)?;
    let clickhouse = clickhouse_client(&config);

    let graph = build_wallet_path_graph(
        clickhouse,
        None,
        &source,
        &target,
        params.max_depth,
        params.limit,
        params.per_address_limit,
        params.direction.as_deref(),
    )
    .await
    .map_err(TronApiError::internal)?;

    Ok(Json(graph))
}

pub async fn tron_wallet_paths_import(
    Path((source, target)): Path<(String, String)>,
    Query(params): Query<WalletPathQuery>,
) -> Result<Json<WalletPathGraph>, TronApiError> {
    let config = AppConfig::from_env();
    let source = normalize_wallet_address(&source)?;
    let target = normalize_wallet_address(&target)?;
    let clickhouse = clickhouse_client(&config);
    let neo4j = neo4j_client(&config).await?;

    let graph = build_wallet_path_graph(
        clickhouse,
        Some(&neo4j),
        &source,
        &target,
        params.max_depth,
        params.limit,
        params.per_address_limit,
        params.direction.as_deref(),
    )
    .await
    .map_err(TronApiError::internal)?;

    Ok(Json(graph))
}
