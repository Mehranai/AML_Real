//! Preserve and verify cluster leads without turning them into risk inputs.
use super::*;
pub(super) fn validity(data: &Value, current: &Value) -> &'static str {
    let expected = &data["cluster_snapshot"];
    let actual = &current["cluster_snapshot"];
    let config = &current["cluster_config"];
    let mut unsigned = expected.clone();
    let Some(object) = unsigned.as_object_mut() else {
        return "unverified";
    };
    object.remove("snapshot_id");
    object.remove("current");
    object.remove("source_current");
    let body = json!({"network_id":data["network_id"],"address":data["address"],"run_id":expected["run_id"],"source_snapshot":data["source_snapshot"],"clusters":data["clusters"],"cluster_coverage":data["cluster_coverage"]});
    let scoped = |v: &Value| {
        v["contract"] == "bitcoin_clustering_v1"
            && v["network_id"] == data["network_id"]
            && v["address"] == data["address"]
            && v["run_id"] == expected["run_id"]
    };
    let valid_hash = |v: &Value| {
        v.as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    if !valid_hash(&expected["definition_hash"])
        || !valid_hash(&config["definition_hash"])
        || !scoped(expected)
        || !scoped(actual)
        || config["contract"] != "bitcoin_clustering_config_v1"
        || config["network_id"] != data["network_id"]
        || expected["source_snapshot_id"] != data["source_snapshot"]["snapshot_id"]
        || expected["snapshot_id"] != aml_risk_policy::digest(&unsigned)
        || expected["body_hash"] != aml_risk_policy::digest(&body)
        || expected["clusters_hash"] != aml_risk_policy::digest(&data["clusters"])
        || expected["coverage_hash"] != aml_risk_policy::digest(&data["cluster_coverage"])
        || data["clusters"].as_array().is_none_or(|list| {
            list.iter().any(|c| {
                c["ownership_claimed"] != false
                    || c["score_eligible"] != false
                    || c["is_exposure_seed"] != false
                    || !c["same_entity"].is_null()
            })
        })
    {
        "unverified"
    } else if actual["source_current"] != true
        || (actual["current"] != true
            && !(expected["audit_only"] == true
                && actual["audit_only"] == true
                && data["cluster_coverage"]["persisted"] == false
                && data["cluster_coverage"]["status"] == "partial"))
        || actual["snapshot_id"] != expected["snapshot_id"]
        || config["detector_version"] != expected["detector_version"]
        || config["definition_hash"] != expected["definition_hash"]
    {
        "stale"
    } else {
        "current"
    }
}
pub(super) async fn fetch(app: &App, upstream: &str, data: &Value) -> anyhow::Result<Value> {
    let address = data["address"]
        .as_str()
        .filter(|a| valid_address("bitcoin", a))
        .context("invalid cluster address")?;
    let id = data["cluster_snapshot"]["run_id"]
        .as_str()
        .context("cluster run missing")?;
    ensure!(
        id.strip_prefix("btc-clrun-")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        "invalid cluster run"
    );
    let mut response = app
        .http
        .get(format!(
            "{upstream}/api/bitcoin/wallet/{address}/clusters/{id}/snapshot"
        ))
        .header("x-aml-service-key", &app.key)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "cluster freshness unavailable"
    );
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 65536,
            "cluster freshness response exceeds bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        value["network_id"] == data["network_id"],
        "cluster freshness network mismatch"
    );
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires actual Bitcoin clustering ClickHouse acceptance JSON"]
    fn actual_adapter_cluster_frames_bind_members_and_reviews() {
        let Ok(path) = std::env::var("BITCOIN_CLUSTER_ACCEPTANCE") else {
            return;
        };
        let proof: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for name in ["pending", "approved", "revoked", "change"] {
            let data = &proof[name];
            let current = &proof[format!("{name}_current")];
            assert_eq!(validity(data, current), "current", "{name}");
            assert!(risk::assess("bitcoin", data)["risk_score"].is_null());
            for key in ["members", "review_status", "evidence"] {
                let mut bad = data.clone();
                bad["clusters"][0][key] = Value::Null;
                assert_eq!(validity(&bad, current), "unverified", "{key}");
            }
            let mut changed = current.clone();
            changed["cluster_snapshot"]["current"] = json!(false);
            assert_eq!(validity(data, &changed), "stale");
            let mut changed = current.clone();
            changed["cluster_config"]["definition_hash"] = json!("a".repeat(64));
            assert_eq!(validity(data, &changed), "stale");
            assert_eq!(validity(data, &Value::Null), "unverified");
        }
        assert_eq!(
            validity(&proof["partial"], &proof["partial_current"]),
            "current",
            "an explicit audit-only scope must not invalidate independent risk evidence"
        );
    }
}
