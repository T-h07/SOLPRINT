//! `/api/v1/radar/*` response contracts (Phase 03, RADAR).
//!
//! Every value comes from PostgreSQL, GATEWAY's in-memory budget snapshot or
//! RADAR's in-process counters; serving these endpoints never contacts a
//! provider and never schedules work. Scores, ratios and amounts are decimal
//! **strings**; counts are `u32` (saturating). Enumerations travel as
//! SCREAMING_SNAKE_CASE strings defined in `docs/architecture/RADAR.md`;
//! clients render unknown values as-is.
//!
//! A priority score orders investigation and monitoring work. It is not a
//! prediction, a recommendation or a measure of trading skill.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::data::LabelCount;

/// RADAR's view of the GATEWAY credit budget (local state only).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarBudgetDto {
    /// GATEWAY budget state, or `UNKNOWN` when no ledger snapshot exists.
    pub gateway_state: String,
    pub ledger_loaded: bool,
    /// `FULL`, `CONSERVATION` or `PAUSED`.
    pub mode: String,
    /// Why provider work is paused or reduced, when it is.
    pub reason: Option<String>,
    pub share_bps: u32,
    #[ts(type = "number")]
    pub allowance: u64,
    /// Allowance left after estimates of RADAR jobs already running.
    #[ts(type = "number")]
    pub available: u64,
    #[ts(type = "number")]
    pub outstanding_estimates: u64,
    #[ts(type = "number")]
    pub spent_24h: u64,
    pub spent_24h_by_class: Vec<LabelCount>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarCycleDto {
    pub cycle_id: String,
    pub trigger: String,
    pub status: String,
    pub budget_state: String,
    pub deferred_reason: Option<String>,
    pub observations: u32,
    pub markets_new: u32,
    pub markets_selected: u32,
    pub samples_scheduled: u32,
    pub credits_estimated: u32,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarRegistryTotals {
    pub wallets: u32,
    pub wallets_new_24h: u32,
    pub by_lifecycle: Vec<LabelCount>,
    pub by_eligibility: Vec<LabelCount>,
    pub evidence: u32,
    pub evidence_invalidated: u32,
    pub reactivations: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarMarketTotals {
    pub markets: u32,
    pub by_selection: Vec<LabelCount>,
    pub pools: u32,
    pub pools_supported: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarJobTotals {
    pub open: u32,
    pub by_state: Vec<LabelCount>,
    pub open_by_class: Vec<LabelCount>,
    pub deferred_by_reason: Vec<LabelCount>,
    pub oldest_pending_at: Option<String>,
}

/// Counters since this RADAR instance started (not persisted).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarProcessMetrics {
    #[ts(type = "number")]
    pub cycles: u64,
    #[ts(type = "number")]
    pub cycles_deferred: u64,
    #[ts(type = "number")]
    pub source_failures: u64,
    #[ts(type = "number")]
    pub provider_requests: u64,
    #[ts(type = "number")]
    pub events_processed: u64,
    #[ts(type = "number")]
    pub event_failures: u64,
    #[ts(type = "number")]
    pub evidence_recorded: u64,
    #[ts(type = "number")]
    pub evidence_invalidated: u64,
    #[ts(type = "number")]
    pub wallets_discovered: u64,
    #[ts(type = "number")]
    pub evaluations: u64,
    #[ts(type = "number")]
    pub evaluation_conflicts: u64,
    #[ts(type = "number")]
    pub jobs_started: u64,
    #[ts(type = "number")]
    pub jobs_completed: u64,
    #[ts(type = "number")]
    pub jobs_deferred: u64,
    #[ts(type = "number")]
    pub monitoring_changes: u64,
    #[ts(type = "number")]
    pub errors: u64,
    pub last_cycle_at: Option<String>,
    pub last_event_at: Option<String>,
    pub last_evaluation_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_at: Option<String>,
}

/// Effective RADAR settings (no secrets; all limits are local).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarSettingsDto {
    pub market_discovery_interval_secs: u32,
    pub max_market_samples_per_cycle: u32,
    pub max_concurrent_sample_jobs: u32,
    pub max_concurrent_history_jobs: u32,
    pub max_investigating_candidates: u32,
    pub max_live_monitored_wallets: u32,
    pub initial_sample_signatures: u32,
    pub history_stages: Vec<RadarStageDto>,
    /// Bucket shares in basis points, in `ESTABLISHED, EMERGING,
    /// LOCAL_ACTIVITY, HISTORICAL, EXPLORATION` order.
    pub bucket_shares_bps: Vec<LabelCount>,
    pub dormant_after_days: u32,
    pub archive_after_days: u32,
    pub classification_ruleset: String,
    pub priority_policy: String,
    pub lifecycle_policy: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarStageDto {
    pub stage: String,
    pub days: u32,
    pub signatures: u32,
}

/// `GET /api/v1/radar/status`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarStatus {
    pub enabled: bool,
    /// RADAR loops run in this process.
    pub running: bool,
    pub auto_discovery_enabled: bool,
    pub data_engine_running: bool,
    /// Live (stream) monitoring is possible; otherwise monitored wallets
    /// use periodic history refresh.
    pub live_monitoring_available: bool,
    pub paused: bool,
    pub pause_reason: Option<String>,
    pub cycle_requested: bool,
    pub budget: RadarBudgetDto,
    pub registry: RadarRegistryTotals,
    pub markets: RadarMarketTotals,
    pub jobs: RadarJobTotals,
    pub monitoring_active: Vec<LabelCount>,
    pub last_cycle: Option<RadarCycleDto>,
    pub last_completed_cycle_at: Option<String>,
    pub cycles_24h: u32,
    pub process: Option<RadarProcessMetrics>,
    pub settings: RadarSettingsDto,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarWalletDto {
    pub address: String,
    pub lifecycle: String,
    pub eligibility: String,
    pub eligibility_reasons: Vec<String>,
    pub qualification: Option<String>,
    pub qualification_reasons: Vec<String>,
    /// 0–100 investigation priority (decimal string); `null` before the
    /// first evaluation.
    pub priority_score: Option<String>,
    pub priority_policy: Option<String>,
    pub history_stage: String,
    pub coverage_state: String,
    pub coverage_start: Option<String>,
    pub coverage_end: Option<String>,
    pub monitoring_mode: String,
    pub first_discovery_source: String,
    pub first_discovered_at: String,
    pub last_verified_trade_at: Option<String>,
    pub last_investigated_at: Option<String>,
    pub last_evaluated_at: Option<String>,
    pub next_investigation_at: Option<String>,
    pub evidence: u32,
    pub valid_evidence: u32,
    pub verified_swaps: u32,
    pub distinct_tokens: u32,
    pub operator_excluded: bool,
    pub revision: u32,
}

/// `GET /api/v1/radar/wallets`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarWalletsResponse {
    pub items: Vec<RadarWalletDto>,
    pub next_cursor: Option<String>,
    pub sort: String,
}

// Metrics are stored by RADAR as camelCase JSON and served snake_case.

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all(deserialize = "camelCase"))]
#[ts(export)]
pub struct RadarPriorityDto {
    pub score: String,
    /// Components on 0..1 (decimal strings): A, R, E, P, D.
    pub activity: String,
    pub recency: String,
    pub evidence_quality: String,
    pub evidence_parts: Vec<String>,
    pub performance: String,
    /// Why P is what it is (e.g. `SHRUNK`, `NO_MATCHED_OUTCOMES`).
    pub performance_status: String,
    pub diversity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all(deserialize = "camelCase"))]
