use anyhow::{Result, ensure};
use arz_axum_for_services::{
    config::AppConfig,
    db::tron_schema::{check_tron_schema, validate_tron_schema},
};
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some("--help" | "-h") => {
            println!("Usage: tron_schema [--check]");
            return Ok(());
        }
        _ => anyhow::bail!("usage: tron_schema [--check]"),
    };
    ensure!(
        std::env::args().nth(2).is_none(),
        "too many schema arguments"
    );
    let config = AppConfig::from_env();
    let client = Client::default()
        .with_url(&config.clickhouse_url)
        .with_user(&config.clickhouse_user)
        .with_password(&config.clickhouse_pass);
    if check {
        check_tron_schema(&client).await?;
    } else {
        validate_tron_schema(&client).await?;
    }
    println!(
        "shared warehouse contract v{} verified for TRON",
        aml_warehouse::current_contract()?.version
    );
    Ok(())
}
