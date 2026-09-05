use std::process::ExitCode;

use bsc_aml::runtime::BscRuntime;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("BSC replay failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(target) = Target::parse()? else {
        return Ok(());
    };
    let runtime = BscRuntime::connect().await?;
    let report = match target {
        Target::Hash(hash) => runtime.ingestor.replay_hash(&hash).await?,
        Target::Range { start, end } => runtime.ingestor.replay_range(start, end).await?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

enum Target {
    Hash(String),
    Range { start: u64, end: u64 },
}

impl Target {
    fn parse() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let mut hash = None;
        let mut start = None;
        let mut end = None;
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--hash" => hash = Some(arguments.next().ok_or("--hash requires a block hash")?),
                "--start" => start = Some(parse_u64("--start", arguments.next())?),
                "--end" => end = Some(parse_u64("--end", arguments.next())?),
                "--help" | "-h" => {
                    println!("Usage: bsc_replay (--hash BLOCK_HASH | --start BLOCK [--end BLOCK])");
                    return Ok(None);
                }
                _ => return Err(format!("unknown argument: {argument}").into()),
            }
        }
        match (hash, start, end) {
            (Some(hash), None, None) => Ok(Some(Self::Hash(hash))),
            (None, Some(start), end) => {
                let end = end.unwrap_or(start);
                if start > end {
                    return Err("--start must not exceed --end".into());
                }
                Ok(Some(Self::Range { start, end }))
            }
            _ => Err("choose exactly one replay target: --hash or --start [--end]".into()),
        }
    }
}

fn parse_u64(name: &str, value: Option<String>) -> Result<u64, Box<dyn std::error::Error>> {
    value
        .ok_or_else(|| format!("{name} requires a decimal UInt64"))?
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a decimal UInt64").into())
}
