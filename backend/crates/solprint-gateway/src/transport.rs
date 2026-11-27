//! Centralized HTTP transport.
//!
//! * One `reqwest` client (rustls + ring, bundled Mozilla roots, HTTP/1.1).
//! * Redirects are never followed (a redirect could leak the Helius key,
//!   which travels in the query string, to another host).
//! * Every destination must be on the allowlist built from configuration;
//!   execution and submission endpoints are blocklisted by path and host.
//! * Response bodies are read incrementally and cut off at the limit.
//! * Errors never carry the URL: `reqwest` errors are classified and
//!   discarded, so a key in a query string cannot reach a log line.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use solprint_config::Secret;
use tokio::time::Instant;
use url::Url;

use crate::context::ProviderId;
use crate::error::GatewayError;

/// Paths GATEWAY refuses to call on any host (execution / submission).
pub const BLOCKED_PATH_FRAGMENTS: &[&str] = &[
    "/swap/v2/execute",
    "/swap/v2/build",
    "/swap/v1/swap",
    "/tx/v1/submit",
    "/ultra/v1/execute",
    "/execute",
    "/submit",
];

/// Hosts GATEWAY refuses to call even if configured.
pub const BLOCKED_HOSTS: &[&str] = &["tx.jup.ag"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// One outgoing request. `Debug` never prints the URL or headers.
pub struct HttpRequest {
    pub provider: ProviderId,
    pub method: Method,
    pub url: Secret<Url>,
    pub headers: Vec<(&'static str, Secret<String>)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest")
            .field("provider", &self.provider)
            .field("method", &self.method)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub latency: Duration,
    /// Seconds from `Retry-After` (delta form only).
    pub retry_after: Option<Duration>,
    /// `x-ratelimit-reset` (unix seconds), Jupiter.
    pub ratelimit_reset_unix: Option<i64>,
    pub ratelimit_remaining: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TransportSettings {
    pub connect_timeout: Duration,
    pub max_response_bytes: usize,
    /// Origins (`scheme://host:port`) that may be contacted.
    pub allowed_origins: Vec<Url>,
    /// Tests only: permit `http://127.0.0.1` / `ws://127.0.0.1` mocks.
    pub allow_insecure_loopback: bool,
}

#[derive(Debug, Clone)]
pub struct HttpTransport {
    client: reqwest::Client,
    allowed: Arc<HashSet<String>>,
    max_body: usize,
    allow_insecure_loopback: bool,
}

/// rustls client configuration shared by HTTP and WebSocket.
pub fn tls_config(alpn_http1: bool) -> Result<rustls::ClientConfig, GatewayError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| GatewayError::Stream("TLS configuration failed"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    if alpn_http1 {
        cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    }
    Ok(cfg)
}

fn origin_key(url: &Url) -> String {
    url.origin().ascii_serialization()
}

impl HttpTransport {
    pub fn new(settings: &TransportSettings) -> Result<Self, GatewayError> {
        let tls = tls_config(true)?;
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(settings.connect_timeout)
            .pool_max_idle_per_host(4)
            .pool_idle_timeout(Duration::from_secs(60))
            .http1_only()
            .https_only(!settings.allow_insecure_loopback)
            .no_proxy()
            .user_agent(concat!("solprint-gateway/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| GatewayError::Stream("HTTP client construction failed"))?;
        let allowed = settings.allowed_origins.iter().map(origin_key).collect();
        Ok(Self {
            client,
            allowed: Arc::new(allowed),
            max_body: settings.max_response_bytes,
            allow_insecure_loopback: settings.allow_insecure_loopback,
        })
    }

    #[must_use]
    pub const fn max_body(&self) -> usize {
        self.max_body
    }

    /// Validate a destination against scheme, allowlist and blocklist.
    pub fn check_destination(&self, url: &Url) -> Result<(), GatewayError> {
        check_destination(url, &self.allowed, self.allow_insecure_loopback)
    }

    pub async fn send(&self, req: HttpRequest) -> Result<HttpResponse, GatewayError> {
        let provider = req.provider;
        let url = req.url.expose();
        self.check_destination(url)?;
        let mut builder = match req.method {
            Method::Get => self.client.get(url.clone()),
            Method::Post => self.client.post(url.clone()),
        };
        builder = builder
            .timeout(req.timeout)
            .header("accept", "application/json");
        for (name, value) in &req.headers {
            builder = builder.header(*name, value.expose().as_str());
        }
        if let Some(body) = req.body {
            builder = builder
                .header("content-type", "application/json")
                .body(body);
        }
        let started = Instant::now();
        let deadline = started + req.timeout;
        let mut resp = builder.send().await.map_err(|e| classify(provider, &e))?;
        let status = resp.status().as_u16();
        let headers = resp.headers();
        let header_num = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<i64>().ok())
        };
        let retry_after = header_num("retry-after")
            .and_then(|s| u64::try_from(s).ok())
            .map(Duration::from_secs);
        let ratelimit_reset_unix = header_num("x-ratelimit-reset");
        let ratelimit_remaining =
            header_num("x-ratelimit-remaining").and_then(|v| u64::try_from(v).ok());
        if let Some(len) = resp.content_length()
            && usize::try_from(len).map_or(true, |l| l > self.max_body)
        {
            return Err(GatewayError::PayloadTooLarge {
                provider,
                limit: self.max_body,
            });
        }
        let mut body = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let chunk = tokio::time::timeout(remaining, resp.chunk())
                .await
                .map_err(|_| GatewayError::Timeout { provider })?
                .map_err(|e| classify(provider, &e))?;
            let Some(chunk) = chunk else { break };
            if body.len().saturating_add(chunk.len()) > self.max_body {
                return Err(GatewayError::PayloadTooLarge {
                    provider,
                    limit: self.max_body,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse {
            status,
            body,
            latency: started.elapsed(),
            retry_after,
            ratelimit_reset_unix,
            ratelimit_remaining,
        })
    }
}

pub(crate) fn check_destination(
    url: &Url,
    allowed: &HashSet<String>,
    allow_insecure_loopback: bool,
) -> Result<(), GatewayError> {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let secure = matches!(url.scheme(), "https" | "wss");
    let loopback = matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]");
    if !secure && !(allow_insecure_loopback && loopback) {
        return Err(GatewayError::HostNotAllowed);
    }
    if BLOCKED_HOSTS.contains(&host.as_str()) {
        return Err(GatewayError::Prohibited("execution host is blocklisted"));
    }
    let path = url.path().to_ascii_lowercase();
    if BLOCKED_PATH_FRAGMENTS.iter().any(|b| path.contains(b)) {
        return Err(GatewayError::Prohibited(
            "execution endpoint is blocklisted",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(GatewayError::HostNotAllowed);
    }
    if allowed.contains(&origin_key(url)) {
        Ok(())
    } else {
        Err(GatewayError::HostNotAllowed)
    }
}

/// Classify a `reqwest` error without ever formatting it (its `Display`
/// includes the URL).
fn classify(provider: ProviderId, e: &reqwest::Error) -> GatewayError {
    if e.is_timeout() {
        GatewayError::Timeout { provider }
    } else if e.is_redirect() {
        GatewayError::Http {
            provider,
            status: 310,
        }
    } else if e.is_decode() || e.is_body() {
        GatewayError::Decode {
            provider,
            detail: "response body could not be read",
        }
    } else {
        GatewayError::Connect { provider }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn allowed(urls: &[&str]) -> HashSet<String> {
        urls.iter()
            .map(|u| origin_key(&Url::parse(u).unwrap()))
            .collect()
    }

    #[test]
    fn destination_policy() {
        let a = allowed(&[
            "https://api.jup.ag",
            "https://mainnet.helius-rpc.com",
            "http://127.0.0.1:9999",
        ]);
        let ok = |u: &str, insecure| check_destination(&Url::parse(u).unwrap(), &a, insecure);
        assert!(ok("https://api.jup.ag/price/v3?ids=x", false).is_ok());
        assert!(ok("https://mainnet.helius-rpc.com/?api-key=k", false).is_ok());
        assert_eq!(
            ok("https://evil.example/price", false),
            Err(GatewayError::HostNotAllowed)
        );
        assert!(matches!(
            ok("https://api.jup.ag/swap/v2/execute", false),
            Err(GatewayError::Prohibited(_))
        ));
        assert!(matches!(
            ok("https://api.jup.ag/SWAP/V2/BUILD", false),
            Err(GatewayError::Prohibited(_))
        ));
        assert!(matches!(
            ok("https://tx.jup.ag/", false),
            Err(GatewayError::Prohibited(_))
        ));
        assert_eq!(
            ok("http://api.jup.ag/price/v3", false),
            Err(GatewayError::HostNotAllowed)
        );
        assert_eq!(
            ok("http://127.0.0.1:9999/x", false),
            Err(GatewayError::HostNotAllowed)
        );
        assert!(ok("http://127.0.0.1:9999/x", true).is_ok());
        assert_eq!(
            ok("https://user:pw@api.jup.ag/price/v3", false),
            Err(GatewayError::HostNotAllowed)
        );
    }

    #[test]
    fn request_debug_never_shows_url_or_headers() {
        let req = HttpRequest {
            provider: ProviderId::Helius,
            method: Method::Post,
            url: Secret::new(
                Url::parse("https://mainnet.helius-rpc.com/?api-key=SYNTHETIC").unwrap(),
            ),
            headers: vec![("x-api-key", Secret::new("SYNTHETIC2".to_owned()))],
            body: None,
            timeout: Duration::from_secs(1),
        };
        let dbg = format!("{req:?}");
        assert!(!dbg.contains("SYNTHETIC"));
        assert!(!dbg.contains("helius-rpc"));
    }
}
