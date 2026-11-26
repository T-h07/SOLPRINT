//! Typed, sanitized GATEWAY errors.
//!
//! No variant carries a URL, header, request body or credential. Provider
//! messages are truncated and stripped of anything that is not printable
//! ASCII before they are stored, so a hostile or verbose upstream cannot
//! inject secrets, control characters or unbounded text into logs.

use std::time::Duration;

use crate::budget::BudgetState;
use crate::context::{Priority, ProviderId};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    #[error("{provider} is disabled: {reason}")]
    Disabled {
        provider: ProviderId,
        reason: &'static str,
    },
    #[error("{0} has no credential configured")]
    MissingCredential(ProviderId),
    #[error("invalid request: {0}")]
    InvalidRequest(&'static str),
    #[error("prohibited operation: {0}")]
    Prohibited(&'static str),
    #[error("request cancelled")]
    Cancelled,
    #[error("deadline exceeded")]
    DeadlineExceeded,
    #[error("{provider} queue is full")]
    QueueFull { provider: ProviderId },
    #[error("{provider} rate limited")]
    RateLimited {
        provider: ProviderId,
        retry_after: Option<Duration>,
    },
    #[error("budget {} denies {} requests", .state.as_str(), .priority.as_str())]
    BudgetDenied {
        state: BudgetState,
        priority: Priority,
    },
    #[error("{provider} circuit open")]
    CircuitOpen {
        provider: ProviderId,
        retry_in: Duration,
    },
    #[error("{provider} timed out")]
    Timeout { provider: ProviderId },
    #[error("{provider} connection failed")]
    Connect { provider: ProviderId },
    #[error("{provider} returned HTTP {status}")]
    Http { provider: ProviderId, status: u16 },
    #[error("{provider} JSON-RPC error {code}: {message}")]
    Rpc {
        provider: ProviderId,
        code: i64,
        message: String,
    },
    #[error("{provider} response could not be decoded: {detail}")]
    Decode {
        provider: ProviderId,
        detail: &'static str,
    },
    #[error("{provider} response exceeded {limit} bytes")]
    PayloadTooLarge { provider: ProviderId, limit: usize },
    #[error("no route available")]
    NoRoute,
    #[error("transaction version is not supported")]
    UnsupportedTransactionVersion,
    #[error("no cached value and the request does not allow a provider call")]
    Unavailable,
    #[error("destination is not on the allowlist")]
    HostNotAllowed,
    #[error("stream error: {0}")]
    Stream(&'static str),
}

impl GatewayError {
    /// Whether retrying the same request can succeed (transient failures).
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::Timeout { .. } | Self::Connect { .. } | Self::RateLimited { .. } => true,
            Self::Http { status, .. } => matches!(*status, 408 | 429 | 500..=599),
            Self::Rpc { code, .. } => is_transient_rpc_code(*code),
            _ => false,
        }
    }

    /// Whether this outcome counts against the provider's circuit breaker.
    /// Client errors, local refusals and "not found" do not.
    #[must_use]
    pub const fn is_provider_failure(&self) -> bool {
        match self {
            Self::Timeout { .. }
            | Self::Connect { .. }
            | Self::PayloadTooLarge { .. }
            | Self::Decode { .. } => true,
            Self::Http { status, .. } => *status >= 500,
            Self::Rpc { code, .. } => is_transient_rpc_code(*code),
            _ => false,
        }
    }

    /// Stable, bounded label for metrics and the monitoring API.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Disabled { .. } => "DISABLED",
            Self::MissingCredential(_) => "MISSING_CREDENTIAL",
            Self::InvalidRequest(_) => "INVALID_REQUEST",
            Self::Prohibited(_) => "PROHIBITED",
            Self::Cancelled => "CANCELLED",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Self::QueueFull { .. } => "QUEUE_FULL",
            Self::RateLimited { .. } => "RATE_LIMITED",
            Self::BudgetDenied { .. } => "BUDGET_DENIED",
            Self::CircuitOpen { .. } => "CIRCUIT_OPEN",
            Self::Timeout { .. } => "TIMEOUT",
            Self::Connect { .. } => "CONNECT",
            Self::Http { .. } => "HTTP",
            Self::Rpc { .. } => "RPC",
            Self::Decode { .. } => "DECODE",
            Self::PayloadTooLarge { .. } => "PAYLOAD_TOO_LARGE",
            Self::NoRoute => "NO_ROUTE",
            Self::UnsupportedTransactionVersion => "UNSUPPORTED_TX_VERSION",
            Self::Unavailable => "UNAVAILABLE",
            Self::HostNotAllowed => "HOST_NOT_ALLOWED",
            Self::Stream(_) => "STREAM",
        }
    }

    /// Whether the request reached (or attempted to reach) the provider, so
    /// that the call is billable.
    #[must_use]
    pub const fn was_dispatched(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. }
                | Self::Http { .. }
                | Self::Rpc { .. }
                | Self::Decode { .. }
                | Self::PayloadTooLarge { .. }
                | Self::RateLimited { .. }
                | Self::NoRoute
                | Self::UnsupportedTransactionVersion
        )
    }
}

/// JSON-RPC codes that indicate a transient server-side condition.
/// `-32005` is Helius "Too many requests" and Solana "node unhealthy";
/// `-32004` block not available; `-32603` internal error; `-32014` block
/// status not yet available.
const fn is_transient_rpc_code(code: i64) -> bool {
    matches!(code, -32005 | -32004 | -32603 | -32014)
}

/// Make an upstream message safe to store and display: printable ASCII only,
/// whitespace collapsed, at most 160 characters. Anything that looks like a
/// URL is dropped entirely, because RPC nodes sometimes echo endpoints.
#[must_use]
pub fn sanitize_message(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(160));
    for word in raw.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        if lower.contains("://") || lower.contains("api-key") || lower.contains("apikey") {
            continue;
        }
        let clean: String = word
            .chars()
            .filter(|c| c.is_ascii_graphic())
            .take(160)
            .collect();
        if clean.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&clean);
        if out.len() >= 160 {
            out.truncate(160);
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_drops_urls_keys_and_control_characters() {
        let raw = "failed\u{0007} at https://mainnet.helius-rpc.com/?api-key=SECRET  api-key=SECRET2 \n retry";
        let clean = sanitize_message(raw);
        assert_eq!(clean, "failed at retry");
        assert!(!clean.contains("SECRET"));
        assert!(sanitize_message(&"x".repeat(1_000)).len() <= 160);
    }

    #[test]
    fn retry_classification_follows_provider_docs() {
        let p = ProviderId::Helius;
        for status in [408, 429, 500, 502, 503] {
            assert!(
                GatewayError::Http {
                    provider: p,
                    status
                }
                .is_retryable()
            );
        }
        for status in [400, 401, 403, 404, 409, 422] {
            assert!(
                !GatewayError::Http {
                    provider: p,
                    status
                }
                .is_retryable()
            );
        }
        assert!(!GatewayError::NoRoute.is_retryable());
        assert!(
            !GatewayError::Http {
                provider: p,
                status: 404
            }
            .is_provider_failure()
        );
    }
}
