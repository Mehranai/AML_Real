//! Only a fresh, disposable ClickHouse. No external intelligence is downloaded.
use anyhow::Result;
use bitcoin_aml::{
    NETWORK_ID, api,
    config::DatabaseConfig,
    db::Database,
    intelligence::{self, Claim, Review, Source},
};
use serde_json::{Value, json};
use tower::ServiceExt;
const ADDRESS: &str = "1BoatSLRHtKNngkdXEeobR76b53LETtpyT";
fn source() -> Source {
    serde_json::from_value(json!({"network_id":NETWORK_ID,"source_id":"fixture-source","source_name":"Synthetic test source","source_type":"test_fixture","reference_url":"https://example.invalid/fixture","trust_tier":"unrated","license":"test only","is_active":true,"created_by":"fixture-operator"})).unwrap()
}
fn claim(id: &str) -> Claim {
    serde_json::from_value(json!({"network_id":NETWORK_ID,"claim_id":id,"address":ADDRESS,"entity_id":"synthetic-entity","entity_name":"Synthetic fixture, not a real attribution","entity_type":"exchange","address_role":"deposit","confidence":0.95,"risk_level":100,"is_exposure_seed":true,"seed_category":"fixture_only","source_id":"fixture-source","source_reference":"case:fixture","evidence_refs":["case:fixture"],"submitted_by":"fixture-operator"})).unwrap()
}
fn review(id: &str, revision: &Value, decision: &str) -> Review {
    serde_json::from_value(json!({"network_id":NETWORK_ID,"claim_id":id,"expected_revision":revision,"decision":decision,"reviewed_by":"fixture-reviewer","reason":"Synthetic acceptance review","evidence_refs":["case:fixture-review"]})).unwrap()
}
#[tokio::test]
#[ignore = "requires a fresh disposable ClickHouse 23.8"]
async fn pending_review_revoke_source_crash_and_investigation() -> Result<()> {
    let db = Database::new(DatabaseConfig {
        url: std::env::var("BITCOIN_TEST_CLICKHOUSE_URL")?.parse()?,
        user: "bitcoin_test".into(),
        password: "bitcoin-test-only".into(),
        database: "bitcoin_intelligence_test".into(),
    })?;
    db.initialize().await?;
    assert!(
        intelligence::read(&db, ADDRESS).await?.claims.is_empty(),
        "requires fresh fixture"
    );
    intelligence::register(&db, source(), true).await?;
    assert!(
        intelligence::import(&db, vec![claim("label")], false)
            .await
            .is_err()
    );
    let registered = intelligence::register(&db, source(), false).await?;
    let dry = intelligence::import(&db, vec![claim("label")], true).await?;
    assert_eq!(dry["written"], false);
    assert!(intelligence::read(&db, ADDRESS).await?.claims.is_empty());
    assert!(
        intelligence::import(&db, vec![claim("label"), claim("label")], false)
            .await
            .is_err()
    );
    let submitted = intelligence::import(&db, vec![claim("label")], false).await?;
    let pending = api::investigation(&db, ADDRESS, 40).await?;
    assert!(pending["entities"].as_array().unwrap().is_empty());
    assert_eq!(
        pending["intelligence"]["claims"][0]["review_status"],
        "pending"
    );
    assert!(
        intelligence::review(&db, review("label", &json!("1"), "approved"), false)
            .await
            .is_err()
    );
    intelligence::review(
        &db,
        review("label", &submitted["claims"][0]["revision"], "approved"),
        true,
    )
    .await?;
    assert!(intelligence::read(&db, ADDRESS).await?.entities.is_empty());
    let approved_receipt = intelligence::review(
        &db,
        review("label", &submitted["claims"][0]["revision"], "approved"),
        false,
    )
    .await?;
    let approved = api::investigation(&db, ADDRESS, 40).await?;
    assert_eq!(approved["entities"].as_array().unwrap().len(), 1);
    assert_eq!(approved["entities"][0]["is_exposure_seed"], true);
    assert_eq!(approved["entities"][0]["active"], true);
    assert_ne!(
        pending["intelligence_snapshot"],
        approved["intelligence_snapshot"]
    );
    assert_eq!(
        intelligence::read(&db, ADDRESS).await?.snapshot,
        approved["intelligence_snapshot"]
    );
    let mut revised = claim("label");
    revised.expected_revision = Some(approved_receipt["revision"].as_str().unwrap().into());
    let updated = intelligence::import(&db, vec![revised], false).await?;
    assert!(
        intelligence::read(&db, ADDRESS).await?.entities.is_empty(),
        "editing an approval must require a new review"
    );
    let again = intelligence::review(
        &db,
        review("label", &updated["claims"][0]["revision"], "approved"),
        false,
    )
    .await?;
    let mut disabled = source();
    disabled.expected_revision = Some(registered["revision"].as_str().unwrap().into());
    disabled.is_active = false;
    let disabled_receipt = intelligence::register(&db, disabled, false).await?;
    let inactive = api::investigation(&db, ADDRESS, 40).await?;
    assert!(inactive["entities"].as_array().unwrap().is_empty());
    assert!(
        intelligence::review(&db, review("label", &again["revision"], "approved"), false)
            .await
            .is_err()
    );
    let mut enabled = source();
    enabled.expected_revision = Some(disabled_receipt["revision"].as_str().unwrap().into());
    intelligence::register(&db, enabled, false).await?;
    assert_eq!(intelligence::read(&db, ADDRESS).await?.entities.len(), 1);
    // Simulate process failure after append-only revocation evidence but before
    // publishing its claim row. The old approved revision must already disappear.
    let crash_revision = again["revision"].as_str().unwrap().parse::<u64>()? + 100;
    db.execute(&format!("INSERT INTO aml_intelligence_reviews (network_id,review_id,subject_type,subject_id,decision,reviewed_by,reason,evidence_refs,revision,created_at_unix_ms,subject_revision) VALUES ('{NETWORK_ID}','crashed-review','intelligence_claim','label','revoked','fixture-reviewer','simulated crash',['case:fixture'],{crash_revision},1,{crash_revision})")).await?;
    assert!(
        intelligence::read(&db, ADDRESS).await?.entities.is_empty(),
        "a partially published review must fail closed"
    );
    let recovered =
        intelligence::review(&db, review("label", &again["revision"], "approved"), false).await?;
    assert!(recovered["revision"].as_str().unwrap().parse::<u64>()? > crash_revision);
    assert_eq!(intelligence::read(&db, ADDRESS).await?.entities.len(), 1);
    intelligence::review(
        &db,
        review("label", &recovered["revision"], "revoked"),
        false,
    )
    .await?;
    let revoked = api::investigation(&db, ADDRESS, 40).await?;
    assert!(revoked["entities"].as_array().unwrap().is_empty());
    db.execute("OPTIMIZE TABLE aml_intelligence_claims FINAL")
        .await?;
    db.execute("OPTIMIZE TABLE aml_intelligence_reviews FINAL")
        .await?;
    let history = intelligence::history(&db, "label").await?;
    assert!(history["reviews"].as_array().unwrap().len() >= 5);
    assert_eq!(history["current_claim"]["review_status"], "revoked");
    assert!(history["reviews"].as_array().unwrap().iter().any(|r| {
        serde_json::from_str::<Value>(r["evidence_json"].as_str().unwrap()).unwrap()["claim_after"]["review_status"] == "approved"
    }),"approval evidence must survive physical MergeTree compaction");
    let mut expired = claim("expired");
    expired.valid_from_unix_ms = Some(1);
    expired.valid_to_unix_ms = Some(2);
    let expired = intelligence::import(&db, vec![expired], false).await?;
    assert!(
        intelligence::review(
            &db,
            review("expired", &expired["claims"][0]["revision"], "approved"),
            false
        )
        .await
        .is_err()
    );
    let future_at = bitcoin_aml::db::timestamp() + 60000;
    let mut future = claim("future");
    future.valid_from_unix_ms = Some(future_at);
    let future = intelligence::import(&db, vec![future], false).await?;
    intelligence::review(
        &db,
        review("future", &future["claims"][0]["revision"], "approved"),
        false,
    )
    .await?;
    assert!(intelligence::read(&db, ADDRESS).await?.entities.is_empty());
    let mut rejected = claim("rejected");
    rejected.risk_level = 0;
    rejected.is_exposure_seed = false;
    let rejected = intelligence::import(&db, vec![rejected], false).await?;
    intelligence::review(
        &db,
        review("rejected", &rejected["claims"][0]["revision"], "rejected"),
        false,
    )
    .await?;
    assert!(intelligence::read(&db, ADDRESS).await?.entities.is_empty());
    let router = api::router(api::Api::new(
        db.clone(),
        "fixture-service-key-0123456789abcdef".into(),
    )?);
    let uri = format!("/api/bitcoin/wallet/{ADDRESS}/intelligence/snapshot");
    let denied = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(&uri)
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(denied.status(), 401);
    let response = router
        .oneshot(
            axum::http::Request::builder()
                .uri(&uri)
                .header("X-AML-Service-Key", "fixture-service-key-0123456789abcdef")
                .body(axum::body::Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["Cache-Control"], "no-store");
    let body: Value =
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 65536).await?)?;
    assert_eq!(
        body["intelligence_snapshot"],
        intelligence::read(&db, ADDRESS).await?.snapshot
    );
    if let Ok(path) = std::env::var("BITCOIN_INTELLIGENCE_ACCEPTANCE") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &json!({"scope":"synthetic ClickHouse acceptance only","pending":pending,"approved":approved,"source_inactive":inactive,"revoked":revoked}),
            )?,
        )?;
    }
    Ok(())
}
