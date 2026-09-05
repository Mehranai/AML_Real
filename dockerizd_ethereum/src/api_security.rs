use std::{env, sync::Arc};

use anyhow::{bail, ensure};
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

const SERVICE_KEY_HEADER: &str = "x-aml-service-key";
const MINIMUM_SERVICE_KEY_LENGTH: usize = 32;

#[derive(Clone, Debug)]
pub struct ServiceAuth {
    required: bool,
    expected_key: Arc<str>,
}

impl ServiceAuth {
    pub fn from_env() -> anyhow::Result<Self> {
        let required = match env::var("AML_SERVICE_AUTH_REQUIRED") {
            Ok(value) => parse_bool("AML_SERVICE_AUTH_REQUIRED", &value)?,
            Err(env::VarError::NotPresent) => false,
            Err(error) => bail!("failed to read AML_SERVICE_AUTH_REQUIRED: {error}"),
        };
        let expected_key = env::var("AML_SERVICE_KEY")
            .unwrap_or_default()
            .trim()
            .to_string();

        if required {
            ensure!(
                expected_key.len() >= MINIMUM_SERVICE_KEY_LENGTH,
                "AML_SERVICE_KEY must contain at least {MINIMUM_SERVICE_KEY_LENGTH} characters when service authentication is required"
            );
        }

        Ok(Self {
            required,
            expected_key: Arc::from(expected_key),
        })
    }
}

pub async fn require_service_auth(
    State(auth): State<ServiceAuth>,
    request: Request,
    next: Next,
) -> Response {
    if !auth.required {
        return next.run(request).await;
    }

    let supplied_key = request
        .headers()
        .get(SERVICE_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if !constant_time_eq(supplied_key.as_bytes(), auth.expected_key.as_bytes()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized service request"})),
        )
            .into_response();
    }

    next.run(request).await
}

fn parse_bool(key: &str, value: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{key} must be true or false"),
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, parse_bool};

    #[test]
    fn parses_supported_boolean_values() {
        assert!(parse_bool("TEST", "yes").unwrap());
        assert!(!parse_bool("TEST", "off").unwrap());
        assert!(parse_bool("TEST", "sometimes").is_err());
    }

    #[test]
    fn compares_service_keys_without_early_content_exit() {
        assert!(constant_time_eq(b"same-key", b"same-key"));
        assert!(!constant_time_eq(b"same-key", b"diff-key"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }
}
