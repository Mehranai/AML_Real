use super::*;
fn config(path: PathBuf) -> Config {
    Config {
        directory: path,
        page_size: 2,
        poll_seconds: 1,
        refresh_seconds: 10,
        job_timeout_seconds: 60,
        retry_seconds: 1,
        max_attempts: 2,
    }
}
fn job(id: &str) -> Job {
    Job {
        id: id.into(),
        address: "1BoatSLRHtKNngkdXEeobR76b53LETtpyT".into(),
        status: "pending".into(),
        attempts: 0,
        next_attempt_unix_ms: 0,
        last_error: String::new(),
    }
}
#[test]
fn failed_job_backoff_does_not_block_new_jobs_and_eventually_requires_review() {
    let c = config(std::env::temp_dir());
    let mut s = State::new("unit", &c);
    s.queue = vec![job("bad"), job("good")];
    s.queue[0].attempts = 1;
    s.finish(0, Err(anyhow::anyhow!("synthetic failure")), &c, 1000);
    assert_eq!(s.queue[0].status, "retry");
    assert_eq!(s.due(1000), Some(1));
    s.queue[1].attempts = 1;
    s.finish(1, Ok(json!({"verified":true})), &c, 1100);
    assert_eq!(s.succeeded, 1);
    assert_eq!(s.due(2000), None);
    assert_eq!(s.due(3000), Some(0));
    s.queue[0].attempts = 2;
    s.finish(0, Err(anyhow::anyhow!("still unavailable")), &c, 3000);
    assert_eq!(s.queue[0].status, "failed");
    assert_eq!(s.due(u64::MAX), None);
    assert_eq!(s.recent[0]["job_id"], "good");
}
#[test]
fn checkpoint_is_scoped_atomic_checked_and_does_not_trust_stale_heartbeat() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("btc-analytics-checkpoint-{}", std::process::id()));
    let c = config(directory.clone());
    let mut s = State::new("fixture", &c);
    checkpoint::save(&directory, &mut s, 1000)?;
    assert_eq!(
        checkpoint::load(&directory, "fixture")?.unwrap().generation,
        1
    );
    assert_eq!(
        status_at(&directory, "fixture", 1000)["heartbeat_fresh"],
        true
    );
    assert_eq!(
        status_at(&directory, "fixture", 999)["heartbeat_fresh"],
        false
    );
    assert_eq!(status_at(&directory, "fixture", 100000)["phase"], "failed");
    assert_eq!(
        status_at(&directory, "another", 1000)["reason"],
        "not_started"
    );
    std::fs::write(
        directory.join("bitcoin-analytics-fixture.json.tmp"),
        b"interrupted write",
    )?;
    assert_eq!(
        checkpoint::load(&directory, "fixture")?.unwrap().generation,
        1
    );
    std::fs::write(directory.join("bitcoin-analytics-fixture.json"), b"{broken")?;
    assert_eq!(
        status_at(&directory, "fixture", 1000)["reason"],
        "checkpoint_invalid_or_unreadable"
    );
    assert!(checkpoint::load(&directory, "../unsafe").is_err());
    std::fs::remove_file(directory.join("bitcoin-analytics-fixture.json"))?;
    std::fs::remove_file(directory.join("bitcoin-analytics-fixture.json.tmp"))?;
    std::fs::remove_dir(directory)?;
    Ok(())
}
#[test]
fn limits_versioned_configuration_and_cli_reject_unsafe_inputs() {
    let mut c = config(std::env::temp_dir());
    let hash = c.hash();
    c.page_size = 101;
    assert!(c.validate().is_err());
    c.page_size = 2;
    c.refresh_seconds = 11;
    assert_ne!(hash, c.hash());
    for args in [
        vec!["once", "--max-jobs", "0"],
        vec!["approve"],
        vec!["retry", "bad/id"],
        vec!["status", "extra"],
    ] {
        assert!(cli::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
    }
    assert_eq!(cli::parse(&[]).unwrap(), cli::Command::Run);
    assert!(!cli::healthy(
        &json!({"heartbeat_fresh":true,"reason":"stopped"})
    ));
}
#[test]
fn restart_recovers_interrupted_work_and_configuration_changes_resweep() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("btc-analytics-resume-{}", std::process::id()));
    let database = format!("worker_resume_{}", std::process::id());
    let mut c = config(directory.clone());
    let mut s = State::new(&database, &c);
    let mut j = job("interrupted");
    j.status = "running".into();
    j.attempts = 1;
    s.queue.push(j);
    checkpoint::save(&directory, &mut s, 1000)?;
    let worker = Worker::open(&database, c.clone())?;
    assert_eq!(worker.report()["queue"][0]["status"], "retry");
    assert!(Worker::open(&database, c.clone()).is_err());
    let independent = crate::writer_lock::WriterLock::for_database(&database)?;
    drop(independent);
    drop(worker);
    c.refresh_seconds = 12;
    let mut worker = Worker::open(&database, c.clone())?;
    assert_eq!(worker.report()["queue"].as_array().unwrap().len(), 0);
    assert_eq!(worker.report()["abandoned_jobs"], 1);
    worker.state.queue.push(Job {
        status: "running".into(),
        attempts: 2,
        ..job("last-interrupted")
    });
    worker.pause()?;
    drop(worker);
    let mut worker = Worker::open(&database, c)?;
    assert_eq!(worker.report()["queue"][0]["status"], "failed");
    worker.retry("last-interrupted")?;
    assert_eq!(worker.report()["queue"][0]["attempts"], 0);
    worker.pause()?;
    drop(worker);
    std::fs::remove_file(directory.join(format!("bitcoin-analytics-{database}.json")))?;
    std::fs::remove_dir(directory)?;
    Ok(())
}
