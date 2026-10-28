use anyhow::{Result, ensure};
use bitcoin_aml::{
    config::DatabaseConfig,
    db::{self, Database},
};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "Usage: bitcoin_init_schema [--check|--print-sql]\nApplies shared migrations v1 through v5; --check is read-only, --print-sql needs no database configuration."
        );
        return Ok(());
    }
    if args == ["--print-sql"] {
        print!("{}", db::schema_sql()?);
        return Ok(());
    }
    ensure!(
        args.is_empty() || args == ["--check"],
        "usage: bitcoin_init_schema [--check|--print-sql]"
    );
    let db = Database::new(DatabaseConfig::from_env()?)?;
    println!(
        "{}",
        serde_json::to_string(&db::migrations::schema_command(&db, !args.is_empty()).await?)?
    );
    Ok(())
}
