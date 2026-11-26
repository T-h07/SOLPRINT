//! Cross-cutting HTTP behaviour.
//!
//! `observe` is the outermost layer. For every request it:
//!
//! 1. assigns a request id (a well-formed client `x-request-id` is reused,
//!    anything else is replaced with a fresh UUIDv7);
//! 2. opens a tracing span carrying method, matched route and request id;
//! 3. rewrites any non-JSON error response into the standard
//!    [`ApiError`](crate::contracts::ApiError) envelope;
//! 4. adds security headers and echoes the request id;
//! 5. logs one access line: status and latency (monotonic), never headers,
//!    query strings or bodies.

use axum::body::Body;
use axum::extract::{MatchedPath, Request};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use solprint_observability::REQUEST_ID_HEADER;
use tokio::time::Instant;
use tracing::Instrument;

use crate::contracts::{ApiError, ApiErrorBody, ApiErrorCode};

/// Response extension marking a deliberate structured non-2xx body (e.g. a
/// 503 readiness report) that must not be replaced by the error envelope.
#[derive(Debug, Clone, Copy)]
pub struct StructuredBody;

fn valid_client_request_id(value: &HeaderValue) -> Option<String> {
    let s = value.to_str().ok()?;
    let ok = (8..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| s.to_owned())
}

pub async fn observe(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(valid_client_request_id)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "<unmatched>".to_owned(), |p| p.as_str().to_owned());

    let span = tracing::info_span!(
        "http",
        request_id = %request_id,
        method = %method,
        route = %route
    );

    async move {
        let response = next.run(request).await;
        let response = envelope_errors(response, &request_id);
        let mut response = response;
        apply_headers(response.headers_mut(), &request_id);

        let status = response.status().as_u16();
        let latency_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        if response.status().is_server_error() {
            tracing::warn!(status, latency_us, "request completed with server error");
        } else if method == Method::GET && route.starts_with("/health") {
            // Health probes run every few seconds; keep them out of INFO.
            tracing::debug!(status, latency_us, "request completed");
        } else {
            tracing::info!(status, latency_us, "request completed");
        }
        response
    }
    .instrument(span)
    .await
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}

fn envelope_errors(response: Response, request_id: &str) -> Response {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;
    }
    if response.extensions().get::<StructuredBody>().is_some() || is_json(response.headers()) {
        return response;
    }
    // Replace whatever body the inner layer produced (plain text from axum
    // rejections, empty bodies from tower-http) with the sanitized envelope.
    let allow = response.headers().get(header::ALLOW).cloned();
    let mut rebuilt = json_error(status, Some(request_id));
    if let Some(allow) = allow {
        rebuilt.headers_mut().insert(header::ALLOW, allow);
    }
    rebuilt
}

/// Build a standard error response for `status`.
#[must_use]
pub fn json_error(status: StatusCode, request_id: Option<&str>) -> Response {
    let (code, message) = match status {
        StatusCode::NOT_FOUND => (ApiErrorCode::NotFound, "Resource not found."),
        StatusCode::METHOD_NOT_ALLOWED => (
            ApiErrorCode::MethodNotAllowed,
            "Method not allowed for this resource.",
        ),
        StatusCode::PAYLOAD_TOO_LARGE => (ApiErrorCode::PayloadTooLarge, "Request body too large."),
        StatusCode::REQUEST_TIMEOUT => (ApiErrorCode::RequestTimeout, "Request timed out."),
        StatusCode::SERVICE_UNAVAILABLE => (
            ApiErrorCode::ServiceUnavailable,
            "Service temporarily unavailable.",
        ),
        s if s.is_client_error() => (ApiErrorCode::BadRequest, "Invalid request."),
        _ => (ApiErrorCode::InternalError, "Internal server error."),
    };
    let body = ApiError {
        error: ApiErrorBody {
            code,
            message: message.to_owned(),
            request_id: request_id.map(str::to_owned),
        },
    };
    (status, axum::Json(body)).into_response()
}

fn apply_headers(headers: &mut HeaderMap, request_id: &str) {
    const STATIC: [(&str, &str); 6] = [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
        ("cross-origin-resource-policy", "same-origin"),
    ];
    for (name, value) in STATIC {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    if let Ok(v) = HeaderValue::from_str(request_id) {
        headers.insert(HeaderName::from_static(REQUEST_ID_HEADER), v);
    }
}

pub async fn not_found() -> Response {
    // The envelope (with request id) is applied by `observe`.
    (StatusCode::NOT_FOUND, Body::empty()).into_response()
}
