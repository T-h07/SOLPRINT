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
//!
//! None of these endpoints contacts an external provider.

pub mod contracts;
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
        providers: state.providers().to_vec(),
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
