use ethereum_aml::{
    clustering::discover_address_clusters, config::AppConfig, db::initialize_ethereum_schema,
    init_tracing,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config = AppConfig::from_env()?;
    initialize_ethereum_schema(&config).await?;
    let report = discover_address_clusters(&config).await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
