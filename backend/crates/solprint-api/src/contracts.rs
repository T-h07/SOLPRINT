//! HTTP response contracts.
//!
//! These Rust types are the single source of truth for the API shape. The
//! TypeScript definitions in `frontend/src/types/generated/` are generated
//! from them by `ts-rs` whenever `cargo test` runs, and `scripts/check.ps1`
//! (also run by `scripts/verify.ps1`) fails if the committed TypeScript
//! differs from what the Rust types produce.
//!
//! Numeric fields are bounded to `u32` (or annotated as `number`) so every
//! value is exactly representable in JavaScript. Timestamps are UTC RFC 3339
//! strings.

pub mod data;
pub mod gateway;
pub mod radar;

use serde::Serialize;
use ts_rs::TS;

pub use gateway::GatewayHealthState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum LivenessStatus {
    Alive,
}

/// `GET /health/live`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LiveResponse {
    pub status: LivenessStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ReadinessStatus {
    Ready,
    NotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ComponentStatus {
    Available,
    Unavailable,
}

/// Result of probing one dependency.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ComponentCheck {
    pub status: ComponentStatus,
    /// Probe round-trip in microseconds (monotonic clock). `null` when the
    /// probe failed.
    pub latency_us: Option<u32>,
    /// UTC time the probe completed.
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReadinessChecks {
    pub database: ComponentCheck,
}

/// `GET /health/ready` (200 when ready, 503 when not).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReadyResponse {
    pub status: ReadinessStatus,
    pub checks: ReadinessChecks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum TradingModeDto {
    Paper,
    Live,
}

/// Whether this build can sign or submit transactions at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ExecutionCapability {
    /// No signing or submission code exists in this build.
    None,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TradingState {
    pub mode: TradingModeDto,
    pub live_trading_enabled: bool,
    pub execution_capability: ExecutionCapability,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DatabaseState {
    pub status: ComponentStatus,
    pub latency_us: Option<u32>,
    /// Number of successfully applied migrations; `null` when the database
    /// could not be queried.
    pub migrations_applied: Option<u32>,
    /// Number of migrations embedded in this build.
    pub migrations_expected: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ProviderId {
    Helius,
    Jupiter,
    DexScreener,
    SolanaRpc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum CredentialState {
    Configured,
    Absent,
    NotRequired,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProviderStatus {
    pub id: ProviderId,
    pub name: String,
    pub role: String,
    /// Derived by GATEWAY from real traffic only (`UNKNOWN` until a request
    /// has been made). No background probe exists.
    pub state: GatewayHealthState,
    pub credential: CredentialState,
    /// UTC time of the last real request outcome (success or failure);
    /// `null` until one happened. Historical diagnostics are never reported here.
    pub last_checked_at: Option<String>,
}

/// `GET /api/v1/system/status`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SystemStatus {
    pub application: String,
    pub environment: String,
    pub version: String,
    pub trading: TradingState,
    pub started_at: String,
    pub timestamp: String,
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    pub database: DatabaseState,
    pub providers: Vec<ProviderStatus>,
}

/// `GET /api/v1/system/version`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct VersionInfo {
    pub name: String,
    pub version: String,
    pub api_version: String,
    /// Git commit the binary was built from, when supplied at build time.
    pub git_commit: Option<String>,
    pub build_profile: String,
}

/// Stable machine-readable error codes. Never renamed once released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ApiErrorCode {
    BadRequest,
    NotFound,
    MethodNotAllowed,
    PayloadTooLarge,
    RequestTimeout,
    ServiceUnavailable,
    InternalError,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ApiErrorBody {
    pub code: ApiErrorCode,
    /// Human-readable, sanitized. May change; clients must branch on `code`.
    pub message: String,
    pub request_id: Option<String>,
}

/// Envelope for every non-2xx response.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ApiError {
    pub error: ApiErrorBody,
}
