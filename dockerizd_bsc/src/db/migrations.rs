//! BSC fresh deployments use only the common schema and immutable migration checks.
use super::{Client, database_client};
use crate::config::ClickHouseConfig;
pub type SchemaSummary = aml_warehouse::SchemaSummary;
pub type SchemaError = anyhow::Error;
pub async fn initialize_bsc_schema(
    config: &ClickHouseConfig,
) -> Result<SchemaSummary, SchemaError> {
    let admin = Client::default()
        .with_url(config.endpoint())
        .with_user(config.user())
        .with_password(config.password());
    aml_warehouse::initialize(&admin, config.database(), aml_warehouse::Network::Bsc).await
}
pub async fn validate_bsc_schema(config: &ClickHouseConfig) -> Result<SchemaSummary, SchemaError> {
    aml_warehouse::validate(
        &database_client(config),
        config.database(),
        aml_warehouse::Network::Bsc,
    )
    .await
}
#[cfg(test)]
pub(crate) async fn initialize_test_database(
    client: &clickhouse::Client,
    database: &str,
) -> Result<SchemaSummary, SchemaError> {
    aml_warehouse::initialize(
        &Client::from_inner(client.clone(), ""),
        database,
        aml_warehouse::Network::Bsc,
    )
    .await
}
