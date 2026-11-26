//! Shared GET handling for REST providers (Jupiter, DEX Screener).

use std::time::Duration;

use solprint_config::Secret;
use time::OffsetDateTime;
use url::Url;

use crate::context::ProviderId;
use crate::error::GatewayError;
use crate::transport::{HttpRequest, HttpResponse, HttpTransport, Method};

/// Statuses the caller interprets itself (body is returned).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    /// 2xx only.
    Success,
    /// 2xx, plus 400 and 404 (provider-specific meaning).
    SuccessOrClientAnswer,
}

pub async fn get(
    transport: &HttpTransport,
    provider: ProviderId,
    url: Url,
    header: Option<(&'static str, &Secret<String>)>,
    timeout: Duration,
    accept: Accept,
) -> Result<HttpResponse, GatewayError> {
    let headers = header
        .map(|(name, value)| vec![(name, value.clone())])
        .unwrap_or_default();
    let resp = transport
        .send(HttpRequest {
            provider,
            method: Method::Get,
            url: Secret::new(url),
            headers,
            body: None,
            timeout,
        })
        .await?;
    match resp.status {
        200..=299 => Ok(resp),
        429 => Err(GatewayError::RateLimited {
            provider,
            retry_after: retry_after(&resp),
        }),
        400 | 404 if accept == Accept::SuccessOrClientAnswer => Ok(resp),
        status => Err(GatewayError::Http { provider, status }),
    }
}

/// `Retry-After` seconds, else Jupiter's `x-ratelimit-reset` (unix seconds).
#[must_use]
pub fn retry_after(resp: &HttpResponse) -> Option<Duration> {
    resp.retry_after.or_else(|| {
        resp.ratelimit_reset_unix.and_then(|reset| {
            let now = OffsetDateTime::now_utc().unix_timestamp();
            u64::try_from(reset.saturating_sub(now))
                .ok()
                .map(Duration::from_secs)
        })
    })
}

pub fn decode_json<T: serde::de::DeserializeOwned>(
    provider: ProviderId,
    body: &[u8],
    detail: &'static str,
) -> Result<T, GatewayError> {
    serde_json::from_slice(body).map_err(|_| GatewayError::Decode { provider, detail })
}
