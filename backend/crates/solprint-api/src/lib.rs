//! SOLPRINT HTTP API.
//!
//! Routes:
//!
//! | Method | Path                     | Purpose                                   |
//! |--------|--------------------------|-------------------------------------------|
//! | GET    | `/health/live`           | process liveness (no dependency checks)   |
//! | GET    | `/health/ready`          | readiness incl. PostgreSQL (200 / 503)    |
//! | GET    | `/api/v1/system/status`  | actual operating state                    |
//! | GET    | `/api/v1/system/version` | version and safe build metadata           |
//! | GET    | `/api/v1/gateway/status`    | GATEWAY overview                       |
//! | GET    | `/api/v1/gateway/providers` | per-provider health, limits, traffic   |
//! | GET    | `/api/v1/gateway/usage`     | process totals and stored hourly usage |
//! | GET    | `/api/v1/gateway/metrics`   | per-operation counts and latency       |
//! | GET    | `/api/v1/gateway/budgets`   | Helius credit budget                   |
//! | GET    | `/api/v1/gateway/streams`   | WebSocket stream state, checkpoints    |
//! | GET    | `/api/v1/gateway/cache`     | response cache statistics              |
//! | GET    | `/api/v1/data/status`       | DATA ENGINE queue, totals, counters    |
//! | GET    | `/api/v1/data/decoders`     | registered protocol decoders           |
//! | GET    | `/api/v1/data/transactions` | decoded transactions (keyset paging)   |
//! | GET    | `/api/v1/data/transactions/{signature}` | evidence, balances, activities, swaps |
//! | GET    | `/api/v1/data/swaps`        | normalized swaps (filters, paging)     |
//! | GET    | `/api/v1/data/issues`       | data-quality issues                    |
//! | GET    | `/api/v1/data/targets`      | ingestion targets and coverage         |
//! | GET    | `/api/v1/data/backfills`    | bounded backfills                      |
//! | GET    | `/api/v1/data/events`       | recent domain events (outbox)          |
//!
//! None of these endpoints contacts an external provider: GATEWAY endpoints
//! read only in-memory state and stored aggregates; DATA ENGINE endpoints
//! read PostgreSQL and in-process counters and never enqueue work.

pub mod contracts;
mod data_routes;
mod gateway_routes;
mod middleware;
mod state;

use std::future::IntoFuture;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use contracts::{
    ComponentCheck, ComponentStatus, DatabaseState, LiveResponse, LivenessStatus, ReadinessChecks,
    ReadinessStatus, ReadyResponse, SystemStatus, VersionInfo,
};
pub use middleware::json_error;
pub use state::{AppState, BuildInfo, HttpLimits};

use solprint_core::UtcTimestamp;
use solprint_storage::DatabaseHealth;

/// Build the complete router with all middleware applied.
pub fn router(state: AppState) -> Router {
    let limits = state.http_limits();
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/api/v1/system/status", get(system_status))
        .route("/api/v1/system/version", get(system_version))
        .route("/api/v1/gateway/status", get(gateway_routes::status))
        .route("/api/v1/gateway/providers", get(gateway_routes::providers))
        .route("/api/v1/gateway/usage", get(gateway_routes::usage))
        .route("/api/v1/gateway/metrics", get(gateway_routes::metrics))
        .route("/api/v1/gateway/budgets", get(gateway_routes::budgets))
        .route("/api/v1/gateway/streams", get(gateway_routes::streams))
        .route("/api/v1/gateway/cache", get(gateway_routes::cache))
        .route("/api/v1/data/status", get(data_routes::status))
        .route("/api/v1/data/decoders", get(data_routes::decoders))
        .route("/api/v1/data/transactions", get(data_routes::transactions))
        .route(
            "/api/v1/data/transactions/{signature}",
            get(data_routes::transaction),
        )
        .route("/api/v1/data/swaps", get(data_routes::swaps))
        .route("/api/v1/data/issues", get(data_routes::issues))
        .route("/api/v1/data/targets", get(data_routes::targets))
        .route("/api/v1/data/backfills", get(data_routes::backfills))
        .route("/api/v1/data/events", get(data_routes::events))
        .fallback(middleware::not_found)
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(limits.body_limit_bytes))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            limits.request_timeout,
        ))
        // Outermost: request id, access log, error envelope, security headers.
        .layer(axum::middleware::from_fn(middleware::observe))
}

/// Serve until `shutdown` is cancelled, then stop accepting connections and
/// let in-flight requests finish.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: CancellationToken,
) -> std::io::Result<()> {
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .into_future()
        .await
}

async fn live() -> impl IntoResponse {
    axum::Json(LiveResponse {
        status: LivenessStatus::Alive,
    })
}

fn component_check(health: DatabaseHealth) -> ComponentCheck {
    let checked_at = UtcTimestamp::now().to_rfc3339();
    match health {
        DatabaseHealth::Available { latency } => ComponentCheck {
            status: ComponentStatus::Available,
            latency_us: Some(u32::try_from(latency.as_micros()).unwrap_or(u32::MAX)),
            checked_at,
        },
        DatabaseHealth::Unavailable => ComponentCheck {
            status: ComponentStatus::Unavailable,
            latency_us: None,
            checked_at,
        },
    }
}

async fn ready(State(state): State<AppState>) -> Response {
    let database = component_check(state.database().health().await);
    let ready = database.status == ComponentStatus::Available;
    let body = ReadyResponse {
        status: if ready {
            ReadinessStatus::Ready
        } else {
            ReadinessStatus::NotReady
        },
        checks: ReadinessChecks { database },
    };
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    // A 503 here is an expected, structured readiness answer, not an error
    // envelope: mark it so the error middleware leaves it untouched.
    let mut response = (status, axum::Json(body)).into_response();
    response.extensions_mut().insert(middleware::StructuredBody);
    response
}

async fn system_status(State(state): State<AppState>) -> axum::Json<SystemStatus> {
    let health = state.database().health().await;
    let check = component_check(health);
    let migrations_applied = if check.status == ComponentStatus::Available {
        state
            .database()
            .applied_migration_versions()
            .await
            .ok()
            .map(|v| u32::try_from(v.len()).unwrap_or(u32::MAX))
    } else {
        None
    };
    axum::Json(SystemStatus {
        application: "SOLPRINT".to_owned(),
        environment: state.environment().to_owned(),
        version: state.build().version.to_owned(),
        trading: state.trading_state(),
        started_at: state.started_at().to_rfc3339(),
        timestamp: UtcTimestamp::now().to_rfc3339(),
        uptime_seconds: state.uptime().as_secs(),
        database: DatabaseState {
            status: check.status,
            latency_us: check.latency_us,
            migrations_applied,
            migrations_expected: u32::try_from(solprint_storage::MIGRATOR.iter().count())
                .unwrap_or(u32::MAX),
        },
        providers: state.providers(),
    })
}

async fn system_version(State(state): State<AppState>) -> axum::Json<VersionInfo> {
    let build = state.build();
    axum::Json(VersionInfo {
        name: "solprint".to_owned(),
        version: build.version.to_owned(),
        api_version: "v1".to_owned(),
        git_commit: build.git_commit.map(str::to_owned),
        build_profile: build.profile.to_owned(),
    })
}
