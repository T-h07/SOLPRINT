//! `/api/v1/gateway/*` response contracts (Phase 01).
//!
//! Every value here comes from GATEWAY's local state or stored aggregates;
//! serving these endpoints never contacts a provider.
//!
//! Counters are `u64` exported as `number`: they are bounded far below
//! 2^53 in practice (credits by the configured monthly limit, bytes and
//! request counts by process lifetime). Durations are milliseconds or
//! microseconds as named.

use serde::Serialize;
use ts_rs::TS;

use super::{ExecutionCapability, ProviderId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum GatewayHealthState {
    /// Turned off by configuration, or a required credential is absent.
    Disabled,
    /// No request has been made yet; nothing has been measured.
    Unknown,
    Healthy,
    Degraded,
    Unavailable,
    BudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum CircuitStateDto {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum BudgetStateDto {
    Normal,
    Conservation,
    Critical,
    NormalBudgetExhausted,
    HardLimitReached,
    AccountUsageUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub enum PriorityDto {
    P0,
    P1,
    P2,
    P3,
    P4,
    P5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum StreamStateDto {
    Disconnected,
    Connecting,
    Subscribing,
    Connected,
    Degraded,
    Reconnecting,
    Disabled,
    BudgetExhausted,
    ShuttingDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ContinuityDto {
    Continuous,
    Recovered,
    Gap,
    Unknown,
}

/// Latency percentiles in microseconds (histogram bucket upper bounds).
/// `null` when nothing has been measured.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[ts(export)]
pub struct LatencyDto {
    #[ts(type = "number | null")]
    pub p50_us: Option<u64>,
    #[ts(type = "number | null")]
    pub p95_us: Option<u64>,
    #[ts(type = "number | null")]
    pub p99_us: Option<u64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CircuitDto {
    pub state: CircuitStateDto,
    pub consecutive_failures: u32,
    /// Milliseconds until a half-open probe is allowed (`null` unless open).
    #[ts(type = "number | null")]
    pub retry_in_ms: Option<u64>,
    #[ts(type = "number")]
    pub times_opened: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LaneDto {
    pub max_in_flight: u32,
    pub queue_limit: u32,
    pub in_flight: u32,
    pub queued: u32,
    pub peak_in_flight: u32,
    #[ts(type = "number")]
    pub admitted: u64,
    #[ts(type = "number")]
    pub queued_total: u64,
    #[ts(type = "number")]
    pub rejected: u64,
    #[ts(type = "number")]
    pub timed_out: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LimiterDto {
    pub group: String,
    /// Configured rate: `rate_count` requests per `rate_period_ms`.
    pub rate_count: u32,
    #[ts(type = "number")]
    pub rate_period_ms: u64,
    pub burst: u32,
    #[ts(type = "number")]
    pub admitted: u64,
    #[ts(type = "number")]
    pub delayed: u64,
    #[ts(type = "number")]
    pub rejected: u64,
    /// Upstream 429 / rate-limit answers received.
    #[ts(type = "number")]
    pub upstream_limited: u64,
    /// Remaining upstream-imposed pause, if any.
    #[ts(type = "number | null")]
    pub blocked_for_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct HealthDto {
    pub state: GatewayHealthState,
    pub reason: Option<String>,
    /// Measured from real traffic only; `null` until a request succeeds.
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_error_code: Option<String>,
    pub recent_failures: u32,
    pub recent_total: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TrafficDto {
    #[ts(type = "number")]
    pub requests: u64,
    #[ts(type = "number")]
    pub credits: u64,
    #[ts(type = "number")]
    pub bytes: u64,
    #[ts(type = "number")]
    pub success: u64,
    #[ts(type = "number")]
    pub errors: u64,
    #[ts(type = "number")]
    pub cache_hits: u64,
    #[ts(type = "number")]
    pub coalesced: u64,
    #[ts(type = "number")]
    pub retries: u64,
    #[ts(type = "number")]
    pub fallbacks: u64,
}

/// One provider in detail (`/gateway/providers`).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayProvider {
    pub id: ProviderId,
    pub name: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub health: HealthDto,
    pub circuit: CircuitDto,
    pub lane: Option<LaneDto>,
    pub limiters: Vec<LimiterDto>,
    pub traffic: TrafficDto,
    pub latency: LatencyDto,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayProviderSummary {
    pub id: ProviderId,
    pub name: String,
    pub enabled: bool,
    pub health: GatewayHealthState,
    pub circuit: CircuitStateDto,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BudgetSummary {
    pub provider: ProviderId,
    pub state: BudgetStateDto,
    #[ts(type = "number")]
    pub credits_used: u64,
    #[ts(type = "number")]
    pub credit_limit: u64,
    #[ts(type = "number")]
    pub credits_remaining: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StreamSummary {
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    /// `null` when streams are disabled.
    pub state: Option<StreamStateDto>,
    pub subscriptions: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PersistenceDto {
    pub ledger_loaded: bool,
    pub last_flush_at: Option<String>,
    pub last_flush_ok: Option<bool>,
    #[ts(type = "number")]
    pub flush_failures: u64,
    pub last_prune_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SchedulerDto {
    pub global_limit: u32,
    pub global_waiting: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CacheSummary {
    #[ts(type = "number")]
    pub entries: u64,
    #[ts(type = "number")]
    pub max_entries: u64,
}

/// `GET /api/v1/gateway/status`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayStatus {
    pub enabled: bool,
    /// GATEWAY is read-only: it never builds, signs or submits transactions.
    pub execution_capability: ExecutionCapability,
    pub generated_at: String,
    pub started_at: String,
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    pub providers: Vec<GatewayProviderSummary>,
    pub budget: Option<BudgetSummary>,
    pub streams: StreamSummary,
    pub persistence: PersistenceDto,
    pub scheduler: SchedulerDto,
    pub cache: CacheSummary,
}

/// `GET /api/v1/gateway/providers`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayProvidersResponse {
    pub generated_at: String,
    pub providers: Vec<GatewayProvider>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProviderUsageDto {
    pub provider: ProviderId,
    #[ts(type = "number")]
    pub requests: u64,
    #[ts(type = "number")]
    pub credits: u64,
    #[ts(type = "number")]
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UsageHourDto {
    pub provider: ProviderId,
    pub hour_start: String,
    #[ts(type = "number")]
    pub requests: u64,
    #[ts(type = "number")]
    pub credits: u64,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// `GET /api/v1/gateway/usage?hours=N` (1–168, default 24)
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayUsageResponse {
    pub generated_at: String,
    pub since: String,
    /// Totals since this process started (in memory).
    pub process: Vec<ProviderUsageDto>,
    /// Stored hourly aggregates; empty with `history_available = false` when
    /// the database could not be read.
    pub history: Vec<UsageHourDto>,
    pub history_available: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ErrorCountDto {
    pub code: String,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OperationMetricsDto {
    pub provider: ProviderId,
    pub operation: String,
    #[ts(type = "number")]
    pub success: u64,
    #[ts(type = "number")]
    pub errors: u64,
    #[ts(type = "number")]
    pub cache_hits: u64,
    #[ts(type = "number")]
    pub coalesced: u64,
    #[ts(type = "number")]
    pub retries: u64,
    #[ts(type = "number")]
    pub fallbacks: u64,
    pub latency: LatencyDto,
    pub errors_by_code: Vec<ErrorCountDto>,
    pub last_error_code: Option<String>,
    pub last_error_at: Option<String>,
}

/// `GET /api/v1/gateway/metrics` (since process start)
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayMetricsResponse {
    pub generated_at: String,
    pub operations: Vec<OperationMetricsDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PriorityCountDto {
    pub priority: PriorityDto,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BudgetDto {
    pub provider: ProviderId,
    pub state: BudgetStateDto,
    /// Lowest-urgency priority still admitted (`null`: none).
    pub admits_up_to: Option<PriorityDto>,
    pub cycle_start: String,
    pub cycle_end: String,
    #[ts(type = "number")]
    pub credit_limit: u64,
    #[ts(type = "number")]
    pub reserve_credits: u64,
    #[ts(type = "number")]
    pub normal_credits: u64,
    #[ts(type = "number")]
    pub conservation_at: u64,
    #[ts(type = "number")]
    pub critical_at: u64,
    #[ts(type = "number")]
    pub credits_used: u64,
    #[ts(type = "number")]
    pub credits_pending_persist: u64,
    #[ts(type = "number")]
    pub credits_remaining: u64,
    /// Linear projection to cycle end; `null` during the first hour.
    #[ts(type = "number | null")]
    pub projected_cycle_credits: Option<u64>,
    #[ts(type = "number")]
    pub stream_bytes: u64,
    #[ts(type = "number")]
    pub stream_credits: u64,
    pub denied_by_priority: Vec<PriorityCountDto>,
    pub ledger_loaded: bool,
    pub last_persisted_at: Option<String>,
    #[ts(type = "number")]
    pub persist_failures: u64,
}

/// `GET /api/v1/gateway/budgets`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayBudgetsResponse {
    pub generated_at: String,
    pub budgets: Vec<BudgetDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct StreamConnectionDto {
    pub state: StreamStateDto,
    #[ts(type = "number")]
    pub generation: u64,
    pub connected_since: Option<String>,
    pub last_message_at: Option<String>,
    #[ts(type = "number")]
    pub reconnects: u64,
    #[ts(type = "number")]
    pub bytes_received: u64,
    #[ts(type = "number")]
    pub credits_charged: u64,
    #[ts(type = "number")]
    pub events_emitted: u64,
    #[ts(type = "number")]
    pub events_dropped: u64,
    #[ts(type = "number")]
    pub gaps: u64,
    #[ts(type = "number")]
    pub recovered: u64,
    #[ts(type = "number")]
    pub duplicates_suppressed: u64,
    pub last_error: Option<String>,
    pub max_subscriptions: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SubscriptionDto {
    #[ts(type = "number")]
    pub id: u64,
    pub kind: String,
    pub commitment: Option<String>,
    pub durable_key: Option<String>,
    pub active: bool,
    #[ts(type = "number")]
    pub notifications: u64,
    #[ts(type = "number")]
    pub dropped: u64,
    pub continuity: ContinuityDto,
    #[ts(type = "number | null")]
    pub last_slot: Option<u64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CheckpointDto {
    pub stream_key: String,
    pub kind: String,
    pub commitment: String,
    #[ts(type = "number | null")]
    pub last_slot: Option<u64>,
    pub last_signature: Option<String>,
    pub continuity: String,
    pub updated_at: String,
}

/// `GET /api/v1/gateway/streams`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayStreamsResponse {
    pub generated_at: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub connection: Option<StreamConnectionDto>,
    pub subscriptions: Vec<SubscriptionDto>,
    /// Stored checkpoints; empty with `checkpoints_available = false` when
    /// the database could not be read.
    pub checkpoints: Vec<CheckpointDto>,
    pub checkpoints_available: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CacheNamespaceDto {
    pub namespace: String,
    #[ts(type = "number")]
    pub hits: u64,
    #[ts(type = "number")]
    pub stale_hits: u64,
    #[ts(type = "number")]
    pub negative_hits: u64,
    #[ts(type = "number")]
    pub misses: u64,
    #[ts(type = "number")]
    pub inserts: u64,
}

/// `GET /api/v1/gateway/cache`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayCacheResponse {
    pub generated_at: String,
    #[ts(type = "number")]
    pub entries: u64,
    #[ts(type = "number")]
    pub max_entries: u64,
    pub namespaces: Vec<CacheNamespaceDto>,
}
