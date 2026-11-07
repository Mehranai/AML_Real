//! Fresh Ethereum deployments install only the common ClickHouse contract.
use crate::{
    config::AppConfig,
    db::{Client, database_client},
};
pub async fn initialize_ethereum_schema(config: &AppConfig) -> anyhow::Result<()> {
    let admin = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_password);
    aml_warehouse::initialize(
        &admin,
        &config.clickhouse_database,
        aml_warehouse::Network::Ethereum,
    )
    .await?;
    super::records::initialize_policy(&database_client(config)).await?;
    Ok(())
}
pub async fn check_ethereum_schema(config: &AppConfig) -> anyhow::Result<()> {
    aml_warehouse::validate(
        &database_client(config),
        &config.clickhouse_database,
        aml_warehouse::Network::Ethereum,
    )
    .await?;
    Ok(())
}