#[ts(export)]
pub struct RadarActivityWindowDto {
    pub window: String,
    pub swaps: u32,
    pub buys: u32,
    pub sells: u32,
    pub distinct_tokens: u32,
    pub trading_days: u32,
    pub undated: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all(deserialize = "camelCase"))]
#[ts(export)]
pub struct RadarDenominationDto {
    pub denomination: String,
    pub outcomes: u32,
    pub wins: u32,
    /// Raw quote units (decimal string).
    pub cost: String,
    pub proceeds: String,
    /// proceeds / cost, exact decimal; `null` when cost is zero.
    pub return_ratio: Option<String>,
    pub incomplete_fees: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all(deserialize = "camelCase"))]
#[ts(export)]
pub struct RadarScreeningDto {
    pub outcomes: u32,
    pub disposals: u32,
    pub disposals_matched: u32,
    pub matched_coverage_bps: u32,
    pub open_positions: u32,
    pub unevaluated_cycles: u32,
    pub ambiguous_tokens: u32,
    pub unknown_cost_tokens: u32,
    pub by_denomination: Vec<RadarDenominationDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all(deserialize = "camelCase"))]
#[ts(export)]
pub struct RadarCoverageDto {
    pub state: String,
    pub signatures_discovered: u32,
    pub transactions_retrieved: u32,
    pub transactions_unavailable: u32,
    pub transactions_verified: u32,
    pub transactions_unsupported: u32,
    pub stage_completed: Option<String>,
    pub swaps_analysed: u32,
    pub swap_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all(deserialize = "camelCase"))]
