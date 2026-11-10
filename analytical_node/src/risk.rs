pub use aml_risk_policy::{assess, text};
use serde_json::{Value, json};
use std::sync::LazyLock;

static POLICY_HASH: LazyLock<String> =
    LazyLock::new(|| aml_risk_policy::digest(&aml_risk_policy::definition()));

pub fn attach_assessment(data: &mut Value, assessed: Value) {
    data["risk_engine"] = assessed;
    data["risk_policy_hash"] = json!(POLICY_HASH.as_str());
}

pub fn record_source_validity(data: &mut Value, network: &str, status: &str, checked_at: i64) {
    let already_historical = data["historical_risk_engine"].is_object()
        && data["risk_engine"]["risk_score"].is_null()
        && data["risk_engine"]["risk_level"] == "UNKNOWN";
    let mut usable = false;
    if data["risk_engine"].is_object() && (status != "current" || !already_historical) {
        if status != "current" {
            retain_historical_risk(
                data,
                "source_snapshot_invalid",
                "The chain or operational evidence changed or could not be verified. Run a new investigation before using current risk.",
            );
        } else if data["risk_policy_hash"] != POLICY_HASH.as_str()
            || data["risk_engine"] != assess(network, data)
        {
            retain_historical_risk(
                data,
                "policy_unverified",
                "The saved assessment does not match the running policy and its recorded evidence. Run a new investigation before using current risk.",
            );
        } else {
            usable = data["risk_engine"]["risk_score"].is_number();
        }
    }
    data["source_validity"] = json!({"status":status,"checked_at_unix_ms":checked_at,
        "current_risk_usable":usable,"historical_evidence_preserved":true});
}

fn retain_historical_risk(data: &mut Value, status: &str, limitation: &str) {
    if !data["historical_risk_engine"].is_object() {
        data["historical_risk_engine"] = data["risk_engine"].clone();
    }
    data["risk_engine"] = json!({"status":status,"risk_score":null,"risk_level":"UNKNOWN",
        "signals":[],"policy_version":data["historical_risk_engine"]["policy_version"],
        "probability_claimed":false,"limitations":[limitation]});
}
