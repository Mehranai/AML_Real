//! Uses only a disposable ClickHouse with explicit fixture credentials.
use aml_warehouse::{Connection, Network, initialize, validate};
use anyhow::Result;
use clickhouse::Client;

aml_warehouse::clickhouse_connection!(NativeConnection);

#[tokio::test]
#[ignore = "requires WAREHOUSE_TEST_URL pointing to a disposable ClickHouse"]
async fn native_driver_initialization_and_readonly_validation() -> Result<()> {
    let client = Client::default()
        .with_url(std::env::var("WAREHOUSE_TEST_URL")?)
        .with_user("warehouse_test")
        .with_password("warehouse-test-only");
    let database = "warehouse_native_bsc";
    assert_eq!(
        NativeConnection(&client)
            .query("SELECT 'literal ? ?? ?fields'")
            .await?,
        vec!["literal ? ?? ?fields".to_owned()]
    );
    assert_eq!(
        NativeConnection(&client)
            .query_bounded("SELECT 'bounded ?' SETTINGS max_execution_time=2,max_threads=1")
            .await?,
        vec!["bounded ?".to_owned()]
    );
    initialize(&NativeConnection(&client), database, Network::Bsc).await?;
    let state = aml_warehouse::publication::State {
        network_id: Network::Bsc.id().into(),
        revision: 1,
        ..Default::default()
    };
    aml_warehouse::publication::insert(
        &NativeConnection(&client),
        database,
        "aml_sync_state",
        &[serde_json::to_value(&state)?],
    )
    .await?;
    assert_eq!(
        aml_warehouse::publication::state(&NativeConnection(&client), database, Network::Bsc)
            .await?,
        state
    );
    let readonly = client.with_option("readonly", "1");
    let summary = validate(&NativeConnection(&readonly), database, Network::Bsc).await?;
    assert!(!summary.newly_applied);
    assert!(
        initialize(&NativeConnection(&readonly), database, Network::Bsc)
            .await
            .is_err()
    );
    Ok(())
}
