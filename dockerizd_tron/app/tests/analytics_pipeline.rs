//! Run only against a fresh disposable ClickHouse initialized from the TRON baseline.
use anyhow::{Result, ensure};
use arz_axum_for_services::{
    services::tron::wallet_exposure::load_wallet_exposure_summary,
    tasks::exposure_task::{run_all_exposure_scans, run_exposure_scan},
};
use clickhouse::Client;
use sha2::{Digest, Sha256};
use std::{process::Command, sync::Arc};

fn address(n: u8) -> String {
    let mut bytes = vec![0x41];
    bytes.extend([n; 20]);
    let checksum = Sha256::digest(Sha256::digest(&bytes));
    bytes.extend(&checksum[..4]);
    bs58::encode(bytes).into_string()
}

#[tokio::test]
#[ignore = "requires TRON_ANALYTICS_TEST_URL pointing to a fresh disposable ClickHouse"]
async fn worker_discovers_pending_claims_resumes_and_preserves_exposure_boundaries() -> Result<()> {
    let url = std::env::var("TRON_ANALYTICS_TEST_URL")?;
    let user = std::env::var("TRON_ANALYTICS_TEST_USER").unwrap_or_else(|_| "default".into());
    let password = std::env::var("TRON_ANALYTICS_TEST_PASSWORD").unwrap_or_default();
    let client = Arc::new(
        Client::default()
            .with_url(&url)
            .with_user(&user)
            .with_password(&password)
            .with_database("tron_db")
            .with_option("join_algorithm", "auto"),
    );
    let count = client
        .query("SELECT count() FROM address_relationships")
        .fetch_one::<u64>()
        .await?;
    ensure!(
        count == 0,
        "test requires an empty disposable database; refusing existing data"
    );
    let dir = std::env::temp_dir().join(format!("tron-analytics-test-{}", nanoid::nanoid!(12)));
    std::fs::create_dir_all(&dir)?;
    for block in 0..=4 {
        client.query("INSERT INTO ingested_blocks (block_number,block_hash,parent_hash,block_timestamp,transaction_count,finality_status,ingestion_status,indexed_at_unix_ms) VALUES (?,?,'parent',1700000000000,1,'SOLID','COMPLETE',1)")
            .bind(block as u64).bind(format!("hash-{block}")).execute().await?;
    }
    client
        .query("INSERT INTO sync_state (chain,last_synced_block) VALUES ('tron',4)")
        .execute()
        .await?;
    client.query("INSERT INTO exchange_addresses (address,entity_id,exchange_name,address_role,confidence,detection_source,first_seen_block,last_seen_block) VALUES (?,'fixture-exchange','Fixture exchange','HOT',0.99,'test',0,4)")
        .bind(address(4)).execute().await?;
    client.query("INSERT INTO exposure_seeds (address,entity_name,entity_type,risk_level,source) VALUES (?,'Fixture seed','SCAM',100,'fixture')")
        .bind(address(1)).execute().await?;
    let edges = [
        (1, 2, 1, "TRX"),
        (2, 3, 2, "TRX"),
        (2, 4, 3, "TRX"),
        (4, 5, 4, "TRX"),
        (2, 6, 4, "TOKEN"),
        (2, 7, 0, "TRX"),
        (7, 9, 1, "TRX"),
        (8, 9, 1, "TRX"),
        (9, 4, 2, "TRX"),
    ];
    for (id, (from, to, block, asset)) in edges.into_iter().enumerate() {
        client.query("INSERT INTO address_relationships (relationship_id,from_address,to_address,token_address,tx_hash,block_number,timestamp,amount,transfer_type) VALUES (?,?,?,?,?,?,1700000000000,100,'native_transfer')")
            .bind(format!("edge-{id}")).bind(address(from)).bind(address(to)).bind(asset)
            .bind(format!("tx-{id}")).bind(block as u64).execute().await?;
    }
    let run_worker = || -> Result<()> {
        let output = Command::new(env!("CARGO_BIN_EXE_tron_analytics_worker"))
            .arg("--once")
            .current_dir(&dir)
            .env("CLICKHOUSE_URL", &url)
            .env("CLICKHOUSE_USER", &user)
            .env("CLICKHOUSE_PASSWORD", &password)
            .env("CLICKHOUSE_DB_TRON", "tron_db")
            .env("TRON_ANALYTICS_STATE_DIR", &dir)
            .env("TRON_ANALYTICS_BATCH_BLOCKS", "2")
            .output()?;
        ensure!(
            output.status.success(),
            "worker failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    };
    for expected in [1, 3, 4] {
        run_worker()?;
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("progress.json"))?)?;
        assert_eq!(value["last_cluster_block"], expected);
    }
    let claim_count = client.query("SELECT count() FROM address_cluster_claims FINAL WHERE address=? AND review_status='PENDING'")
        .bind(address(9)).fetch_one::<u64>().await?;
    assert_eq!(
        claim_count, 1,
        "restart must not duplicate unchanged pending claims"
    );
    assert_eq!(
        client
            .query("SELECT count() FROM exchange_addresses FINAL WHERE address=?")
            .bind(address(9))
            .fetch_one::<u64>()
            .await?,
        0,
        "candidate must not be auto-approved"
    );
    let sources = load_wallet_exposure_summary(client.clone(), &address(3), None).await?;
    assert_eq!(sources.source_count, 1);
    assert_eq!(sources.min_hop_distance, Some(2));
    for n in [5, 6, 7] {
        assert_eq!(
            load_wallet_exposure_summary(client.clone(), &address(n), None)
                .await?
                .source_count,
            0,
            "must not cross a service, change assets, or travel backward in block order"
        );
    }
    assert!(
        load_wallet_exposure_summary(client.clone(), &address(4), None)
            .await?
            .top_sources[0]
            .service_mediated
    );
    client.query("INSERT INTO exposure_seeds (address,entity_name,entity_type,risk_level,source,is_active,created_at) VALUES (?,'Fixture seed','SCAM',100,'fixture',0,now()+INTERVAL 1 SECOND)")
        .bind(address(1)).execute().await?;
    assert_eq!(
        load_wallet_exposure_summary(client.clone(), &address(3), None)
            .await?
            .source_count,
        0
    );

    client.query("INSERT INTO address_relationships (relationship_id,from_address,to_address,token_address,tx_hash,block_number,timestamp,amount,transfer_type) SELECT concat('limit-',toString(number)),?,?,'TRX',concat('limit-tx-',toString(number)),1,1700000000000,100,'native_transfer' FROM numbers(2001)")
        .bind(address(20)).bind(address(21)).execute().await?;
    assert!(
        run_exposure_scan(client.clone(), &address(20), 5)
            .await
            .is_err()
    );
    assert_eq!(client.query("SELECT count() FROM exposure_runs FINAL WHERE source_address=? AND status='COMPLETE'")
        .bind(address(20)).fetch_one::<u64>().await?, 0, "truncated traversal must not publish COMPLETE");
    client.query("INSERT INTO exposure_seeds (address,entity_name,entity_type,risk_level,source) VALUES (?,'Large seed','SCAM',100,'fixture'),(?,'Small seed','SCAM',100,'fixture')")
        .bind(address(20)).bind(address(22)).execute().await?;
    client.query("INSERT INTO address_relationships (relationship_id,from_address,to_address,token_address,tx_hash,block_number,timestamp,amount,transfer_type) VALUES ('small-seed',?,?,'TRX','small-seed-tx',1,1700000000000,100,'native_transfer')")
        .bind(address(22)).bind(address(23)).execute().await?;
    assert!(run_all_exposure_scans(client.clone(), 5).await.is_err());
    assert_eq!(client.query("SELECT count() FROM exposure_runs FINAL WHERE source_address=? AND status='COMPLETE'")
        .bind(address(22)).fetch_one::<u64>().await?, 1, "one failed seed must not starve the remaining seeds");

    client.query("INSERT INTO intelligence_sources (chain,source_id,source_name,source_type,trust_tier,is_active,created_by,created_at_unix_ms) VALUES ('tron','tron_structural_heuristics_v1','disabled','HEURISTIC','UNVERIFIED',0,'test',1)").execute().await?;
    assert!(
        run_worker().is_err(),
        "analyst-disabled sources must not be silently enabled"
    );
    let health = Command::new(env!("CARGO_BIN_EXE_tron_analytics_worker"))
        .arg("--healthcheck")
        .env("TRON_ANALYTICS_STATE_DIR", &dir)
        .output()?;
    assert!(
        !health.status.success(),
        "failed stage must fail the Docker healthcheck"
    );
    // Remove only this test's generated state directory, never application state or data volumes.
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
