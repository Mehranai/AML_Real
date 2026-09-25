use anyhow::{Result, bail};
use arz_axum_for_services::tasks::analytics::{Settings, read_status, run};

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() > 1 {
        bail!("usage: tron_analytics_worker [--once|--status|--healthcheck]");
    }
    let settings = Settings::from_env()?;
    match args.first().map(String::as_str) {
        None => run(settings, false).await,
        Some("--once") => run(settings, true).await,
        Some("--status") => {
            println!("{}", read_status(&settings, false)?);
            Ok(())
        }
        Some("--healthcheck") => {
            read_status(&settings, true)?;
            Ok(())
        }
        _ => bail!("usage: tron_analytics_worker [--once|--status|--healthcheck]"),
    }
}
