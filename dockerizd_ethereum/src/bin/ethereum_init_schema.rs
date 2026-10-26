use ethereum_aml::{
    config::AppConfig,
    db::{check_ethereum_schema, initialize_ethereum_schema},
    init_tracing,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some("--help" | "-h") => {
            println!("Usage: ethereum_init_schema [--check]");
            return Ok(());
        }
        _ => anyhow::bail!("usage: ethereum_init_schema [--check]"),
    };
    anyhow::ensure!(
        std::env::args().nth(2).is_none(),
        "too many schema arguments"
    );
    let config = AppConfig::from_env()?;

    if check {
        check_ethereum_schema(&config).await?;
    } else {
        initialize_ethereum_schema(&config).await?;
    }
    println!(
        "Ethereum legacy and shared warehouse schema verified for {}.",
        config.clickhouse_database
    );

    Ok(())
}
