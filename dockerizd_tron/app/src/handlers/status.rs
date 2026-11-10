use crate::{
    config::AppConfig,
    handlers::tron_common::{TronApiError, clickhouse_client},
};
use aml_chain_api::{ApiError, ApiPath, ApiQuery, EmptyQuery, Network, bridge};
use axum::Json;
use serde_json::json;
pub async fn kyt_transaction(
    aml_chain_api::ApiPath(hash): aml_chain_api::ApiPath<String>,
) -> Result<Json<aml_chain_api::kyt::TransactionObservation>, aml_chain_api::ApiError> {
    let client = clickhouse_client(&AppConfig::from_env());
    aml_chain_api::kyt::transaction(
        &*client,
        client.database(),
        aml_chain_api::Network::Tron,
        &hash,
    )
    .await
    .map(Json)
}
pub async fn status() -> Result<Json<serde_json::Value>, TronApiError> {
    let client = clickhouse_client(&AppConfig::from_env());
    let snapshot = aml_warehouse::visibility::capture(
        &*client,
        client.database(),
        aml_warehouse::Network::Tron,
    )
    .await
    .map_err(TronApiError::internal)?;
    let operational = client
        .operational_version()
        .await
        .map_err(TronApiError::internal)?;
    aml_warehouse::visibility::unchanged(&*client, client.database(), &snapshot)
        .await
        .map_err(TronApiError::internal)?;
    if client
        .operational_version()
        .await
        .map_err(TronApiError::internal)?
        != operational
    {
        return Err(TronApiError::internal(anyhow::anyhow!(
            "operational source changed; retry status"
        )));
    }
    Ok(Json(
        json!({"service":"tron-aml","network_id":"tron:mainnet","source_snapshot":snapshot.value(),"operational_snapshot":crate::db::records::operational_snapshot(&operational)}),
    ))
}

pub async fn bridge_transaction(
    ApiPath(hash): ApiPath<String>,
    ApiQuery(anchor): ApiQuery<bridge::Anchor>,
) -> Result<Json<bridge::Observation>, ApiError> {
    let client = clickhouse_client(&AppConfig::from_env());
    bridge::lookup(
        &*client,
        client.database(),
        Network::Tron,
        &hash,
        bridge::Lookup::Transaction(anchor),
    )
    .await
    .map(Json)
}

pub async fn allbridge_operation(
    ApiPath(hash): ApiPath<String>,
    ApiQuery(window): ApiQuery<bridge::Window>,
) -> Result<Json<bridge::Observation>, ApiError> {
    let client = clickhouse_client(&AppConfig::from_env());
    bridge::lookup(
        &*client,
        client.database(),
        Network::Tron,
        &hash,
        bridge::Lookup::AllbridgeOperation(window),
    )
    .await
    .map(Json)
}

pub async fn bridge_snapshot(
    ApiQuery(_): ApiQuery<EmptyQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = clickhouse_client(&AppConfig::from_env());
    bridge::snapshot(&*client, client.database(), Network::Tron)
        .await
        .map(Json)
}
