//! `/api/v1/data/*` response contracts (Phase 02, DATA ENGINE).
//!
//! Every value comes from PostgreSQL or the engine's in-process counters;
//! serving these endpoints never contacts a provider. Raw token amounts and
//! lamports are decimal **strings** (u64 does not fit a JavaScript number);
//! exchange ratios are decimal strings. Counts are `u32` (saturating).
//! Enumerations travel as SCREAMING_SNAKE_CASE strings defined in
//! `docs/architecture/DATA_ENGINE.md`; clients must render unknown values
//! as-is rather than guessing.

use serde::Serialize;
use ts_rs::TS;

use super::ExecutionCapability;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LabelCount {
    pub label: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataQueueDto {
    pub depth: u32,
    pub capacity: u32,
    pub by_state: Vec<LabelCount>,
    pub oldest_pending_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataTransactionTotals {
    pub total: u32,
    pub by_finality: Vec<LabelCount>,
    pub by_decode_status: Vec<LabelCount>,
    pub last_decoded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataSwapTotals {
    pub canonical: u32,
    pub verified: u32,
    pub retracted: u32,
    pub by_protocol: Vec<LabelCount>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataOutboxTotals {
    pub events: u32,
    pub undelivered: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataEvidenceTotals {
    #[ts(type = "number")]
    pub bytes: u64,
    pub pruned: u32,
    pub retention_days: u32,
}

/// Process counters since this engine instance started (not persisted).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataProcessMetrics {
    #[ts(type = "number")]
    pub jobs_claimed: u64,
    #[ts(type = "number")]
    pub fetched: u64,
    #[ts(type = "number")]
    pub fetched_cached: u64,
    #[ts(type = "number")]
    pub persisted: u64,
    #[ts(type = "number")]
    pub unchanged: u64,
    #[ts(type = "number")]
    pub retries: u64,
    #[ts(type = "number")]
    pub unavailable: u64,
    #[ts(type = "number")]
    pub quarantined: u64,
    #[ts(type = "number")]
    pub budget_pauses: u64,
    #[ts(type = "number")]
    pub credits_charged: u64,
    #[ts(type = "number")]
    pub finality_checks: u64,
    #[ts(type = "number")]
    pub stream_observations: u64,
    #[ts(type = "number")]
    pub stream_gaps: u64,
    #[ts(type = "number")]
    pub backfill_pages: u64,
    #[ts(type = "number")]
    pub outbox_delivered: u64,
    /// Mean decode time in microseconds; `null` before the first decode.
    #[ts(type = "number | null")]
    pub mean_decode_us: Option<u64>,
    pub last_error_code: Option<String>,
    pub last_error_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataEngineSettingsDto {
    pub auto_ingest_enabled: bool,
    pub backfill_enabled: bool,
    pub price_enrichment_enabled: bool,
    pub default_commitment: String,
    pub fetch_concurrency: u32,
    pub max_active_targets: u32,
}

/// `GET /api/v1/data/status`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataEngineStatus {
    /// `false` when `DATA_ENGINE_ENABLED=false`: stored data is still served.
    pub running: bool,
    /// The Data Engine is read-only: it never builds, signs or submits.
    pub execution_capability: ExecutionCapability,
    pub decoder_version: String,
    pub generated_at: String,
    /// When the aggregates below were computed (cached up to the refresh
    /// interval); `null` when they are unavailable.
    pub counts_at: Option<String>,
    /// `false` when PostgreSQL could not be read: every total is then absent.
    pub counts_available: bool,
    pub queue: Option<DataQueueDto>,
    pub transactions: Option<DataTransactionTotals>,
    pub swaps: Option<DataSwapTotals>,
    pub open_issues_by_severity: Vec<LabelCount>,
    pub outbox: Option<DataOutboxTotals>,
    pub evidence: Option<DataEvidenceTotals>,
    pub targets_enabled: Option<u32>,
    pub targets_total: Option<u32>,
    pub backfills_active: Option<u32>,
    pub process: Option<DataProcessMetrics>,
    pub settings: Option<DataEngineSettingsDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DecoderDto {
    pub id: String,
    pub protocol: String,
    pub program_id: String,
}

/// `GET /api/v1/data/decoders`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataDecodersResponse {
    pub decoder_version: String,
    pub network: String,
    pub decoders: Vec<DecoderDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TransactionSummaryDto {
    pub signature: String,
    #[ts(type = "number")]
    pub slot: u64,
    pub block_time: Option<String>,
    /// `SUCCESS` | `FAILED`
    pub execution_status: String,
    /// Lamports, decimal string.
    pub fee_lamports: String,
    pub fee_payer: String,
    pub finality: String,
    pub decode_status: String,
    pub decoder_version: String,
    pub protocols: Vec<String>,
    pub activity_kinds: Vec<String>,
    pub swap_count: u32,
    pub revision: u32,
    pub first_observed_at: String,
    pub decoded_at: String,
}

/// `GET /api/v1/data/transactions`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataTransactionsResponse {
    pub generated_at: String,
    pub items: Vec<TransactionSummaryDto>,
    /// Pass as `cursor` for the next (older) page; `null` at the end.
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct EvidenceDto {
    pub revision: u32,
    pub commitment: String,
    pub provider: String,
    #[ts(type = "number")]
    pub slot: u64,
    pub fetched_at: String,
    pub content_sha256: String,
    pub byte_len: u32,
    pub pruned: bool,
    pub canonical: bool,
    pub superseded_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BalanceChangeDto {
    pub account_index: u32,
    /// `SOL` or a mint address.
    pub asset: String,
    pub account: String,
    pub owner: Option<String>,
    pub token_program: Option<String>,
    pub decimals: Option<u32>,
    pub pre_amount: Option<String>,
    pub post_amount: Option<String>,
    /// Signed decimal string.
    pub delta: Option<String>,
    pub pre_evidence: String,
    pub post_evidence: String,
    pub ownership_evidence: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ActivityDto {
    pub activity_id: String,
    pub index: u32,
    pub kind: String,
    pub status: String,
    pub protocol: Option<String>,
    pub program_id: Option<String>,
    pub instruction_ref: Option<String>,
    pub actor: Option<String>,
    pub attribution: Option<String>,
    pub mint: Option<String>,
    pub amount: Option<String>,
    pub source_account: Option<String>,
    pub destination_account: Option<String>,
    pub counterparty: Option<String>,
    pub reasons: Vec<String>,
    pub decoder: String,
    pub canonical: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SwapAssetDto {
    pub mint: Option<String>,
    /// Raw base units, decimal string.
    pub amount: Option<String>,
    pub decimals: Option<u32>,
    pub asset_kind: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RouteLegDto {
    pub index: u32,
    pub protocol: String,
    pub program_id: Option<String>,
    pub pool: Option<String>,
    pub input_mint: String,
    pub input_amount: String,
    pub output_mint: String,
    pub output_amount: String,
    pub instruction_ref: Option<String>,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SwapFeeDto {
    pub kind: String,
    pub mint: Option<String>,
    pub amount: Option<String>,
    pub provenance: String,
    pub recipient: Option<String>,
}

/// Point-in-time valuation. Price enrichment is off by default; the state
/// says why a value is absent instead of showing zero.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SwapValuationDto {
    /// `NOT_ENRICHED` (enrichment disabled) | `PRICE_UNAVAILABLE` | `VALUED`
    pub state: String,
    pub usd_value: Option<String>,
    pub price_source: Option<String>,
    pub price_observed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SwapDto {
    pub swap_id: String,
    pub signature: String,
    #[ts(type = "number")]
    pub slot: u64,
    pub block_time: Option<String>,
    /// Only set for verified attribution; never the fee payer by default.
    pub wallet: Option<String>,
    pub attribution: String,
    pub authority: Option<String>,
    pub beneficiary: Option<String>,
    pub fee_payer: String,
    pub input: SwapAssetDto,
    pub output: SwapAssetDto,
    /// Output per input in UI units, decimal string (18 dp, truncated).
    pub exchange_ratio: Option<String>,
    pub protocol: String,
    pub venue_kind: String,
    pub aggregator: Option<String>,
    pub route_complete: bool,
    pub instruction_ref: String,
    pub classification_status: String,
    pub reasons: Vec<String>,
    pub finality: String,
    pub canonical: bool,
    pub retraction_reason: Option<String>,
    pub revision: u32,
    pub decoder_version: String,
    pub legs: Vec<RouteLegDto>,
    pub fees: Vec<SwapFeeDto>,
    pub valuation: SwapValuationDto,
    pub first_observed_at: String,
    pub decoded_at: String,
    pub finalized_at: Option<String>,
    pub corrected_at: Option<String>,
}

/// `GET /api/v1/data/swaps`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataSwapsResponse {
    pub generated_at: String,
    pub items: Vec<SwapDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct QualityIssueDto {
    #[ts(type = "number")]
    pub issue_id: i64,
    pub signature: Option<String>,
    pub target_id: Option<String>,
    pub backfill_id: Option<String>,
    pub code: String,
    pub severity: String,
    pub detail: Option<String>,
    pub decoder_version: Option<String>,
    pub occurrences: u32,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub resolved_at: Option<String>,
}

/// `GET /api/v1/data/transactions/{signature}`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataTransactionDetail {
    pub generated_at: String,
    pub summary: TransactionSummaryDto,
    pub transaction_index: Option<u32>,
    pub version: String,
    pub err_json: Option<String>,
    pub signers: Vec<String>,
    pub compute_units: Option<String>,
    pub finality_checks: u32,
    pub finalized_at: Option<String>,
    pub evidence: Vec<EvidenceDto>,
    pub balances: Vec<BalanceChangeDto>,
    pub activities: Vec<ActivityDto>,
    pub swaps: Vec<SwapDto>,
    pub issues: Vec<QualityIssueDto>,
}

/// `GET /api/v1/data/issues`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataIssuesResponse {
    pub generated_at: String,
    pub items: Vec<QualityIssueDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TargetDto {
    pub target_id: String,
    pub target_type: String,
    pub address: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub priority: String,
    pub commitment: String,
    pub discovery_source: String,
    pub created_at: String,
    pub last_observation_at: Option<String>,
    #[ts(type = "number | null")]
    pub history_oldest_slot: Option<u64>,
    #[ts(type = "number | null")]
    pub history_newest_slot: Option<u64>,
    pub history_exhausted: bool,
    pub coverage_state: String,
}

/// `GET /api/v1/data/targets`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataTargetsResponse {
    pub generated_at: String,
    pub max_active_targets: u32,
    pub items: Vec<TargetDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BackfillDto {
    pub backfill_id: String,
    pub target_id: Option<String>,
    pub address: String,
    pub commitment: String,
    pub status: String,
    pub stop_reason: Option<String>,
    pub coverage_state: String,
    pub cancel_requested: bool,
    pub pages_fetched: u32,
    pub max_pages: u32,
    pub discovered_count: u32,
    pub queued_count: u32,
    pub duplicate_count: u32,
    pub max_signatures: u32,
    pub credits_estimated: u32,
    pub credits_spent: u32,
    #[ts(type = "number | null")]
    pub min_slot: Option<u64>,
    #[ts(type = "number | null")]
    pub oldest_slot_seen: Option<u64>,
    #[ts(type = "number | null")]
    pub newest_slot_seen: Option<u64>,
    pub history_exhausted: bool,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// `GET /api/v1/data/backfills`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataBackfillsResponse {
    pub generated_at: String,
    pub items: Vec<BackfillDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DomainEventDto {
    pub event_id: String,
    #[ts(type = "number")]
    pub seq: i64,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_revision: u32,
    pub quality_state: String,
    pub persisted_at: String,
}

/// `GET /api/v1/data/events`
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DataEventsResponse {
    pub generated_at: String,
    pub items: Vec<DomainEventDto>,
}