#[ts(export)]
pub struct RadarEvidenceTotalsDto {
    pub total: u32,
    pub valid: u32,
    pub valid_finalized: u32,
    pub distinct_tokens: u32,
}

/// The latest evaluation of a wallet, as RADAR computed it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all(deserialize = "camelCase"))]
#[ts(export)]
pub struct RadarWalletMetricsDto {
    pub policy: String,
    pub computed_at: String,
    pub priority: RadarPriorityDto,
    pub activity: Vec<RadarActivityWindowDto>,
    pub screening: RadarScreeningDto,
    pub coverage: RadarCoverageDto,
    pub evidence: RadarEvidenceTotalsDto,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarRetrievalDto {
    pub signatures_discovered: u32,
    pub transactions_retrieved: u32,
    pub transactions_pending: u32,
    pub transactions_unavailable: u32,
    pub credits_spent: u32,
    pub history_exhausted: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarMonitoringDto {
    pub address: String,
    pub mode: String,
    pub data_engine_target: Option<String>,
    pub priority_at_start: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
}

/// `GET /api/v1/radar/wallets/{address}`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarWalletDetail {
    pub wallet: RadarWalletDto,
    pub discovery_sources: Vec<String>,
    /// `null` until the first evaluation snapshot, or when the stored
    /// snapshot uses a shape this API version does not know.
    pub metrics: Option<RadarWalletMetricsDto>,
    pub snapshot_policy: Option<String>,
    pub retrieval: RadarRetrievalDto,
    pub monitoring: Vec<RadarMonitoringDto>,
    pub recent_jobs: Vec<RadarJobDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarEvidenceDto {
    pub swap_id: String,
    pub signature: String,
    #[ts(type = "number")]
    pub slot: u64,
    pub block_time: Option<String>,
    pub token_mint: Option<String>,
    pub quote_mint: Option<String>,
    pub pool: Option<String>,
    pub protocol: String,
    pub economic_role: String,
    pub attribution: String,
    pub finality: String,
    pub discovery_source: String,
    pub swap_revision: u32,
    pub valid: bool,
    pub invalidation_reason: Option<String>,
    pub invalidated_at: Option<String>,
    pub discovered_at: String,
}

/// `GET /api/v1/radar/wallets/{address}/evidence`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarEvidenceResponse {
    pub address: String,
    pub items: Vec<RadarEvidenceDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarTransitionDto {
    pub from_state: Option<String>,
    pub to_state: String,
    pub trigger: String,
    pub reasons: Vec<String>,
    pub policy: String,
    pub transitioned_at: String,
}

/// `GET /api/v1/radar/wallets/{address}/history`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarHistoryResponse {
    pub address: String,
    pub items: Vec<RadarTransitionDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarSnapshotDto {
    pub policy: String,
    pub input_hash: String,
    pub priority_score: String,
    pub qualification: String,
    pub computed_at: String,
}

/// `GET /api/v1/radar/wallets/{address}/snapshots`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarSnapshotsResponse {
    pub address: String,
    pub items: Vec<RadarSnapshotDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarPoolDto {
    pub pool: String,
    pub quote_mint: Option<String>,
    pub dex_id: Option<String>,
    /// `SUPPORTED`, `UNSUPPORTED_PROGRAM`, `NOT_FOUND`, `PENDING`, …
    pub validation: String,
    pub protocol: Option<String>,
    pub liquidity_usd: Option<String>,
    pub volume_24h_usd: Option<String>,
    pub txns_1h: Option<u32>,
    pub txns_24h: Option<u32>,
    /// When the provider last reported these market figures.
    pub observed_at: String,
    pub last_sampled_at: Option<String>,
    pub samples_completed: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarMarketDto {
    pub mint: String,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub classification: String,
    pub classification_reasons: Vec<String>,
    pub bucket: String,
    pub sources: Vec<String>,
    pub promotional_only: bool,
    pub selection_state: String,
    pub selection_reasons: Vec<String>,
    pub activity_state: String,
    pub market_priority: Option<String>,
    pub coverage_state: String,
    pub samples_completed: u32,
    pub wallets_found: u32,
    pub first_discovered_at: String,
    pub last_observed_at: Option<String>,
    /// The latest market observation is older than the configured maximum.
    pub stale: bool,
    pub last_sampled_at: Option<String>,
    pub pools: Vec<RadarPoolDto>,
}

/// `GET /api/v1/radar/markets`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarMarketsResponse {
    pub items: Vec<RadarMarketDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarSourceDto {
    pub source: String,
    pub runs: u32,
    pub runs_ok: u32,
    pub runs_deferred: u32,
    pub runs_failed: u32,
    pub items: u32,
    pub markets_new: u32,
    pub provider_requests: u32,
    pub credits_estimated: u32,
    pub last_run_at: Option<String>,
    pub last_status: Option<String>,
    /// Evidence first attributed to this source in the window.
    pub evidence: u32,
    pub unique_wallets: u32,
    pub eligible_wallets: u32,
    pub investigated_wallets: u32,
    pub qualified_wallets: u32,
}

/// `GET /api/v1/radar/sources`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarSourcesResponse {
    pub window_hours: u32,
    pub items: Vec<RadarSourceDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarJobDto {
    pub job_id: String,
    pub work_class: String,
    pub subject_kind: String,
    pub subject: String,
    pub origin: String,
    pub bucket: Option<String>,
    pub priority: u32,
    pub state: String,
    pub defer_reason: Option<String>,
    pub result_code: Option<String>,
    pub signature_limit: u32,
    pub lookback_days: Option<u32>,
    pub estimated_credits: u32,
    pub credits_spent: u32,
    pub signatures_found: u32,
    pub signatures_reused: u32,
    pub attempts: u32,
    pub backfill_status: Option<String>,
    pub next_run_at: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// `GET /api/v1/radar/jobs`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarJobsResponse {
    pub totals: RadarJobTotals,
    pub items: Vec<RadarJobDto>,
}

/// `GET /api/v1/radar/monitoring`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarMonitoringResponse {
    pub capacity: u32,
    pub live_available: bool,
    pub items: Vec<RadarMonitoringDto>,
}

/// `GET /api/v1/radar/cycles`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarCyclesResponse {
    pub items: Vec<RadarCycleDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarEventDto {
    pub event_id: String,
    #[ts(type = "number")]
    pub seq: i64,
    pub event_type: String,
    pub aggregate_id: String,
    pub aggregate_revision: u32,
    pub source: String,
    pub observed_at: String,
    pub persisted_at: String,
}

/// `GET /api/v1/radar/events`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RadarEventsResponse {
    pub items: Vec<RadarEventDto>,
}
