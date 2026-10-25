//! Stable, redacted error classification. Provider bodies are never echoed.

use crate::types::{ApiError, ErrorKind};
use std::time::Duration;

/// Maps an HTTP status to a stable error kind. No response body is inspected.
pub fn error_for_status(status: u16, retry_after: Option<Duration>) -> ApiError {
    let kind = match status {
        400 => ErrorKind::InvalidRequest,
        401 => ErrorKind::Authentication,
        403 => ErrorKind::PermissionDenied,
        404 => ErrorKind::InvalidRequest,
        408 => ErrorKind::Network,
        409 => ErrorKind::Protocol,
        413 => ErrorKind::LimitExceeded,
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::ProviderUnavailable,
        _ => ErrorKind::ProviderUnavailable,
    };
    let mut error = ApiError::with_status(kind, status, status_detail(status));
    if kind == ErrorKind::RateLimited {
        error.retry_after = retry_after.map(|delay| delay.min(Duration::from_secs(3600)));
    }
    error
}

/// Detects a provider error object returned with an otherwise successful status.
pub fn provider_error_from_body(body: &[u8]) -> Option<ApiError> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object()?;
    let error = object.get("error")?;
    let kind = error
        .as_object()
        .and_then(|object| {
            object
                .get("code")
                .and_then(|value| value.as_str())
                .or_else(|| object.get("type").and_then(|value| value.as_str()))
        })
        .map(classify_provider_code)
        .unwrap_or(ErrorKind::Protocol);
    Some(ApiError::new(kind, "provider returned an error object"))
}

/// Maps a provider error code to a stable kind without keeping the original text.
pub fn classify_provider_code(code: &str) -> ErrorKind {
    let code = code.to_ascii_lowercase();
    if code.contains("invalid_request") || code.contains("invalid_argument") {
        ErrorKind::InvalidRequest
    } else if code.contains("authentication") || code.contains("invalid_api_key") {
        ErrorKind::Authentication
    } else if code.contains("permission") || code.contains("insufficient_quota") {
        ErrorKind::PermissionDenied
    } else if code.contains("rate_limit") || code.contains("too_many_requests") {
        ErrorKind::RateLimited
    } else if code.contains("context_length") || code.contains("limit_exceeded") {
        ErrorKind::LimitExceeded
    } else if code.contains("unsupported") {
        ErrorKind::UnsupportedParameter
    } else if code.contains("overloaded") || code.contains("server_error") {
        ErrorKind::ProviderUnavailable
    } else {
        ErrorKind::Protocol
    }
}

fn status_detail(status: u16) -> &'static str {
    match status {
        400 => "provider rejected the request",
        401 => "authentication failed",
        403 => "permission denied by provider",
        404 => "provider endpoint or model not found",
        408 => "provider request timeout",
        409 => "provider conflict",
        413 => "request too large",
        429 => "provider rate limit reached",
        _ => "provider unavailable",
    }
}
