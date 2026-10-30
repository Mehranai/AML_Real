use anyhow::{Context, Result};
use bitcoin_aml::{
    api,
    config::{self, DatabaseConfig, IngestConfig, PipelineConfig},
    db::Database,
    failures, ingestion, probe, repair,
    rpc::{Rpc, RpcPool},
    warehouse,
};

#[tokio::main]
async fn main() -> Result<std::process::ExitCode> {
    let _ = dotenvy::dotenv();
    let mode = std::env::args().nth(1).unwrap_or_else(|| "api".into());
    if mode == "analytics" {
        use bitcoin_aml::analytics::{self, cli::Command};
        let command = analytics::cli::parse(&std::env::args().skip(2).collect::<Vec<_>>())?;
        if command == Command::Help {
            println!("{}", analytics::USAGE);
            return Ok(std::process::ExitCode::SUCCESS);
        }
        if matches!(command, Command::Status | Command::Healthcheck) {
            let status = analytics::local_status("bitcoin_aml");
            let healthy = analytics::cli::healthy(&status);
            println!("{}", serde_json::to_string_pretty(&status)?);
            return Ok(std::process::ExitCode::from(u8::from(
                command == Command::Healthcheck && !healthy,
            )));
        }
        config::required("BITCOIN_WRITER_LOCK_DIR")?;
        let once = matches!(command, Command::Once(_));
        let value = analytics::cli::run(
            Database::new(DatabaseConfig::from_env()?)?,
            analytics::Config::from_env()?,
            command,
            shutdown(),
        )
        .await?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(std::process::ExitCode::from(
            if once && value["attempt_failures"].as_u64().unwrap_or(0) > 0 {
                2
            } else {
                0
            },
        ));
    }
    if mode == "clusters"
        && matches!(
            std::env::args().nth(2).as_deref(),
            None | Some("--help" | "-h")
        )
    {
        println!("{}", bitcoin_aml::clustering::cli::USAGE);
        return Ok(std::process::ExitCode::SUCCESS);
    }
    if mode == "risk"
        && matches!(
            std::env::args().nth(2).as_deref(),
            None | Some("--help" | "-h")
        )
    {
        println!(
            "Usage: bitcoin_aml risk ADDRESS\n       bitcoin_aml risk inspect ADDRESS ASSESSMENT_ID\n       bitcoin_aml risk policy\nStores and verifies the shared evidence policy in the Bitcoin VM ClickHouse. No RPC."
        );
        return Ok(std::process::ExitCode::SUCCESS);
    }
    if mode == "schema" && std::env::args().skip(2).collect::<Vec<_>>() == ["--print-sql"] {
        print!("{}", bitcoin_aml::db::schema_sql()?);
        return Ok(std::process::ExitCode::SUCCESS);
    }
    if mode == "intelligence"
        && matches!(
            std::env::args().nth(2).as_deref(),
            None | Some("--help" | "-h")
        )
    {
        println!("{}", bitcoin_aml::intelligence::cli::USAGE);
        return Ok(std::process::ExitCode::SUCCESS);
    }
    if mode == "exposure"
        && matches!(
            std::env::args().nth(2).as_deref(),
            None | Some("--help" | "-h")
        )
    {
        println!(
            "Usage: bitcoin_aml exposure ADDRESS\n       bitcoin_aml exposure inspect ADDRESS RUN_ID\nBounded canonical UTXO exposure; stores run and evidence in the Bitcoin VM ClickHouse. No RPC."
        );
        return Ok(std::process::ExitCode::SUCCESS);
    }
    let tool_args = if matches!(mode.as_str(), "failures" | "repair" | "scan-gaps") {
        let args = std::env::args().skip(2).collect::<Vec<_>>();
        if args == ["--help"] || args == ["-h"] {
            println!("{}", repair::Arguments::usage(&mode));
            return Ok(std::process::ExitCode::SUCCESS);
        }
        Some(repair::Arguments::parse(&mode, &args)?)
    } else {
        None
    };
    if mode == "probe" {
        let args = std::env::args().skip(2).collect::<Vec<_>>();
        if args == ["--help"] || args == ["-h"] {
            println!(
                "Usage: bitcoin_aml probe [--compact]\nReads BITCOIN_RPC_* and ingestion range settings; does not access ClickHouse. Exit: 0 ready, 2 degraded, 1 unready."
            );
            return Ok(std::process::ExitCode::SUCCESS);
        }
        anyhow::ensure!(
            args.is_empty() || args == ["--compact"],
            "usage: bitcoin_aml probe [--compact]"
        );
        let report = match (Rpc::probe_from_env(), IngestConfig::from_env()) {
            (Ok(rpc), Ok(config)) => probe::inspect(&rpc, &config).await,
            _ => probe::Report::configuration_error(),
        };
        println!(
            "{}",
            if args.is_empty() {
                serde_json::to_string_pretty(&report)?
            } else {
                serde_json::to_string(&report)?
            }
        );
        return Ok(std::process::ExitCode::from(report.exit_code()));
    }
    if mode == "--healthcheck" {
        let port = config::setting("BITCOIN_API_ADDR", "0.0.0.0:6101")
            .parse::<std::net::SocketAddr>()?
            .port();
        config::http(4)?
            .get(format!("http://127.0.0.1:{port}/ready"))
            .send()
            .await?
            .error_for_status()?;
        return Ok(std::process::ExitCode::SUCCESS);
    }
    let db = Database::new(DatabaseConfig::from_env()?)?;
    match mode.as_str() {
        "clusters" => {
            let value = bitcoin_aml::clustering::cli::run(
                &db,
                &std::env::args().skip(2).collect::<Vec<_>>(),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        "risk" => {
            db.check_schema().await?;
            let args = std::env::args().skip(2).collect::<Vec<_>>();
            let value = if args == ["policy"] {
                bitcoin_aml::risk::read_policy(&db, aml_risk_policy::POLICY).await?
            } else if args.len() == 1 {
                let address = bitcoin_aml::model::address(&args[0])?;
                let data = api::investigation(&db, &address, 40).await?;
                serde_json::json!({"network_id":bitcoin_aml::NETWORK_ID,"address":address,"risk_engine":data["risk_engine"],"risk_snapshot":data["risk_snapshot"],"risk_persistence":data["risk_persistence"]})
            } else {
                anyhow::ensure!(
                    args.len() == 3 && args[0] == "inspect",
                    "usage: bitcoin_aml risk ADDRESS | inspect ADDRESS ASSESSMENT_ID | policy"
                );
                bitcoin_aml::risk::inspect(&db, &args[1], &args[2]).await?
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        "exposure" => {
            db.check_schema().await?;
            let args = std::env::args().skip(2).collect::<Vec<_>>();
            let value = if args.len() == 1 {
                bitcoin_aml::exposure::compute(&db, &args[0], Default::default())
                    .await?
                    .value()
            } else {
                anyhow::ensure!(
                    args.len() == 3 && args[0] == "inspect",
                    "usage: bitcoin_aml exposure ADDRESS | inspect ADDRESS RUN_ID"
                );
                bitcoin_aml::exposure::inspect(&db, &args[1], &args[2]).await?
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        "intelligence" => {
            let result = bitcoin_aml::intelligence::cli::run(
                &db,
                &std::env::args().skip(2).collect::<Vec<_>>(),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "failures" => {
            db.ready().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &failures::list(&db, &tool_args.unwrap().query).await?
                )?
            );
        }
        "scan-gaps" => {
            config::required("BITCOIN_WRITER_LOCK_DIR")?;
            let args = tool_args.unwrap();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &repair::scan(
                        &db,
                        args.query.from_block.unwrap(),
                        args.query.to_block.unwrap(),
                        args.max_blocks.unwrap()
                    )
                    .await?
                )?
            );
        }
        "repair" => {
            config::required("BITCOIN_WRITER_LOCK_DIR")?;
            let args = tool_args.unwrap();
            let report = repair::run(
                db,
                RpcPool::from_env()?,
                IngestConfig::from_env()?,
                PipelineConfig::from_env()?,
                repair::Options {
                    ids: args.ids,
                    max_blocks: args.max_blocks.unwrap(),
                    policy: failures::Policy::from_env()?,
                },
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(std::process::ExitCode::from(u8::from(
                report["status"] != "complete",
            )));
        }
        "schema" => {
            anyhow::ensure!(
                std::env::args().nth(3).is_none(),
                "too many schema arguments"
            );
            match std::env::args().nth(2).as_deref() {
                None => {
                    println!(
                        "{}",
                        serde_json::to_string(
                            &bitcoin_aml::db::migrations::schema_command(&db, false).await?
                        )?
                    );
                }
                Some("--check") => {
                    println!(
                        "{}",
                        serde_json::to_string(
                            &bitcoin_aml::db::migrations::schema_command(&db, true).await?
                        )?
                    );
                }
                _ => anyhow::bail!("usage: bitcoin_aml schema [--check]"),
            }
        }
        "api" => {
            db.check_schema().await?;
            warehouse::migration_ready(&db).await?;
            let state = api::Api::new(db, config::required("AML_SERVICE_KEY")?)?;
            let address = config::setting("BITCOIN_API_ADDR", "127.0.0.1:6101")
                .parse::<std::net::SocketAddr>()
                .context("invalid BITCOIN_API_ADDR")?;
            let listener = tokio::net::TcpListener::bind(address).await?;
            axum::serve(listener, api::router(state))
                .with_graceful_shutdown(shutdown())
                .await?;
        }
        "warehouse-backfill" => {
            let args = std::env::args().skip(2).collect::<Vec<_>>();
            anyhow::ensure!(
                args.len() == 2 && args[0] == "--max-blocks",
                "usage: bitcoin_aml warehouse-backfill --max-blocks N (stop all ingestion writers first)"
            );
            config::required("BITCOIN_WRITER_LOCK_DIR")?;
            println!(
                "{}",
                warehouse::backfill(&db, args[1].parse().context("invalid --max-blocks")?).await?
            );
        }
        "ingest" | "once" => {
            config::required("BITCOIN_WRITER_LOCK_DIR")?;
            let work = ingestion::run_with_policy(
                db,
                RpcPool::from_env()?,
                IngestConfig::from_env()?,
                PipelineConfig::from_env()?,
                failures::Policy::from_env()?,
                mode == "once",
            );
            // Cancellation may leave unpublished facts, which are intentionally invisible.
            tokio::select! { result = work => result?, _ = shutdown() => {} }
        }
        _ => anyhow::bail!(
            "usage: bitcoin_aml [schema|warehouse-backfill --max-blocks N|analytics|intelligence|clusters|exposure|risk|probe [--compact]|failures|scan-gaps|repair|api|ingest|once|--healthcheck] (tools accept --help)"
        ),
    }
    Ok(std::process::ExitCode::SUCCESS)
}
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
