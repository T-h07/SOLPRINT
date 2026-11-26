//! HTTP contract tests.
//!
//! Tests without `#[ignore]` run anywhere: they use a lazily connecting pool
//! aimed at an unreachable port, which is exactly the "PostgreSQL is down"
//! condition. Tests marked `#[ignore]` need the disposable test database.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use solprint_api::{AppState, BuildInfo, router};
use solprint_config::Config;
use solprint_storage::Database;
use solprint_storage::test_support::TestDatabase;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const HELIUS_SECRET: &str = "synthetic-helius-key-7f3a9c";
const JUPITER_SECRET: &str = "synthetic-jupiter-key-2b8e1d";
const DB_SECRET: &str = "synthetic-db-password-5c4e";
const RPC_SECRET: &str = "synthetic-rpc-key-9d1f";

fn config(database_url: &str) -> Config {
    let mut env: HashMap<&str, String> = HashMap::new();
    env.insert("DATABASE_URL", database_url.to_owned());
    env.insert("HELIUS_API_KEY", HELIUS_SECRET.to_owned());
    env.insert("JUPITER_API_KEY", JUPITER_SECRET.to_owned());
    env.insert(
        "SOLANA_FALLBACK_RPC_URL",
        format!("https://rpc.example.com/?api-key={RPC_SECRET}"),
    );
    env.insert("HTTP_BODY_LIMIT_BYTES", "1024".to_owned());
    Config::from_lookup(|k| env.get(k).cloned()).unwrap()
}

/// State whose database is unreachable (port 1 refuses connections).
fn offline_state() -> AppState {
    let cfg = config(&format!(
        "postgres://solprint_app:{DB_SECRET}@127.0.0.1:1/solprint"
    ));
    let mut db_cfg = cfg.database.clone();
    db_cfg.acquire_timeout = Duration::from_millis(300);
    let db = Database::connect_lazy(&db_cfg).unwrap();
    AppState::new(&cfg, db, BuildInfo::current("0.1.0-test"))
}

