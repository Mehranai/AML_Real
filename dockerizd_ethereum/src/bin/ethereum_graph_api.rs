use ethereum_aml::{
    api::build_router, config::AppConfig, db::initialize_ethereum_schema, init_tracing,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config = AppConfig::from_env()?;
    initialize_ethereum_schema(&config).await?;
    let bind_address = config.ethereum_api_addr.clone();
    let router = build_router(config).await?;
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;

    tracing::info!(address = %bind_address, "Ethereum AML dashboard listening");
    axum::serve(listener, router).await?;
    Ok(())
}
