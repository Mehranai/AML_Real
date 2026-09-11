use anyhow::{Result, ensure};
use bsc_aml::{
    config::{AppConfig, ClickHouseConfig},
    db::{validate_bsc_schema, warehouse::Warehouse},
    investigation::api::shutdown,
    metadata::{TokenRpc, worker_once},
};
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|v| v == "--help") {
        println!("Usage: bsc_token_metadata_worker [--follow]");
        return Ok(());
    }
    ensure!(args.is_empty() || args == ["--follow"], "expected --follow");
    let config = ClickHouseConfig::from_env()?;
    validate_bsc_schema(&config).await?;
    let db = Warehouse::new(config)?;
    let rpc = TokenRpc::new(AppConfig::from_env()?)?;
    loop {
        tokio::select! {
            _=shutdown()=>return Ok(()),
            result=worker_once(&db,&rpc)=>{
                match result {Ok(count)=>tracing::info!(count,"metadata batch complete"),
                    Err(error)=>{if args.is_empty(){return Err(error);}tracing::warn!(%error,"metadata batch failed");}}
            }
        }
        if args.is_empty() {
            return Ok(());
        }
        tokio::select! {_=shutdown()=>return Ok(()),_=tokio::time::sleep(std::time::Duration::from_secs(60))=>{}}
    }
}
