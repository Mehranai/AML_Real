use anyhow::{Context, Result, bail, ensure};
use bsc_aml::{
    config::ClickHouseConfig,
    db::{validate_bsc_schema, warehouse::Warehouse},
    intelligence, metadata,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args[0] == "--help" {
        println!(
            "Usage: bsc_intelligence import-labels|import-metadata FILE.jsonl [--dry-run]\n       bsc_intelligence candidates|claims ADDRESS"
        );
        return Ok(());
    }
    ensure!((2..=3).contains(&args.len()), "invalid arguments");
    let config = ClickHouseConfig::from_env()?;
    validate_bsc_schema(&config).await?;
    let db = Warehouse::new(config)?;
    match args[0].as_str() {
        "import-labels" | "import-metadata" => {
            ensure!(
                args.len() == 2 || args[2] == "--dry-run",
                "expected --dry-run"
            );
            let file = std::fs::File::open(&args[1])?;
            ensure!(
                file.metadata()?.len() <= 16 * 1024 * 1024,
                "import file too large"
            );
            use std::io::{BufRead, BufReader};
            let rows = BufReader::new(file)
                .lines()
                .filter_map(|r| match r {
                    Ok(s) if s.trim().is_empty() => None,
                    other => Some(other),
                })
                .map(|r| Ok(serde_json::from_str::<serde_json::Value>(&r?)?))
                .collect::<Result<Vec<_>>>()?;
            let count = if args[0] == "import-labels" {
                intelligence::import(
                    &db,
                    rows.into_iter()
                        .map(serde_json::from_value)
                        .collect::<Result<Vec<_>, _>>()?,
                    args.len() == 3,
                )
                .await?
            } else {
                metadata::import_metadata(&db, rows, args.len() == 3).await?
            };
            println!(
                "{}",
                serde_json::json!({"validated":count,"written":args.len()==2})
            );
        }
        "candidates" | "claims" => {
            ensure!(args.len() == 2, "unexpected argument");
            let address =
                bsc_aml::domain::normalize_evm_address(&args[1]).context("invalid address")?;
            let rows = if args[0] == "candidates" {
                intelligence::candidates(&db, &address).await?
            } else {
                db.rows("SELECT * FROM intelligence_claims_current WHERE address={address:String} ORDER BY revision DESC LIMIT 100",&[("address",&address)]).await?
            };
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        _ => bail!("unknown command"),
    }
    Ok(())
}