async fn call(state: AppState, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let res = router(state).oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, json)
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn liveness_is_independent_of_database() {
    let (status, _, body) = call(offline_state(), get("/health/live")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ALIVE");
}

#[tokio::test]
async fn readiness_is_503_when_database_is_down() {
    let (status, _, body) = call(offline_state(), get("/health/ready")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "NOT_READY");
    assert_eq!(body["checks"]["database"]["status"], "UNAVAILABLE");
    assert!(body["checks"]["database"]["latency_us"].is_null());
    // Structured readiness report, not the generic error envelope.
    assert!(body.get("error").is_none());
}

#[tokio::test]
async fn system_status_reports_truthful_state() {
    let (status, headers, body) = call(offline_state(), get("/api/v1/system/status")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert_eq!(body["application"], "SOLPRINT");
    assert_eq!(body["environment"], "development");
    assert_eq!(body["trading"]["mode"], "PAPER");
    assert_eq!(body["trading"]["live_trading_enabled"], false);
    assert_eq!(body["trading"]["execution_capability"], "NONE");
    assert_eq!(body["database"]["status"], "UNAVAILABLE");
    assert!(body["database"]["migrations_applied"].is_null());
    assert_eq!(body["database"]["migrations_expected"], 1);
    let providers = body["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 4);
    for p in providers {
        assert_eq!(p["state"], "NOT_INITIALIZED", "{p}");
        assert!(p["last_checked_at"].is_null(), "{p}");
    }
    assert_eq!(providers[0]["id"], "HELIUS");
    assert_eq!(providers[0]["credential"], "CONFIGURED");
    assert_eq!(providers[2]["credential"], "NOT_REQUIRED");
    assert!(body["timestamp"].as_str().unwrap().ends_with('Z'));
    assert!(body["uptime_seconds"].is_u64());
}

#[tokio::test]
async fn version_exposes_only_safe_build_metadata() {
    let (status, _, body) = call(offline_state(), get("/api/v1/system/version")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "solprint");
    assert_eq!(body["version"], "0.1.0-test");
    assert_eq!(body["api_version"], "v1");
    assert!(matches!(
        body["build_profile"].as_str(),
        Some("debug" | "release")
    ));
    let keys: Vec<_> = body.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys.len(), 5, "unexpected fields: {keys:?}");
}

#[tokio::test]
async fn unknown_route_returns_structured_error_with_request_id() {
    let (status, headers, body) = call(offline_state(), get("/api/v1/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    let rid = headers["x-request-id"].to_str().unwrap();
    assert_eq!(body["error"]["request_id"], rid);
}

#[tokio::test]
async fn wrong_method_returns_structured_405() {
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/api/v1/system/status")
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = call(offline_state(), req).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "METHOD_NOT_ALLOWED");
    assert!(headers.contains_key(header::ALLOW));
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let req = Request::get("/health/live")
        .header(header::CONTENT_LENGTH, "4096")
        .body(Body::from(vec![b'x'; 4096]))
        .unwrap();
    let (status, _, body) = call(offline_state(), req).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "PAYLOAD_TOO_LARGE");
}

#[tokio::test]
async fn request_ids_are_echoed_only_when_well_formed() {
    let req = Request::get("/health/live")
        .header("x-request-id", "client-abc-12345")
        .body(Body::empty())
        .unwrap();
    let (_, headers, _) = call(offline_state(), req).await;
    assert_eq!(headers["x-request-id"], "client-abc-12345");

    let req = Request::get("/health/live")
        .header("x-request-id", "bad id\twith <script>")
        .body(Body::empty())
        .unwrap();
    let (_, headers, _) = call(offline_state(), req).await;
    let rid = headers["x-request-id"].to_str().unwrap();
    assert_ne!(rid, "bad id\twith <script>");
    assert_eq!(rid.len(), 36, "expected a generated UUID, got {rid}");
}

#[tokio::test]
async fn security_headers_are_present() {
    let (_, headers, _) = call(offline_state(), get("/api/v1/system/version")).await;
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["x-frame-options"], "DENY");
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["referrer-policy"], "no-referrer");
    assert!(headers.contains_key("content-security-policy"));
    // Same-origin only: no CORS grant.
    assert!(!headers.contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn no_endpoint_leaks_synthetic_secrets() {
    for path in [
        "/health/live",
        "/health/ready",
        "/api/v1/system/status",
        "/api/v1/system/version",
        "/does-not-exist",
    ] {
        let res = router(offline_state()).oneshot(get(path)).await.unwrap();
        let headers = format!("{:?}", res.headers());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let text = format!("{headers}{}", String::from_utf8_lossy(&bytes));
        for secret in [
            HELIUS_SECRET,
            JUPITER_SECRET,
            DB_SECRET,
            RPC_SECRET,
            "solprint_app",
        ] {
            assert!(!text.contains(secret), "{path} leaked {secret}");
        }
    }
}

#[tokio::test]
async fn server_shuts_down_gracefully_on_cancel() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(solprint_api::serve(
        listener,
        offline_state(),
        shutdown.clone(),
    ));

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /health/live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    shutdown.cancel();
    let joined = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("server did not stop within 5s");
    joined.unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn readiness_tracks_real_database_availability() {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    let cfg = config(tdb.config().url.expose().as_str());
    let state = AppState::new(&cfg, db.clone(), BuildInfo::current("0.1.0-test"));

    let (status, _, body) = call(state.clone(), get("/health/ready")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "READY");
    assert!(body["checks"]["database"]["latency_us"].is_u64());

    let (_, _, body) = call(state.clone(), get("/api/v1/system/status")).await;
    assert_eq!(body["database"]["status"], "AVAILABLE");
    assert_eq!(body["database"]["migrations_applied"], 1);

    // Outage: readiness must flip to 503 while liveness stays 200.
    tdb.drop_database().await;
    let (status, _, body) = call(state.clone(), get("/health/ready")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    let (status, _, _) = call(state, get("/health/live")).await;
    assert_eq!(status, StatusCode::OK);
    db.close().await;
}
