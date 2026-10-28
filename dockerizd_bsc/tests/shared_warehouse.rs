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
        &[aml_warehouse::core_schema::extend(
            "aml_sync_state",
            serde_json::to_value(&state)?,
        )?],
    )
    .await?;
    assert_eq!(
        aml_warehouse::publication::state(&NativeConnection(&client), database, Network::Bsc)
            .await?,
        state
    );
    for view in aml_warehouse::core::View::ALL {
        let rows = aml_warehouse::core::read(
            &NativeConnection(&client),
            database,
            Network::Bsc,
            view,
            &Default::default(),
        )
        .await?;
        assert_eq!(
            rows.len(),
            usize::from(matches!(view, aml_warehouse::core::View::State))
        );
    }
    for table in [
        "aml_asset_metadata",
        "aml_protocol_registry",
        "aml_entity_addresses",
        "aml_cluster_memberships",
        "aml_wallet_risk_signals",
    ] {
        assert!(
            aml_warehouse::analytics::read(
                &NativeConnection(&client),
                database,
                Network::Bsc,
                table,
                aml_warehouse::analytics::Mode::Current,
                10
            )
            .await?
            .is_empty()
        );
    }
    let readonly = client.with_option("readonly", "1");
    let subject = aml_warehouse::relationships::Subject::address(
        Network::Bsc,
        &format!("0x{}", "1".repeat(40)),
    )?;
    let empty = aml_warehouse::relationships::read_neighborhood(
        &NativeConnection(&readonly),
        database,
        Network::Bsc,
        &subject,
        Default::default(),
    )
    .await?;
    assert_eq!(empty.graph.nodes.len(), 1);
    assert!(empty.graph.edges.is_empty());
    let query = aml_warehouse::relationships::query_template()
        .lines()
        .filter(|l| !l.starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("{{database}}", database)
        .replace("{{network}}", &format!("'{}'", Network::Bsc.id()))
        .replace("{{projection}}", "r.relationship_id")
        .replace("{{selector}}", "1=0")
        .replace("{{witness_selector}}", "1=0")
        .replace("{{limit}}", "1");
    assert!(NativeConnection(&readonly).query(&query).await?.is_empty());
    let summary = validate(&NativeConnection(&readonly), database, Network::Bsc).await?;
    assert!(!summary.newly_applied);
    assert!(
        initialize(&NativeConnection(&readonly), database, Network::Bsc)
            .await
            .is_err()
    );
    Ok(())
}
