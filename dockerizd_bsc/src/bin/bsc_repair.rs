use std::process::ExitCode;

use bsc_aml::runtime::BscRuntime;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("BSC repair failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };
    let runtime = BscRuntime::connect().await?;
    let report = runtime
        .ingestor
        .repair_range(options.start, options.end, options.include_dead)
        .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

struct Options {
    start: u64,
    end: u64,
    include_dead: bool,
}

impl Options {
    fn parse() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let mut start = None;
        let mut end = None;
        let mut include_dead = false;
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--start" => start = Some(parse_u64("--start", arguments.next())?),
                "--end" => end = Some(parse_u64("--end", arguments.next())?),
                "--include-dead" => include_dead = true,
                "--help" | "-h" => {
                    println!("Usage: bsc_repair --start BLOCK --end BLOCK [--include-dead]");
                    return Ok(None);
                }
                _ => return Err(format!("unknown argument: {argument}").into()),
            }
        }
        let start = start.ok_or("--start is required")?;
        let end = end.unwrap_or(start);
        if start > end {
            return Err("--start must not exceed --end".into());
        }
        Ok(Some(Self {
            start,
            end,
            include_dead,
        }))
    }
}

fn parse_u64(name: &str, value: Option<String>) -> Result<u64, Box<dyn std::error::Error>> {
    value
        .ok_or_else(|| format!("{name} requires a decimal UInt64"))?
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a decimal UInt64").into())
}
