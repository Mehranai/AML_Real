use aml_chain_api::{ApiPath as Path, ApiQuery as Query, bounded};
use axum::Json;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::handlers::tron_common::{
    TronApiError, clickhouse_client, neo4j_client, normalize_wallet_address,
};
use crate::services::tron::neo4j::{flow_graph::build_wallet_path_graph, types::WalletPathGraph};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalletPathQuery {
    #[serde(rename = "max_hops", alias = "max_depth")]
    pub max_depth: Option<u8>,
    pub limit: Option<usize>,
    pub per_address_limit: Option<u64>,
    pub direction: Option<String>,
}

impl WalletPathQuery {
    fn validate(&self) -> Result<Option<&'static str>, TronApiError> {
        let validate = || -> Result<(), aml_chain_api::ApiError> {
            bounded("max_hops", self.max_depth, 1, 10)?;
            bounded("limit", self.limit, 1, 50)?;
            bounded("per_address_limit", self.per_address_limit, 1, 2_000)
        };
        validate().map_err(|e| TronApiError::bad_request(e.message))?;
        match self
            .direction
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            None => Ok(None),
            Some("outgoing" | "outbound" | "forward") => Ok(Some("outgoing")),
            Some("incoming" | "inbound" | "reverse") => Ok(Some("incoming")),
            Some("both" | "any" | "undirected") => Ok(Some("both")),
            _ => Err(TronApiError::bad_request(
                "direction must be outgoing, incoming or both".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_and_legacy_path_options_keep_requested_values() {
        for (query, depth, direction) in [
            (
                "max_hops=4&limit=50&per_address_limit=2000&direction=incoming",
                4,
                "incoming",
            ),
            (
                "max_depth=5&limit=50&per_address_limit=2000&direction=inbound",
                5,
                "incoming",
            ),
            (
                "max_hops=1&limit=50&per_address_limit=2000&direction=outbound",
                1,
                "outgoing",
            ),
            (
                "max_hops=10&limit=50&per_address_limit=2000&direction=both",
                10,
                "both",
            ),
        ] {
            let uri = format!("/paths?{query}").parse().unwrap();
            let axum::extract::Query(value) =
                axum::extract::Query::<WalletPathQuery>::try_from_uri(&uri).unwrap();
            assert_eq!(value.max_depth, Some(depth));
            assert_eq!(value.limit, Some(50));
            assert_eq!(value.per_address_limit, Some(2000));
            assert_eq!(value.validate().unwrap(), Some(direction));
        }
    }
}

pub async fn tron_wallet_paths(
    Path((source, target)): Path<(String, String)>,
    Query(params): Query<WalletPathQuery>,
) -> Result<Json<serde_json::Value>, TronApiError> {
    let direction = params.validate()?;
    let config = AppConfig::from_env();
    let source = normalize_wallet_address(&source)?;
    let target = normalize_wallet_address(&target)?;
    if source == target {
        return Err(TronApiError::bad_request(
            "distinct addresses required".into(),
        ));
    }
    let clickhouse = clickhouse_client(&config);

    let graph = build_wallet_path_graph(
        clickhouse,
        None,
        &source,
        &target,
        params.max_depth,
        params.limit,
        params.per_address_limit,
        direction,
    )
    .await
    .map_err(TronApiError::internal)?;

    let value = serde_json::to_value(graph).map_err(|e| TronApiError::internal(e.into()))?;
    Ok(Json(
        aml_chain_api::response::normalize(
            aml_warehouse::Network::Tron,
            aml_chain_api::response::Mode::Paths,
            value,
        )
        .map_err(TronApiError::internal)?,
    ))
}

pub async fn tron_wallet_paths_import(
    Path((source, target)): Path<(String, String)>,
    Query(params): Query<WalletPathQuery>,
) -> Result<Json<WalletPathGraph>, TronApiError> {
    let direction = params.validate()?;
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
        direction,
    )
    .await
    .map_err(TronApiError::internal)?;

    Ok(Json(graph))
}
