use axum::response::Json;
use serde_json::json;

pub async fn status() -> Json<serde_json::Value> {
    Json(json!({"service":"tron-aml"}))
}
