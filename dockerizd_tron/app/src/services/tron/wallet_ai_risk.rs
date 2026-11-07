//! Local ML is disabled; the main VM applies the shared explainable policy.
use super::{wallet_exposure::WalletExposureSummary, wallet_fingerprint::WalletFingerprint};
use serde_json::{Value, json};
pub type WalletAiRiskAssessment = Value;
pub fn build_disabled_wallet_ai_risk(
    _fingerprint: &WalletFingerprint,
    _exposure: WalletExposureSummary,
) -> Value {
    json!({"status":"DISABLED","risk_level":"UNKNOWN","risk_score":null,"risk_percent":null,"message":"Risk is evaluated by the main VM shared policy when creating an investigation.","feature_importance":[],"model_patterns":[],"evidence_refs":[],"model_available":false})
}
