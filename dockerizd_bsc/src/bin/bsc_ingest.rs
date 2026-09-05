use std::process::ExitCode;

use bsc_aml::{config::SyncMode, runtime::BscRuntime};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("BSC canonical ingestion failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };
    let runtime = BscRuntime::connect().await?;
    let mode = options.mode.unwrap_or(runtime.ingestion_config.sync_mode);
    match mode {
        SyncMode::Range => {
            let start = options
                .start
                .or(runtime.ingestion_config.start_block)
                .ok_or("range mode requires --start or BSC_INGEST_START_BLOCK")?;
            let end = options
                .end
                .or(runtime.ingestion_config.end_block)
                .unwrap_or(start);
            let report = runtime.ingestor.ingest_range(start, end).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        SyncMode::Auto => {
            reject_end(&options, &runtime.ingestion_config)?;
            let report = runtime
                .ingestor
                .sync_once(options.start, options.max_blocks)
                .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        SyncMode::Follow => {
            reject_end(&options, &runtime.ingestion_config)?;
            let report = runtime
                .ingestor
                .follow(options.start, options.max_blocks)
                .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn reject_end(
    options: &Options,
    config: &bsc_aml::config::IngestionConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if options.end.is_some() || config.end_block.is_some() {
        return Err("--end/BSC_INGEST_END_BLOCK is valid only in range mode".into());
    }
    Ok(())
}

struct Options {
    mode: Option<SyncMode>,
    start: Option<u64>,
    end: Option<u64>,
    max_blocks: Option<u64>,
}

impl Options {
    fn parse() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let mut options = Self {
            mode: None,
            start: None,
            end: None,
            max_blocks: None,
        };
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--mode" => {
                    let value = arguments
                        .next()
                        .ok_or("--mode requires range, auto, or follow")?;
                    options.mode = Some(SyncMode::parse(&value)?);
                }
                "--start" => options.start = Some(parse_u64("--start", arguments.next())?),
                "--end" => options.end = Some(parse_u64("--end", arguments.next())?),
                "--max-blocks" => {
                    let value = parse_u64("--max-blocks", arguments.next())?;
                    if value == 0 {
                        return Err("--max-blocks must be greater than zero".into());
                    }
                    options.max_blocks = Some(value);
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: bsc_ingest [--mode range|auto|follow] [--start BLOCK] [--end BLOCK] [--max-blocks COUNT]\n\
                         range reads a fixed finalized range without moving the cursor; auto resumes one bounded batch; follow continuously resumes and polls."
                    );
                    return Ok(None);
                }
                _ => return Err(format!("unknown argument: {argument}").into()),
            }
        }
        Ok(Some(options))
    }
}

fn parse_u64(name: &'static str, value: Option<String>) -> Result<u64, Box<dyn std::error::Error>> {
    value
        .ok_or_else(|| format!("{name} requires a decimal UInt64"))?
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a decimal UInt64").into())
}
