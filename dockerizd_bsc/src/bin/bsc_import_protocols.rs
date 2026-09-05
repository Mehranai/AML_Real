use std::{
    collections::HashSet,
    env,
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

use bsc_aml::{
    config::ClickHouseConfig,
    db::validate_bsc_schema,
    ingestion::ClickHouseIngestionStore,
    semantic::{ProtocolImportResult, ProtocolRegistryInput},
};
use serde::Serialize;

const MAX_RECORDS: usize = 100_000;
const MAX_LINE_BYTES: usize = 64 * 1024;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("BSC protocol registry import failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let file = File::open(&arguments.file)?;
    let mut inputs = Vec::new();
    let mut addresses = HashSet::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            return Err(format!("line {line_number} exceeds {MAX_LINE_BYTES} bytes").into());
        }
        if inputs.len() >= MAX_RECORDS {
            return Err(format!("input exceeds {MAX_RECORDS} records").into());
        }
        let input: ProtocolRegistryInput = serde_json::from_str(trimmed)
            .map_err(|error| format!("invalid JSON at line {line_number}: {error}"))?;
        let address = input
            .validate_only()
            .map_err(|error| format!("invalid record at line {line_number}: {error}"))?;
        if !addresses.insert(address) {
            return Err(format!("duplicate contract address at line {line_number}").into());
        }
        inputs.push(input);
    }
    if inputs.is_empty() {
        return Err("input contains no registry records".into());
    }

    if arguments.dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&ImportSummary {
                dry_run: true,
                imported: 0,
                validated: inputs.len(),
                records: Vec::new(),
            })?
        );
        return Ok(());
    }

    let config = ClickHouseConfig::from_env()?;
    validate_bsc_schema(&config).await?;
    let store = ClickHouseIngestionStore::new(&config);
    let mut records = Vec::with_capacity(inputs.len());
    for input in inputs {
        records.push(store.import_protocol_contract(input).await?);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&ImportSummary {
            dry_run: false,
            imported: records.len(),
            validated: records.len(),
            records,
        })?
    );
    Ok(())
}

struct Arguments {
    file: PathBuf,
    dry_run: bool,
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut file = None;
    let mut dry_run = false;
    let mut values = env::args().skip(1);
    while let Some(argument) = values.next() {
        match argument.as_str() {
            "--file" => {
                file = Some(PathBuf::from(
                    values.next().ok_or("--file requires a JSONL path")?,
                ));
            }
            "--dry-run" => dry_run = true,
            "--help" | "-h" => {
                println!(
                    "Usage: bsc_import_protocols --file <registry.jsonl> [--dry-run]\n\
                     Each non-empty line must be one reviewed protocol registry JSON object."
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(Arguments {
        file: file.ok_or("--file is required")?,
        dry_run,
    })
}

#[derive(Serialize)]
struct ImportSummary {
    dry_run: bool,
    validated: usize,
    imported: usize,
    records: Vec<ProtocolImportResult>,
}
