use std::env;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::services::tron::neo4j::{
    client::Neo4jClient, flow_graph::build_wallet_flow_graph,
};
use arz_axum_for_services::utils::tron_address::normalize_tron_address;
use clickhouse::Client;

#[tokio::main]
async fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();

    if args.len() < 2 {
        return Err(anyhow!(
            "usage: cargo run --bin tron_export_wallet_graph -- <tron_wallet_address> [depth] [per_address_limit]"
        ));
    }

    let address = normalize_tron_address(&args[1])
        .ok_or_else(|| anyhow!("invalid Tron wallet address: {}", args[1]))?;

    let depth = optional_arg(&args, 2, 3)?;
    let limit = optional_arg(&args, 3, 500)?;

    let config = AppConfig::from_env();
    let clickhouse = Arc::new(
        Client::default()
            .with_url(&config.clickhouse_url)
            .with_user(&config.clickhouse_user)
            .with_password(&config.clickhouse_pass)
            .with_database(&config.clickhouse_db_tron),
    );

    let neo4j = Neo4jClient::new(
        &config.neo4j_uri,
        &config.neo4j_username,
        &config.neo4j_password,
    )
    .await
    .with_context(|| {
        format!(
            "Neo4j connection failed; check NEO4J/Bolt endpoint `{}`",
            config.neo4j_uri
        )
    })?;

    let graph = build_wallet_flow_graph(clickhouse, Some(&neo4j), &address, depth, limit)
        .await
        .with_context(|| {
            format!(
                "failed to export wallet graph from ClickHouse `{}` to Neo4j `{}`",
                config.clickhouse_url, config.neo4j_uri
            )
        })?;

    println!("Imported TRON wallet graph into Neo4j");
    println!("wallet: {}", graph.address);
    println!("depth: {}", graph.depth);
    println!("wallet nodes: {}", graph.neo4j.imported_wallet_nodes);
    println!("transfer edges: {}", graph.neo4j.imported_transfer_edges);
    println!(
        "exchange interactions: {}",
        graph.neo4j.imported_exchange_interactions
    );
    println!("Neo4j Browser: {}", graph.neo4j.browser_url);
    println!("Cypher:");
    println!("{}", graph.neo4j.cypher);

    Ok(())
}

fn optional_arg<T>(args: &[String], index: usize, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match args.get(index) {
        Some(value) => value
            .parse::<T>()
            .map_err(|err| anyhow!("invalid argument at position {}: {}", index, err)),
        None => Ok(default),
    }
}
