//! HTTP request/error contract used by the four independent chain adapters.
//! This library owns no database, service, chain facts or balance semantics.
mod analysis;
pub mod bridge;
pub mod kyt;
pub mod marketplace;
pub mod response;
mod seaport;
mod semantic_evidence;
pub use aml_warehouse::Network;
use axum::{
    Json,
    extract::{FromRequestParts, Path, Query, Request},
    http::{StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        let code = match status {
            StatusCode::BAD_REQUEST => "invalid_request",
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::FORBIDDEN => "forbidden",
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
            StatusCode::CONFLICT => "conflict",
            StatusCode::UNPROCESSABLE_ENTITY => "scope_limit",
            StatusCode::TOO_MANY_REQUESTS => "capacity_exceeded",
            StatusCode::SERVICE_UNAVAILABLE => "evidence_unavailable",
            StatusCode::GATEWAY_TIMEOUT => "timeout",
            StatusCode::INTERNAL_SERVER_ERROR => "internal_error",
            _ => "request_failed",
        };
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // Keep the existing string `error` field for gateway/UI compatibility.
        let mut response = (
            self.status,
            Json(json!({
                "api_version": "1.0", "error": self.message,
                "code": self.code, "status": self.status.as_u16()
            })),
        )
            .into_response();
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        response
    }
}

pub fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    ApiError::new(status, message).into_response()
}

pub fn transfer_error(error: aml_warehouse::relationships::PageError) -> ApiError {
    use aml_warehouse::relationships::PageError;
    match error {
        PageError::Invalid(error) => ApiError::new(StatusCode::BAD_REQUEST, error.to_string()),
        PageError::SourceChanged => ApiError {
            status: StatusCode::CONFLICT,
            code: "snapshot_changed",
            message: "source snapshot changed; restart pagination without a cursor".into(),
        },
        PageError::Unavailable(error) => {
            eprintln!("Transfer page evidence unavailable: {error:#}");
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "transfer evidence unavailable; check service logs",
            )
        }
    }
}

/// Wrap extractor rejections as JSON without reflecting request values or secrets.
pub struct ApiQuery<T>(pub T);
impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_query",
                message: "invalid, duplicate or unsupported query parameter".into(),
            })
    }
}

pub struct ApiPath<T>(pub T);
impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|_| ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_path",
                message: "invalid path parameter".into(),
            })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyQuery {}

/// Validate before IO. Chain-specific limits stay explicit at the caller.
pub fn bounded<T: Copy + PartialOrd + std::fmt::Display>(
    name: &str,
    value: Option<T>,
    min: T,
    max: T,
) -> Result<(), ApiError> {
    if value.is_some_and(|v| v < min || v > max) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("{name} must be {min}..{max}"),
        ));
    }
    Ok(())
}

/// Apply outside auth middleware so successful and rejected requests cannot cache.
pub async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
