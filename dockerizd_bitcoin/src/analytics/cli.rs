use super::*;
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run,
    Once(usize),
    Status,
    Healthcheck,
    Retry(String),
    Help,
}
pub fn parse(args: &[String]) -> Result<Command> {
    Ok(match args {
        [] => Command::Run,
        [v] if v == "run" => Command::Run,
        [v] if v == "--help" || v == "-h" => Command::Help,
        [v] if v == "status" => Command::Status,
        [v] if v == "healthcheck" => Command::Healthcheck,
        [v] if v == "once" => Command::Once(1),
        [v, flag, n] if v == "once" && flag == "--max-jobs" => {
            let n: usize = n.parse()?;
            ensure!((1..=100).contains(&n), "max jobs must be 1..100");
            Command::Once(n)
        }
        [v, id]
            if v == "retry"
                && id
                    .strip_prefix("btc-job-")
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())) =>
        {
            Command::Retry(id.clone())
        }
        _ => anyhow::bail!("{USAGE}"),
    })
}
pub fn healthy(status: &Value) -> bool {
    status["heartbeat_fresh"] == true && status["reason"] != "stopped"
}
pub async fn run(
    db: Database,
    config: Config,
    command: Command,
    stop: impl std::future::Future<Output = ()>,
) -> Result<Value> {
    db.check_schema().await?;
    crate::warehouse::migration_ready(&db).await?;
    let mut worker = Worker::open(db.database(), config)?;
    if let Command::Retry(id) = &command {
        worker.retry(id)?;
        let report = worker.report();
        worker.pause()?;
        return Ok(report);
    }
    let max = match command {
        Command::Run => usize::MAX,
        Command::Once(n) => n,
        _ => anyhow::bail!("invalid worker command"),
    };
    let failures_before = worker.state.failures;
    let mut attempts = 0;
    tokio::pin!(stop);
    loop {
        let result =
            tokio::select! {result=worker.cycle(&db)=>result,_=&mut stop=>{worker.pause()?;break}};
        match result {
            Ok(ran) => {
                attempts += usize::from(ran);
                if max != usize::MAX && (!ran || attempts >= max) {
                    worker.pause()?;
                    break;
                }
            }
            Err(error) => {
                if max != usize::MAX {
                    worker.pause()?;
                    return Err(error);
                }
                eprintln!(
                    "Bitcoin analytics source/discovery unavailable; cursor retained: {error}"
                );
            }
        }
        tokio::select! {_=tokio::time::sleep(std::time::Duration::from_secs(worker.config.poll_seconds))=>{},_=&mut stop=>{worker.pause()?;break}}
    }
    let mut report = worker.report();
    report["attempted_jobs"] = json!(attempts);
    report["attempt_failures"] = json!(worker.state.failures.saturating_sub(failures_before));
    Ok(report)
}
