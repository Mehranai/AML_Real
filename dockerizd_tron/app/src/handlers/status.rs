use crate::{
    config::AppConfig,
    handlers::tron_common::{TronApiError, clickhouse_client},
};
use axum::Json;
use serde_json::json;
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
