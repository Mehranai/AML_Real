use ethereum_aml::{config::AppConfig, db::initialize_ethereum_schema, init_tracing};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config = AppConfig::from_env()?;

    initialize_ethereum_schema(&config).await?;
    println!(
        "Ethereum schema migrations completed for {}.",
        config.clickhouse_database
    );

    Ok(())
}
