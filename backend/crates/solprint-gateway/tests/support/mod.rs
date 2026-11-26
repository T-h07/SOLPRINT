//! Loopback mock providers for GATEWAY tests. Nothing here contacts the
//! network beyond 127.0.0.1.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use solprint_config::Secret;
use solprint_gateway::GatewaySettings;
use solprint_gateway::budget::BudgetPolicy;
use solprint_gateway::settings::{
    DexScreenerSettings, FallbackSettings, HeliusSettings, JupiterSettings,
};
use url::Url;

/// One recorded request.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Scripted reply.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    pub delay: Duration,
    pub headers: Vec<(&'static str, String)>,
}

impl Reply {
    pub fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            delay: Duration::ZERO,
            headers: Vec::new(),
        }
    }
    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            ..Self::ok(body)
        }
    }
    pub fn delayed(mut self, d: Duration) -> Self {
        self.delay = d;
        self
    }
    pub fn header(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.headers.push((k, v.into()));
        self
    }
}

type Script = Arc<dyn Fn(&Recorded) -> Reply + Send + Sync>;

#[derive(Clone)]
struct MockState {
    script: Script,
    log: Arc<Mutex<Vec<Recorded>>>,
    hits: Arc<AtomicUsize>,
}

/// A loopback HTTP server answering every request from a script.
pub struct MockServer {
    pub url: Url,
    log: Arc<Mutex<Vec<Recorded>>>,
    hits: Arc<AtomicUsize>,
}

impl MockServer {
    pub async fn start(script: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        let state = MockState {
            script: Arc::new(script),
            log: Arc::default(),
            hits: Arc::default(),
        };
        let log = Arc::clone(&state.log);
        let hits = Arc::clone(&state.hits);
        let app = Router::new().fallback(handle).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            url: Url::parse(&format!("http://{addr}/")).unwrap(),
            log,
            hits,
        }
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.log.lock().unwrap().clone()
    }
}

async fn handle(
    State(s): State<MockState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let rec = Recorded {
        method: method.to_string(),
        path: uri.path().to_owned(),
        query: uri.query().unwrap_or_default().to_owned(),
        headers,
        body: body.to_vec(),
    };
    s.hits.fetch_add(1, Ordering::SeqCst);
    let reply = (s.script)(&rec);
    s.log.lock().unwrap().push(rec);
    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }
    let mut resp = (
        StatusCode::from_u16(reply.status).unwrap(),
        [("content-type", "application/json")],
        reply.body,
    )
        .into_response();
    for (k, v) in reply.headers {
        resp.headers_mut().insert(k, v.parse().unwrap());
    }
    resp
}

/// JSON-RPC reply helpers (echo the request id).
pub fn rpc_result(req: &Recorded, result: &Value) -> Reply {
    let id = req.json()["id"].clone();
    Reply::ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
}

pub fn rpc_error(req: &Recorded, code: i64, message: &str) -> Reply {
    let id = req.json()["id"].clone();
    Reply::ok(
        serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            .to_string(),
    )
}

pub fn rpc_method(req: &Recorded) -> String {
    req.json()["method"].as_str().unwrap_or_default().to_owned()
}

/// A URL nothing listens on (port 9 on loopback is "discard", normally closed).
pub fn dead_url() -> Url {
    Url::parse("http://127.0.0.1:9/").unwrap()
}

pub const TEST_KEY: &str = "test-key-not-a-real-credential";

/// Settings pointing every provider at `dead_url()`; tests override.
pub fn settings() -> GatewaySettings {
    GatewaySettings {
        enabled: true,
        default_deadline: Duration::from_secs(10),
        connect_timeout: Duration::from_millis(500),
        max_response_bytes: 1024 * 1024,
        global_queue_limit: 512,
        provider_queue_limit: 128,
        cache_max_entries: 1_000,
        flush_interval: Duration::from_secs(30),
        usage_retention: Duration::from_secs(86_400),
        metrics_retention: Duration::from_secs(86_400),
        streams_enabled: false,
        stream_ping_interval: Duration::from_secs(30),
        stream_pong_timeout: Duration::from_secs(10),
        helius: HeliusSettings {
            api_key: Some(Secret::new(TEST_KEY.to_owned())),
            rpc_url: dead_url(),
            ws_url: Url::parse("ws://127.0.0.1:9/").unwrap(),
            budget: BudgetPolicy {
                monthly_credits: 1_000_000,
                reserve_percent: 20,
                cycle_start_day: 1,
            },
            rps: 10,
            max_in_flight: 4,
            ws_max_subscriptions: 100,
            get_block_enabled: false,
        },
        jupiter: JupiterSettings {
            api_key: None,
            base_url: dead_url(),
            rpm: 60,
            max_in_flight: 2,
            quote_enabled: true,
        },
        dexscreener: DexScreenerSettings {
            base_url: dead_url(),
            rpm: 300,
            max_in_flight: 4,
            discovery_enabled: false,
        },
        fallback: FallbackSettings {
            url: Secret::new(dead_url()),
            enabled: false,
            rps: 4,
        },
        allow_insecure_loopback: true,
    }
}

/// Deterministic valid signature (64 bytes of `n`).
pub fn sig(n: u8) -> String {
    bs58::encode([n; 64]).into_string()
}

/// Deterministic valid 32-byte address.
pub fn addr(n: u8) -> String {
    bs58::encode([n; 32]).into_string()
}
