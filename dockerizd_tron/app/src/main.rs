use anyhow::Result;

use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::tasks::fetch_loop::run_tron_loop;

// نقطه شروع برنامه
#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|v| v == "maintenance") {
        if args.len() == 1 || args[1..] == ["--help"] {
            println!("{}", aml_warehouse::maintenance::Command::help());
            return Ok(());
        }
        let command = aml_warehouse::maintenance::Command::parse(&args[1..])?;
        let value = arz_axum_for_services::operations::run(AppConfig::from_env(), &command).await?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        anyhow::ensure!(
            command.action != "probe" || value["status"] == "ready",
            "RPC probe requires review; inspect the JSON report"
        );
        return Ok(());
    }
    anyhow::ensure!(
        args.is_empty(),
        "unknown ingestor arguments; use maintenance --help"
    );
    run_tron_loop(AppConfig::from_env()).await
}
