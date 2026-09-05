use std::str::FromStr;

use clap::Parser;
use ethereum_aml::{
    config::AppConfig,
    db::initialize_ethereum_schema,
    domain::{AddressId, NetworkId},
    init_tracing,
    risk::EvidenceRiskEngine,
};

#[derive(Debug, Parser)]
#[command(
    name = "ethereum_assess_wallet",
    about = "Create and persist an explainable evidence-policy risk assessment"
)]
struct Cli {
    #[arg(long)]
    address: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config = AppConfig::from_env()?;
    initialize_ethereum_schema(&config).await?;
    let network = NetworkId::from_str(&config.eth_network_id)?;
    let address = AddressId::parse_evm(network, &cli.address)?
        .address()
        .to_string();
    let assessment = EvidenceRiskEngine::new(&config).assess(&address).await?;
    println!("{}", serde_json::to_string_pretty(&assessment)?);
    Ok(())
}
