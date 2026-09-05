use anyhow::Result;
use arz_axum_for_services::{
    config::AppConfig, db::tron_schema::validate_tron_schema, router::build_router,
};
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let config = AppConfig::from_env();
    let admin_client = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass);
    validate_tron_schema(&admin_client).await?;

    let bind_addr =
        std::env::var("TRON_GRAPH_API_ADDR").unwrap_or_else(|_| "127.0.0.1:4001".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    println!("TRON AML dashboard listening on http://{}", bind_addr);

    axum::serve(listener, build_router()?).await?;

    Ok(())
}
