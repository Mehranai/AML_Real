use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;
pub async fn tron_wallet_analysis_snapshot() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(
            json!({"error":"Wallet analysis persistence is owned by the main VM.","service":"main_vm","method":"GET","endpoint":"/api/tron/wallet/{address}/investigation","network":"tron","storage":"central_investigation_snapshot"}),
        ),
    )
}
