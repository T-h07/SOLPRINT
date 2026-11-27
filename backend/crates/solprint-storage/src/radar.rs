//! RADAR persistence (`migrations/20261127000004_radar.sql`).
//!
//! RADAR stores references to DATA ENGINE records (signatures, swap ids,
//! backfill ids), never copies of them. Unsigned amounts travel as decimal
//! strings (`::numeric` in, `::text` out); USD figures reported by providers
//! are decimal strings too, never floats.
//!
//! Invariants enforced here:
//! * one registry row per (network, address); upserts are atomic;
//! * one evidence row per (wallet, swap); a revision never goes backwards and
//!   invalidated evidence is kept with its reason;
//! * applying a DATA ENGINE event and recording its delivery to the `radar`
//!   consumer commit in **one** transaction, so redelivery is a no-op;
//! * an evaluation updates the wallet, appends its transitions, its snapshot
//!   (only when the inputs changed) and its RADAR events in one transaction,
//!   guarded by the wallet revision;
//! * at most one open job per (work class, subject) (unique partial index).

use std::time::Duration;

use solprint_core::UtcTimestamp;
use sqlx::types::Uuid;
use sqlx::{Postgres, Row, Transaction};
use time::OffsetDateTime;

use crate::{Database, StorageError};

pub const NETWORK: &str = "solana-mainnet";
/// The DATA ENGINE outbox consumer name RADAR uses.
pub const RADAR_CONSUMER: &str = "radar";

macro_rules! const_sql {
    ($($t:tt)*) => {
        sqlx::AssertSqlSafe(format!($($t)*))
    };
}

fn ts(t: &UtcTimestamp) -> OffsetDateTime {
    *t.as_offset_datetime()
}

fn utc(t: OffsetDateTime) -> UtcTimestamp {
    UtcTimestamp::from_offset_datetime(t)
}

fn opt_utc(t: Option<OffsetDateTime>) -> Option<UtcTimestamp> {
    t.map(utc)
}

fn e(err: &sqlx::Error) -> StorageError {
    StorageError::from_sqlx(err)
}

fn i64_of(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn u64_of(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn opt_u64(v: Option<i64>) -> Option<u64> {
    v.map(u64_of)
}

fn secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

// ---- control ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RadarControl {
    pub paused: bool,
    pub pause_reason: Option<String>,
    pub cycle_requested: bool,
    pub last_cycle_at: Option<UtcTimestamp>,
}

// ---- cycles and sources ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCycle {
    pub cycle_id: Uuid,
    pub trigger: String,
    pub seed: i64,
    pub budget_state: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleFinish {
    pub status: String,
    pub deferred_reason: Option<String>,
    pub observations: i32,
    pub markets_new: i32,
    pub markets_selected: i32,
    pub samples_scheduled: i32,
    pub credits_estimated: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleRow {
    pub cycle_id: Uuid,
    pub trigger: String,
    pub status: String,
    pub seed: i64,
    pub budget_state: String,
    pub deferred_reason: Option<String>,
    pub observations: i32,
    pub markets_new: i32,
    pub markets_selected: i32,
    pub samples_scheduled: i32,
    pub credits_estimated: i32,
    pub started_at: UtcTimestamp,
    pub finished_at: Option<UtcTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRunWrite {
    pub run_id: Uuid,
    pub cycle_id: Uuid,
    pub source: String,
    pub status: String,
    pub error_code: Option<String>,
    pub items: i32,
    pub markets_new: i32,
    pub provider_requests: i32,
    pub credits_estimated: i32,
    pub started_at: UtcTimestamp,
    pub finished_at: UtcTimestamp,
}

/// Per-source effectiveness over a time range (local counts only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMetricsRow {
    pub source: String,
    pub runs: i64,
    pub runs_ok: i64,
    pub runs_deferred: i64,
    pub runs_failed: i64,
    pub items: i64,
    pub markets_new: i64,
    pub provider_requests: i64,
    pub credits_estimated: i64,
    pub last_run_at: Option<UtcTimestamp>,
    pub last_status: Option<String>,
}

/// Wallet yield by discovery source (from evidence; one wallet counts once
/// per source no matter how many swaps named it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceYieldRow {
    pub source: String,
    pub evidence: i64,
    pub unique_wallets: i64,
    pub eligible_wallets: i64,
    pub investigated_wallets: i64,
    pub qualified_wallets: i64,
}

// ---- markets and pools ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketUpsert {
    pub mint: String,
    pub token_program: Option<String>,
    pub display_symbol: Option<String>,
    pub display_name: Option<String>,
    pub classification: String,
    pub classification_reasons: Vec<String>,
    pub ruleset: String,
    pub bucket: String,
    pub source: String,
    pub promotional: bool,
    pub observed_at: UtcTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketRow {
    pub mint: String,
    pub token_program: Option<String>,
    pub display_symbol: Option<String>,
    pub display_name: Option<String>,
    pub classification: String,
    pub classification_reasons: Vec<String>,
    pub classification_ruleset: String,
    pub bucket: String,
    pub sources: Vec<String>,
    pub promotional_only: bool,
    pub selection_state: String,
    pub selection_reasons: Vec<String>,
    pub activity_state: String,
    pub market_priority: Option<String>,
    pub coverage_state: String,
    pub enabled: bool,
    pub samples_completed: i32,
    pub wallets_found: i32,
    pub first_discovered_at: UtcTimestamp,
    pub last_market_observation_at: Option<UtcTimestamp>,
    pub last_sampled_at: Option<UtcTimestamp>,
    pub updated_at: UtcTimestamp,
}

const MARKET_COLS: &str = "mint, token_program, display_symbol, display_name, classification,
    classification_reasons, classification_ruleset, bucket, sources, promotional_only, selection_state,
    selection_reasons, activity_state, market_priority::text AS market_priority, coverage_state, enabled,
    samples_completed, wallets_found, first_discovered_at, last_market_observation_at, last_sampled_at, updated_at";

fn market_row(r: &sqlx::postgres::PgRow) -> MarketRow {
    MarketRow {
        mint: r.get("mint"),
        token_program: r.get("token_program"),
        display_symbol: r.get("display_symbol"),
        display_name: r.get("display_name"),
        classification: r.get("classification"),
        classification_reasons: r.get("classification_reasons"),
        classification_ruleset: r.get("classification_ruleset"),
        bucket: r.get("bucket"),
        sources: r.get("sources"),
        promotional_only: r.get("promotional_only"),
        selection_state: r.get("selection_state"),
        selection_reasons: r.get("selection_reasons"),
        activity_state: r.get("activity_state"),
        market_priority: r.get("market_priority"),
        coverage_state: r.get("coverage_state"),
        enabled: r.get("enabled"),
        samples_completed: r.get("samples_completed"),
        wallets_found: r.get("wallets_found"),
        first_discovered_at: utc(r.get("first_discovered_at")),
        last_market_observation_at: opt_utc(r.get("last_market_observation_at")),
        last_sampled_at: opt_utc(r.get("last_sampled_at")),
        updated_at: utc(r.get("updated_at")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolUpsert {
    pub pool: String,
    pub mint: String,
    pub quote_mint: Option<String>,
    pub dex_id: Option<String>,
    pub labels: Vec<String>,
    pub liquidity_usd: Option<String>,
    pub volume_24h_usd: Option<String>,
    pub txns_1h: Option<i32>,
    pub txns_24h: Option<i32>,
    pub pair_created_at: Option<UtcTimestamp>,
    pub observed_at: UtcTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolRow {
    pub pool: String,
    pub mint: String,
    pub quote_mint: Option<String>,
    pub dex_id: Option<String>,
    pub labels: Vec<String>,
    pub validation: String,
    pub program_id: Option<String>,
    pub protocol: Option<String>,
    pub validated_at: Option<UtcTimestamp>,
    pub liquidity_usd: Option<String>,
    pub volume_24h_usd: Option<String>,
    pub txns_1h: Option<i32>,
    pub txns_24h: Option<i32>,
    pub pair_created_at: Option<UtcTimestamp>,
    pub last_observed_at: UtcTimestamp,
    pub last_sampled_at: Option<UtcTimestamp>,
    pub sample_until_signature: Option<String>,
    pub samples_completed: i32,
}

const POOL_COLS: &str =
    "pool, mint, quote_mint, dex_id, labels, validation, program_id, protocol, validated_at,
    liquidity_usd::text AS liquidity_usd, volume_24h_usd::text AS volume_24h_usd, txns_1h, txns_24h,
    pair_created_at, last_observed_at, last_sampled_at, sample_until_signature, samples_completed";

fn pool_row(r: &sqlx::postgres::PgRow) -> PoolRow {
    PoolRow {
        pool: r.get("pool"),
        mint: r.get("mint"),
        quote_mint: r.get("quote_mint"),
        dex_id: r.get("dex_id"),
        labels: r.get("labels"),
        validation: r.get("validation"),
        program_id: r.get("program_id"),
        protocol: r.get("protocol"),
        validated_at: opt_utc(r.get("validated_at")),
        liquidity_usd: r.get("liquidity_usd"),
        volume_24h_usd: r.get("volume_24h_usd"),
        txns_1h: r.get("txns_1h"),
        txns_24h: r.get("txns_24h"),
        pair_created_at: opt_utc(r.get("pair_created_at")),
        last_observed_at: utc(r.get("last_observed_at")),
        last_sampled_at: opt_utc(r.get("last_sampled_at")),
        sample_until_signature: r.get("sample_until_signature"),
        samples_completed: r.get("samples_completed"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationWrite {
    pub cycle_id: Option<Uuid>,
    pub mint: String,
    pub pool: Option<String>,
    pub source: String,
    pub promotional: bool,
    pub source_rank: Option<i32>,
    pub liquidity_usd: Option<String>,
    pub volume_24h_usd: Option<String>,
    pub txns_1h: Option<i32>,
    pub txns_24h: Option<i32>,
    pub pair_created_at: Option<UtcTimestamp>,
    pub organic_score: Option<String>,
    pub decision: Option<String>,
    pub decision_reasons: Vec<String>,
    pub observed_at: UtcTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketSelection {
    pub mint: String,
    pub selection_state: String,
    pub selection_reasons: Vec<String>,
    pub activity_state: String,
    pub market_priority: Option<String>,
    pub coverage_state: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarketFilter {
    pub selection_state: Option<String>,
    pub bucket: Option<String>,
    pub classification: Option<String>,
    /// Keyset cursor: rows after (first_discovered_at, mint) descending.
    pub before: Option<(UtcTimestamp, String)>,
    pub limit: i64,
}

// ---- jobs ------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJob {
    pub job_id: Uuid,
    pub work_class: String,
    pub subject_kind: String,
    pub subject: String,
    pub origin: String,
    pub bucket: Option<String>,
    pub cycle_id: Option<Uuid>,
    pub priority: i32,
    pub signature_limit: i32,
    pub lookback_days: Option<i32>,
    pub start_before_signature: Option<String>,
    pub until_signature: Option<String>,
    pub min_slot: Option<u64>,
    pub commitment: String,
    pub estimated_credits: i32,
    pub next_run_at: Option<UtcTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub job_id: Uuid,
    pub work_class: String,
    pub subject_kind: String,
    pub subject: String,
    pub origin: String,
    pub bucket: Option<String>,
    pub cycle_id: Option<Uuid>,
    pub priority: i32,
    pub state: String,
    pub defer_reason: Option<String>,
    pub result_code: Option<String>,
    pub signature_limit: i32,
    pub lookback_days: Option<i32>,
    pub start_before_signature: Option<String>,
    pub until_signature: Option<String>,
    pub min_slot: Option<u64>,
    pub commitment: String,
    pub backfill_id: Option<Uuid>,
    pub estimated_credits: i32,
    pub credits_spent: i32,
    pub signatures_found: i32,
    pub signatures_reused: i32,
    pub attempts: i32,
    pub cancel_requested: bool,
    pub next_run_at: UtcTimestamp,
    pub created_at: UtcTimestamp,
    pub started_at: Option<UtcTimestamp>,
    pub completed_at: Option<UtcTimestamp>,
    pub updated_at: UtcTimestamp,
    /// The linked DATA ENGINE backfill's status, when there is one.
    pub backfill_status: Option<String>,
}

const JOB_COLS: &str =
    "j.job_id, j.work_class, j.subject_kind, j.subject, j.origin, j.bucket, j.cycle_id,
    j.priority, j.state, j.defer_reason, j.result_code, j.signature_limit, j.lookback_days,
    j.start_before_signature, j.until_signature, j.min_slot, j.commitment, j.backfill_id,
    j.estimated_credits, j.credits_spent, j.signatures_found, j.signatures_reused, j.attempts,
    j.cancel_requested, j.next_run_at, j.created_at, j.started_at, j.completed_at, j.updated_at,
    b.status AS backfill_status";

fn job_row(r: &sqlx::postgres::PgRow) -> JobRow {
    JobRow {
        job_id: r.get("job_id"),
        work_class: r.get("work_class"),
        subject_kind: r.get("subject_kind"),
        subject: r.get("subject"),
        origin: r.get("origin"),
        bucket: r.get("bucket"),
        cycle_id: r.get("cycle_id"),
        priority: r.get("priority"),
        state: r.get("state"),
        defer_reason: r.get("defer_reason"),
        result_code: r.get("result_code"),
        signature_limit: r.get("signature_limit"),
        lookback_days: r.get("lookback_days"),
        start_before_signature: r.get("start_before_signature"),
        until_signature: r.get("until_signature"),
        min_slot: opt_u64(r.get("min_slot")),
        commitment: r.get("commitment"),
        backfill_id: r.get("backfill_id"),
        estimated_credits: r.get("estimated_credits"),
        credits_spent: r.get("credits_spent"),
        signatures_found: r.get("signatures_found"),
        signatures_reused: r.get("signatures_reused"),
        attempts: r.get("attempts"),
        cancel_requested: r.get("cancel_requested"),
        next_run_at: utc(r.get("next_run_at")),
        created_at: utc(r.get("created_at")),
        started_at: opt_utc(r.get("started_at")),
        completed_at: opt_utc(r.get("completed_at")),
        updated_at: utc(r.get("updated_at")),
        backfill_status: r.get("backfill_status"),
    }
}

/// Progress of a job's DATA ENGINE backfill and of the transactions it queued.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackfillProgress {
    pub status: String,
    pub stop_reason: Option<String>,
    pub coverage_state: String,
    pub discovered: i32,
    pub queued: i32,
    pub duplicates: i32,
    pub credits_spent: i32,
    pub oldest_slot: Option<u64>,
    pub newest_slot: Option<u64>,
    pub history_exhausted: bool,
    /// Observed signatures whose ingestion job is not terminal yet.
    pub transactions_pending: i64,
    /// Ingestion jobs that ended without a transaction (unavailable,
    /// unsupported, quarantined).
    pub transactions_unavailable: i64,
    pub transactions_retrieved: i64,
    pub transactions_failed: i64,
    pub transactions_verified: i64,
    pub transactions_unsupported: i64,
    /// Newest observed signature (the next incremental sample stops there).
    pub newest_signature: Option<String>,
    pub oldest_block_time: Option<UtcTimestamp>,
    pub newest_block_time: Option<UtcTimestamp>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobFilter {
    pub state: Option<String>,
    pub work_class: Option<String>,
    pub subject: Option<String>,
    pub limit: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobCounts {
    pub by_state: Vec<(String, i64)>,
    pub open_by_class: Vec<(String, i64)>,
    pub deferred_by_reason: Vec<(String, i64)>,
    pub oldest_pending_at: Option<UtcTimestamp>,
    pub open_total: i64,
}

// ---- wallets ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletRow {
    pub wallet_id: Uuid,
    pub address: String,
    pub lifecycle: String,
    pub eligibility: String,
    pub eligibility_reasons: Vec<String>,
    pub qualification: Option<String>,
    pub qualification_reasons: Vec<String>,
    pub priority_score: Option<String>,
    pub priority_policy_version: Option<String>,
    pub history_stage: String,
    pub history_coverage_state: String,
    pub history_coverage_start: Option<UtcTimestamp>,
    pub history_coverage_end: Option<UtcTimestamp>,
    pub monitoring_mode: String,
    pub first_discovered_at: UtcTimestamp,
    pub first_discovery_source: String,
    pub last_discovered_at: UtcTimestamp,
    pub last_verified_trade_at: Option<UtcTimestamp>,
    pub last_observed_at: Option<UtcTimestamp>,
    pub last_investigated_at: Option<UtcTimestamp>,
    pub last_evaluated_at: Option<UtcTimestamp>,
    pub next_investigation_at: Option<UtcTimestamp>,
    pub discovery_evidence_count: i32,
    pub valid_evidence_count: i32,
    pub verified_swap_count: i32,
    pub distinct_token_count: i32,
    pub operator_excluded: bool,
    pub needs_evaluation: bool,
    pub revision: i32,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

const WALLET_COLS: &str = "wallet_id, address, lifecycle, eligibility, eligibility_reasons, qualification,
    qualification_reasons, priority_score::text AS priority_score, priority_policy_version, history_stage,
    history_coverage_state, history_coverage_start, history_coverage_end, monitoring_mode, first_discovered_at,
    first_discovery_source, last_discovered_at, last_verified_trade_at, last_observed_at, last_investigated_at,
    last_evaluated_at, next_investigation_at, discovery_evidence_count, valid_evidence_count, verified_swap_count,
    distinct_token_count, operator_excluded, needs_evaluation, revision, created_at, updated_at";

fn wallet_row(r: &sqlx::postgres::PgRow) -> WalletRow {
    WalletRow {
        wallet_id: r.get("wallet_id"),
        address: r.get("address"),
        lifecycle: r.get("lifecycle"),
        eligibility: r.get("eligibility"),
        eligibility_reasons: r.get("eligibility_reasons"),
        qualification: r.get("qualification"),
        qualification_reasons: r.get("qualification_reasons"),
        priority_score: r.get("priority_score"),
        priority_policy_version: r.get("priority_policy_version"),
        history_stage: r.get("history_stage"),
        history_coverage_state: r.get("history_coverage_state"),
        history_coverage_start: opt_utc(r.get("history_coverage_start")),
        history_coverage_end: opt_utc(r.get("history_coverage_end")),
        monitoring_mode: r.get("monitoring_mode"),
        first_discovered_at: utc(r.get("first_discovered_at")),
        first_discovery_source: r.get("first_discovery_source"),
        last_discovered_at: utc(r.get("last_discovered_at")),
        last_verified_trade_at: opt_utc(r.get("last_verified_trade_at")),
        last_observed_at: opt_utc(r.get("last_observed_at")),
        last_investigated_at: opt_utc(r.get("last_investigated_at")),
        last_evaluated_at: opt_utc(r.get("last_evaluated_at")),
        next_investigation_at: opt_utc(r.get("next_investigation_at")),
        discovery_evidence_count: r.get("discovery_evidence_count"),
        valid_evidence_count: r.get("valid_evidence_count"),
        verified_swap_count: r.get("verified_swap_count"),
        distinct_token_count: r.get("distinct_token_count"),
        operator_excluded: r.get("operator_excluded"),
        needs_evaluation: r.get("needs_evaluation"),
        revision: r.get("revision"),
        created_at: utc(r.get("created_at")),
        updated_at: utc(r.get("updated_at")),
    }
}

/// Registry query. Sorting is restricted to a closed set (no SQL from input).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WalletFilter {
    pub lifecycle: Option<String>,
    pub eligibility: Option<String>,
    pub source: Option<String>,
    pub monitoring_mode: Option<String>,
    pub coverage_state: Option<String>,
    pub qualification: Option<String>,
    pub min_priority: Option<String>,
    pub max_priority: Option<String>,
    /// Only wallets whose last verified trade is at or after this time.
    pub active_since: Option<UtcTimestamp>,
    pub sort: WalletSort,
    /// Opaque keyset cursor (sort key, address) produced by the previous page.
    pub after: Option<(String, String)>,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WalletSort {
    #[default]
    Priority,
    Discovered,
    LastTrade,
}

/// One DATA ENGINE swap as RADAR needs it for extraction and screening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapForRadar {
    pub swap_id: String,
    pub signature: String,
    pub slot: u64,
    pub transaction_index: Option<i32>,
    pub block_time: Option<UtcTimestamp>,
    pub wallet: Option<String>,
    pub attribution: String,
    pub reasons: Vec<String>,
    pub fee_payer: String,
    pub fee_lamports: String,
    pub input_mint: Option<String>,
    pub input_amount: Option<String>,
    pub input_decimals: Option<i16>,
    pub input_asset_kind: String,
    pub output_mint: Option<String>,
    pub output_amount: Option<String>,
    pub output_decimals: Option<i16>,
    pub output_asset_kind: String,
    pub protocol: String,
    pub classification_status: String,
    pub finality: String,
    pub canonical: bool,
    pub revision: i32,
    pub instruction_ref: String,
    /// First route leg's pool (direct swaps name their pool; aggregator
    /// routes may not).
    pub pool: Option<String>,
    pub first_observed_at: UtcTimestamp,
}

const SWAP_COLS: &str = "s.swap_id, s.signature, s.slot, s.transaction_index, s.block_time, s.wallet, s.attribution,
    s.reasons, s.fee_payer, t.fee_lamports::text AS fee_lamports, s.input_mint, s.input_amount::text AS input_amount,
    s.input_decimals, s.input_asset_kind, s.output_mint, s.output_amount::text AS output_amount, s.output_decimals,
    s.output_asset_kind, s.protocol, s.classification_status, s.finality, s.canonical, s.revision, s.instruction_ref,
    (SELECT l.pool FROM solprint.de_route_legs l WHERE l.swap_id = s.swap_id AND l.pool IS NOT NULL
      ORDER BY l.leg_index LIMIT 1) AS pool, s.first_observed_at";

fn swap_row(r: &sqlx::postgres::PgRow) -> SwapForRadar {
    SwapForRadar {
        swap_id: r.get("swap_id"),
        signature: r.get("signature"),
        slot: u64_of(r.get("slot")),
        transaction_index: r.get("transaction_index"),
        block_time: opt_utc(r.get("block_time")),
        wallet: r.get("wallet"),
        attribution: r.get("attribution"),
        reasons: r.get("reasons"),
        fee_payer: r.get("fee_payer"),
        fee_lamports: r.get("fee_lamports"),
        input_mint: r.get("input_mint"),
        input_amount: r.get("input_amount"),
        input_decimals: r.get("input_decimals"),
        input_asset_kind: r.get("input_asset_kind"),
        output_mint: r.get("output_mint"),
        output_amount: r.get("output_amount"),
        output_decimals: r.get("output_decimals"),
        output_asset_kind: r.get("output_asset_kind"),
        protocol: r.get("protocol"),
        classification_status: r.get("classification_status"),
        finality: r.get("finality"),
        canonical: r.get("canonical"),
        revision: r.get("revision"),
        instruction_ref: r.get("instruction_ref"),
        pool: r.get("pool"),
        first_observed_at: utc(r.get("first_observed_at")),
    }
}

/// A wallet's balance of one token at the start and end of one transaction,
/// summed over the token accounts it owns there. `None` means the evidence
/// does not establish it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBalanceRow {
    pub signature: String,
    pub mint: String,
    pub pre: Option<String>,
    pub post: Option<String>,
}

/// Where a signature first came from: the backfill (and RADAR job) that
/// queued it, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureOrigin {
    pub backfill_id: Option<Uuid>,
    pub job_id: Option<Uuid>,
    pub work_class: Option<String>,
    pub subject: Option<String>,
    pub source: String,
}

/// Write for one evidence observation (applied idempotently).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceWrite {
    pub address: String,
    pub swap_id: String,
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<UtcTimestamp>,
    pub token_mint: Option<String>,
    pub quote_mint: Option<String>,
    pub pool: Option<String>,
    pub protocol: String,
    pub economic_role: String,
    pub attribution: String,
    pub finality: String,
    pub discovery_source: String,
    pub job_id: Option<Uuid>,
    pub backfill_id: Option<Uuid>,
    pub swap_revision: i32,
    pub valid: bool,
    pub invalidation_reason: Option<String>,
    pub evidence_available_at: UtcTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRow {
    pub evidence_id: i64,
    pub address: String,
    pub swap_id: String,
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<UtcTimestamp>,
    pub token_mint: Option<String>,
    pub quote_mint: Option<String>,
    pub pool: Option<String>,
    pub protocol: String,
    pub economic_role: String,
    pub attribution: String,
    pub finality: String,
    pub discovery_source: String,
    pub job_id: Option<Uuid>,
    pub backfill_id: Option<Uuid>,
    pub swap_revision: i32,
    pub valid: bool,
    pub invalidation_reason: Option<String>,
    pub invalidated_at: Option<UtcTimestamp>,
    pub evidence_available_at: UtcTimestamp,
    pub discovered_at: UtcTimestamp,
}

const EVIDENCE_COLS: &str = "evidence_id, address, swap_id, signature, slot, block_time, token_mint, quote_mint,
    pool, protocol, economic_role, attribution, finality, discovery_source, job_id, backfill_id, swap_revision,
    valid, invalidation_reason, invalidated_at, evidence_available_at, discovered_at";

fn evidence_row(r: &sqlx::postgres::PgRow) -> EvidenceRow {
    EvidenceRow {
        evidence_id: r.get("evidence_id"),
        address: r.get("address"),
        swap_id: r.get("swap_id"),
        signature: r.get("signature"),
        slot: u64_of(r.get("slot")),
        block_time: opt_utc(r.get("block_time")),
        token_mint: r.get("token_mint"),
        quote_mint: r.get("quote_mint"),
        pool: r.get("pool"),
        protocol: r.get("protocol"),
        economic_role: r.get("economic_role"),
        attribution: r.get("attribution"),
        finality: r.get("finality"),
        discovery_source: r.get("discovery_source"),
        job_id: r.get("job_id"),
        backfill_id: r.get("backfill_id"),
        swap_revision: r.get("swap_revision"),
        valid: r.get("valid"),
        invalidation_reason: r.get("invalidation_reason"),
        invalidated_at: opt_utc(r.get("invalidated_at")),
        evidence_available_at: utc(r.get("evidence_available_at")),
        discovered_at: utc(r.get("discovered_at")),
    }
}

/// RADAR domain event (same contract as the DATA ENGINE outbox).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadarEventWrite {
    pub event_id: Uuid,
    pub event_type: String,
    pub schema_version: i16,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_revision: i32,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub source: String,
    pub observed_at: UtcTimestamp,
    pub payload_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadarEventRow {
    pub event_id: Uuid,
    pub seq: i64,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_revision: i32,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub source: String,
    pub observed_at: UtcTimestamp,
    pub persisted_at: UtcTimestamp,
    pub payload_json: String,
}

/// Outcome of applying one DATA ENGINE event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceOutcome {
    /// A new wallet entered the registry.
    NewWallet,
    /// New evidence for a known wallet.
    NewEvidence,
    /// Existing evidence changed (revision, validity, finality).
    Updated,
    /// Already applied (same or older revision) or not applicable.
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionWrite {
    pub from_state: Option<String>,
    pub to_state: String,
    pub trigger: String,
    pub reasons: Vec<String>,
    pub metrics_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionRow {
    pub transition_id: i64,
    pub from_state: Option<String>,
    pub to_state: String,
    pub trigger: String,
    pub reasons: Vec<String>,
    pub metrics_json: String,
    pub policy_version: String,
    pub transitioned_at: UtcTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRow {
    pub snapshot_id: i64,
    pub policy_version: String,
    pub input_hash: String,
    pub priority_score: String,
    pub qualification: String,
    pub metrics_json: String,
    pub computed_at: UtcTimestamp,
}

/// Everything one evaluation writes, applied atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationWrite {
    pub address: String,
    /// The wallet revision the evaluation read; a concurrent change aborts it.
    pub expected_revision: i32,
    pub lifecycle: String,
    pub eligibility: String,
    pub eligibility_reasons: Vec<String>,
    pub qualification: Option<String>,
    pub qualification_reasons: Vec<String>,
    pub priority_score: String,
    pub policy_version: String,
    pub history_stage: String,
    pub history_coverage_state: String,
    pub history_coverage_start: Option<UtcTimestamp>,
    pub history_coverage_end: Option<UtcTimestamp>,
    pub last_verified_trade_at: Option<UtcTimestamp>,
    pub next_investigation_at: Option<UtcTimestamp>,
    pub valid_evidence_count: i32,
    pub verified_swap_count: i32,
    pub distinct_token_count: i32,
    pub investigated: bool,
    pub transitions: Vec<TransitionWrite>,
    pub input_hash: String,
    pub metrics_json: String,
    pub events: Vec<RadarEventWrite>,
}

/// A maintenance lifecycle change (dormancy, archival), guarded by `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleChange {
    pub address: String,
    pub from: String,
    pub to: String,
    pub trigger: String,
    pub reasons: Vec<String>,
    pub monitoring_mode: Option<String>,
    pub event: Option<RadarEventWrite>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitoringRow {
    pub assignment_id: i64,
    pub address: String,
    pub mode: String,
    pub target_id: Option<Uuid>,
    pub priority_at_start: Option<String>,
    pub started_at: UtcTimestamp,
    pub ended_at: Option<UtcTimestamp>,
    pub end_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RadarCounts {
    pub wallets_total: i64,
    pub wallets_by_lifecycle: Vec<(String, i64)>,
    pub wallets_by_eligibility: Vec<(String, i64)>,
    pub wallets_new_24h: i64,
    pub evidence_total: i64,
    pub evidence_invalidated: i64,
    pub markets_total: i64,
    pub markets_by_state: Vec<(String, i64)>,
    pub pools_total: i64,
    pub pools_supported: i64,
    pub monitoring_active: Vec<(String, i64)>,
    pub reactivations: i64,
    pub last_cycle_at: Option<UtcTimestamp>,
    pub last_completed_cycle_at: Option<UtcTimestamp>,
    pub cycles_24h: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RadarRetentionOutcome {
    pub observations_deleted: u64,
    pub snapshots_deleted: u64,
    pub jobs_deleted: u64,
    pub events_deleted: u64,
    pub cycles_deleted: u64,
}

async fn insert_event(
    tx: &mut Transaction<'_, Postgres>,
    ev: &RadarEventWrite,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO solprint.radar_outbox
            (event_id, event_type, schema_version, aggregate_type, aggregate_id, aggregate_revision, network,
             correlation_id, causation_id, source, observed_at, payload)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::jsonb)
         ON CONFLICT (aggregate_type, aggregate_id, aggregate_revision, event_type) DO NOTHING",
    )
    .bind(ev.event_id)
    .bind(&ev.event_type)
    .bind(ev.schema_version)
    .bind(&ev.aggregate_type)
    .bind(&ev.aggregate_id)
    .bind(ev.aggregate_revision)
    .bind(NETWORK)
    .bind(ev.correlation_id)
    .bind(ev.causation_id)
    .bind(&ev.source)
    .bind(ts(&ev.observed_at))
    .bind(&ev.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|x| e(&x))?;
    Ok(())
}

async fn mark_delivered(
    tx: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO solprint.de_outbox_deliveries (consumer, event_id, status, delivered_at)
         VALUES ($1, $2, 'DELIVERED', now())
         ON CONFLICT (consumer, event_id) DO UPDATE SET status = 'DELIVERED',
            delivered_at = COALESCE(solprint.de_outbox_deliveries.delivered_at, now()),
            next_attempt_at = NULL, updated_at = now()",
    )
    .bind(RADAR_CONSUMER)
    .bind(event_id)
    .execute(&mut **tx)
    .await
    .map_err(|x| e(&x))?;
    Ok(())
}

/// Recount a wallet's evidence summary and mark it for evaluation.
async fn refresh_wallet_counts(
    tx: &mut Transaction<'_, Postgres>,
    address: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        "UPDATE solprint.radar_wallets w SET
            discovery_evidence_count = c.total, valid_evidence_count = c.valid,
            last_observed_at = GREATEST(w.last_observed_at, c.newest),
            needs_evaluation = TRUE, revision = w.revision + 1, updated_at = now()
           FROM (SELECT count(*)::int AS total, count(*) FILTER (WHERE valid)::int AS valid,
                        max(block_time) FILTER (WHERE valid) AS newest
                   FROM solprint.radar_wallet_evidence WHERE network = $1 AND address = $2) c
          WHERE w.network = $1 AND w.address = $2",
    )
    .bind(NETWORK)
    .bind(address)
    .execute(&mut **tx)
    .await
    .map_err(|x| e(&x))?;
    Ok(())
}

impl Database {
    // ---- control -----------------------------------------------------------------------------

    pub async fn radar_control(&self) -> Result<RadarControl, StorageError> {
        let row = sqlx::query(
            "INSERT INTO solprint.radar_control (network) VALUES ($1)
             ON CONFLICT (network) DO UPDATE SET network = EXCLUDED.network
             RETURNING paused, pause_reason, cycle_requested, last_cycle_at",
        )
        .bind(NETWORK)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(RadarControl {
            paused: row.get("paused"),
            pause_reason: row.get("pause_reason"),
            cycle_requested: row.get("cycle_requested"),
            last_cycle_at: opt_utc(row.get("last_cycle_at")),
        })
    }

    pub async fn radar_set_paused(
        &self,
        paused: bool,
        reason: Option<&str>,
    ) -> Result<(), StorageError> {
        self.radar_control().await?;
        sqlx::query(
            "UPDATE solprint.radar_control SET paused = $2, pause_reason = $3, updated_at = now() WHERE network = $1",
        )
        .bind(NETWORK)
        .bind(paused)
        .bind(reason)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_request_cycle(&self, requested: bool) -> Result<(), StorageError> {
        self.radar_control().await?;
        sqlx::query(
            "UPDATE solprint.radar_control SET cycle_requested = $2, updated_at = now() WHERE network = $1",
        )
        .bind(NETWORK)
        .bind(requested)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_record_operator_action(
        &self,
        action: &str,
        subject: Option<&str>,
        outcome: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO solprint.radar_operator_actions (network, action, subject, outcome) VALUES ($1, $2, $3, $4)",
        )
        .bind(NETWORK)
        .bind(action)
        .bind(subject)
        .bind(outcome)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    // ---- cycles ------------------------------------------------------------------------------

    pub async fn radar_start_cycle(&self, c: &NewCycle) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO solprint.radar_cycles (cycle_id, network, trigger, status, seed, budget_state)
             VALUES ($1, $2, $3, 'RUNNING', $4, $5)",
        )
        .bind(c.cycle_id)
        .bind(NETWORK)
        .bind(&c.trigger)
        .bind(c.seed)
        .bind(&c.budget_state)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_finish_cycle(&self, id: Uuid, f: &CycleFinish) -> Result<(), StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        sqlx::query(
            "UPDATE solprint.radar_cycles SET status = $2, deferred_reason = $3, observations = $4, markets_new = $5,
                    markets_selected = $6, samples_scheduled = $7, credits_estimated = $8, finished_at = now()
              WHERE cycle_id = $1",
        )
        .bind(id)
        .bind(&f.status)
        .bind(&f.deferred_reason)
        .bind(f.observations)
        .bind(f.markets_new)
        .bind(f.markets_selected)
        .bind(f.samples_scheduled)
        .bind(f.credits_estimated)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        sqlx::query(
            "INSERT INTO solprint.radar_control (network, last_cycle_at, cycle_requested) VALUES ($1, now(), FALSE)
             ON CONFLICT (network) DO UPDATE SET last_cycle_at = now(), cycle_requested = FALSE, updated_at = now()",
        )
        .bind(NETWORK)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_cycles(&self, limit: i64) -> Result<Vec<CycleRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT cycle_id, trigger, status, seed, budget_state, deferred_reason, observations, markets_new,
                    markets_selected, samples_scheduled, credits_estimated, started_at, finished_at
               FROM solprint.radar_cycles WHERE network = $1 ORDER BY started_at DESC LIMIT $2",
        )
        .bind(NETWORK)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| CycleRow {
                cycle_id: r.get("cycle_id"),
                trigger: r.get("trigger"),
                status: r.get("status"),
                seed: r.get("seed"),
                budget_state: r.get("budget_state"),
                deferred_reason: r.get("deferred_reason"),
                observations: r.get("observations"),
                markets_new: r.get("markets_new"),
                markets_selected: r.get("markets_selected"),
                samples_scheduled: r.get("samples_scheduled"),
                credits_estimated: r.get("credits_estimated"),
                started_at: utc(r.get("started_at")),
                finished_at: opt_utc(r.get("finished_at")),
            })
            .collect())
    }

    pub async fn radar_record_source_run(&self, s: &SourceRunWrite) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO solprint.radar_source_runs
                (run_id, cycle_id, source, status, error_code, items, markets_new, provider_requests,
                 credits_estimated, started_at, finished_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(s.run_id)
        .bind(s.cycle_id)
        .bind(&s.source)
        .bind(&s.status)
        .bind(&s.error_code)
        .bind(s.items)
        .bind(s.markets_new)
        .bind(s.provider_requests)
        .bind(s.credits_estimated)
        .bind(ts(&s.started_at))
        .bind(ts(&s.finished_at))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_source_metrics(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<SourceMetricsRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT r.source, count(*) AS runs,
                    count(*) FILTER (WHERE r.status IN ('OK', 'EMPTY')) AS runs_ok,
                    count(*) FILTER (WHERE r.status IN ('BUDGET_DEFERRED', 'PROVIDER_COOLDOWN', 'DISABLED')) AS runs_deferred,
                    count(*) FILTER (WHERE r.status = 'FAILED') AS runs_failed,
                    COALESCE(sum(r.items), 0)::bigint AS items, COALESCE(sum(r.markets_new), 0)::bigint AS markets_new,
                    COALESCE(sum(r.provider_requests), 0)::bigint AS provider_requests,
                    COALESCE(sum(r.credits_estimated), 0)::bigint AS credits_estimated,
                    max(r.finished_at) AS last_run_at,
                    (SELECT x.status FROM solprint.radar_source_runs x WHERE x.source = r.source
                      ORDER BY x.finished_at DESC LIMIT 1) AS last_status
               FROM solprint.radar_source_runs r
              WHERE r.started_at >= $1
              GROUP BY r.source ORDER BY r.source",
        )
        .bind(ts(since))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| SourceMetricsRow {
                source: r.get("source"),
                runs: r.get("runs"),
                runs_ok: r.get("runs_ok"),
                runs_deferred: r.get("runs_deferred"),
                runs_failed: r.get("runs_failed"),
                items: r.get("items"),
                markets_new: r.get("markets_new"),
                provider_requests: r.get("provider_requests"),
                credits_estimated: r.get("credits_estimated"),
                last_run_at: opt_utc(r.get("last_run_at")),
                last_status: r.get("last_status"),
            })
            .collect())
    }

    /// Wallet yield per discovery source since `since` (by evidence time).
    pub async fn radar_source_yield(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<SourceYieldRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT ev.discovery_source AS source, count(*) AS evidence,
                    count(DISTINCT ev.address) AS unique_wallets,
                    count(DISTINCT ev.address) FILTER (WHERE w.eligibility IN ('ELIGIBLE_CANDIDATE', 'PROVISIONAL_CANDIDATE')) AS eligible_wallets,
                    count(DISTINCT ev.address) FILTER (WHERE w.history_stage <> 'NONE') AS investigated_wallets,
                    count(DISTINCT ev.address) FILTER (WHERE w.lifecycle IN ('PRELIMINARY_QUALIFIED', 'PRIORITIZED', 'MONITORED')) AS qualified_wallets
               FROM solprint.radar_wallet_evidence ev
               JOIN solprint.radar_wallets w ON w.network = ev.network AND w.address = ev.address
              WHERE ev.network = $1 AND ev.discovered_at >= $2
              GROUP BY ev.discovery_source ORDER BY ev.discovery_source",
        )
        .bind(NETWORK)
        .bind(ts(since))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| SourceYieldRow {
                source: r.get("source"),
                evidence: r.get("evidence"),
                unique_wallets: r.get("unique_wallets"),
                eligible_wallets: r.get("eligible_wallets"),
                investigated_wallets: r.get("investigated_wallets"),
                qualified_wallets: r.get("qualified_wallets"),
            })
            .collect())
    }

    // ---- markets and pools -------------------------------------------------------------------

    /// Insert or refresh a market. Returns `true` when the mint is new to the
    /// universe. Sources accumulate; a promotional observation never
    /// overrides an organic bucket.
    pub async fn radar_upsert_market(&self, m: &MarketUpsert) -> Result<bool, StorageError> {
        let row = sqlx::query(
            "INSERT INTO solprint.radar_markets
                (network, mint, token_program, display_symbol, display_name, classification, classification_reasons,
                 classification_ruleset, bucket, sources, promotional_only, first_discovered_at, last_market_observation_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, ARRAY[$10]::text[], $11, $12, $12)
             ON CONFLICT (network, mint) DO UPDATE SET
                token_program = COALESCE(EXCLUDED.token_program, solprint.radar_markets.token_program),
                display_symbol = COALESCE(EXCLUDED.display_symbol, solprint.radar_markets.display_symbol),
                display_name = COALESCE(EXCLUDED.display_name, solprint.radar_markets.display_name),
                classification = EXCLUDED.classification,
                classification_reasons = EXCLUDED.classification_reasons,
                classification_ruleset = EXCLUDED.classification_ruleset,
                bucket = CASE WHEN $11 THEN solprint.radar_markets.bucket ELSE EXCLUDED.bucket END,
                sources = CASE WHEN $10 = ANY(solprint.radar_markets.sources) THEN solprint.radar_markets.sources
                               ELSE solprint.radar_markets.sources || ARRAY[$10]::text[] END,
                promotional_only = solprint.radar_markets.promotional_only AND $11,
                last_market_observation_at = GREATEST(solprint.radar_markets.last_market_observation_at, $12),
                updated_at = now()
             RETURNING (xmax = 0) AS inserted",
        )
        .bind(NETWORK)
        .bind(&m.mint)
        .bind(&m.token_program)
        .bind(&m.display_symbol)
        .bind(&m.display_name)
        .bind(&m.classification)
        .bind(&m.classification_reasons)
        .bind(&m.ruleset)
        .bind(&m.bucket)
        .bind(&m.source)
        .bind(m.promotional)
        .bind(ts(&m.observed_at))
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(row.get("inserted"))
    }

    pub async fn radar_market_count(&self) -> Result<i64, StorageError> {
        sqlx::query_scalar("SELECT count(*) FROM solprint.radar_markets WHERE network = $1")
            .bind(NETWORK)
            .fetch_one(self.pool())
            .await
            .map_err(|x| e(&x))
    }

    pub async fn radar_market(&self, mint: &str) -> Result<Option<MarketRow>, StorageError> {
        let row = sqlx::query(const_sql!(
            "SELECT {MARKET_COLS} FROM solprint.radar_markets WHERE network = $1 AND mint = $2"
        ))
        .bind(NETWORK)
        .bind(mint)
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(row.as_ref().map(market_row))
    }

    pub async fn radar_markets(&self, f: &MarketFilter) -> Result<Vec<MarketRow>, StorageError> {
        let (bt, bm) = f
            .before
            .as_ref()
            .map_or((None, None), |(t, m)| (Some(ts(t)), Some(m.clone())));
        let rows = sqlx::query(const_sql!(
            "SELECT {MARKET_COLS} FROM solprint.radar_markets
              WHERE network = $1
                AND ($2::text IS NULL OR selection_state = $2)
                AND ($3::text IS NULL OR bucket = $3)
                AND ($4::text IS NULL OR classification = $4)
                AND ($5::timestamptz IS NULL OR (first_discovered_at, mint) < ($5, $6))
              ORDER BY first_discovered_at DESC, mint DESC LIMIT $7"
        ))
        .bind(NETWORK)
        .bind(&f.selection_state)
        .bind(&f.bucket)
        .bind(&f.classification)
        .bind(bt)
        .bind(bm)
        .bind(f.limit.clamp(1, 500))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(market_row).collect())
    }

    /// Every enabled, non-excluded market (bounded by the universe ceiling).
    pub async fn radar_markets_for_selection(
        &self,
        limit: i64,
    ) -> Result<Vec<MarketRow>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "SELECT {MARKET_COLS} FROM solprint.radar_markets
              WHERE network = $1 AND enabled AND selection_state <> 'EXCLUDED'
              ORDER BY last_market_observation_at DESC NULLS LAST, mint LIMIT $2"
        ))
        .bind(NETWORK)
        .bind(limit.clamp(1, 10_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(market_row).collect())
    }

    pub async fn radar_set_market_selection(
        &self,
        s: &MarketSelection,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE solprint.radar_markets SET selection_state = $3, selection_reasons = $4, activity_state = $5,
                    market_priority = $6::numeric, coverage_state = COALESCE($7, coverage_state), updated_at = now()
              WHERE network = $1 AND mint = $2",
        )
        .bind(NETWORK)
        .bind(&s.mint)
        .bind(&s.selection_state)
        .bind(&s.selection_reasons)
        .bind(&s.activity_state)
        .bind(&s.market_priority)
        .bind(&s.coverage_state)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_upsert_pool(&self, p: &PoolUpsert) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO solprint.radar_pools
                (network, pool, mint, quote_mint, dex_id, labels, liquidity_usd, volume_24h_usd, txns_1h, txns_24h,
                 pair_created_at, last_observed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7::numeric, $8::numeric, $9, $10, $11, $12)
             ON CONFLICT (network, pool) DO UPDATE SET
                quote_mint = COALESCE(EXCLUDED.quote_mint, solprint.radar_pools.quote_mint),
                dex_id = COALESCE(EXCLUDED.dex_id, solprint.radar_pools.dex_id),
                labels = EXCLUDED.labels,
                liquidity_usd = EXCLUDED.liquidity_usd, volume_24h_usd = EXCLUDED.volume_24h_usd,
                txns_1h = EXCLUDED.txns_1h, txns_24h = EXCLUDED.txns_24h,
                pair_created_at = COALESCE(EXCLUDED.pair_created_at, solprint.radar_pools.pair_created_at),
                last_observed_at = GREATEST(solprint.radar_pools.last_observed_at, EXCLUDED.last_observed_at)
             WHERE solprint.radar_pools.mint = EXCLUDED.mint",
        )
        .bind(NETWORK)
        .bind(&p.pool)
        .bind(&p.mint)
        .bind(&p.quote_mint)
        .bind(&p.dex_id)
        .bind(&p.labels)
        .bind(&p.liquidity_usd)
        .bind(&p.volume_24h_usd)
        .bind(p.txns_1h)
        .bind(p.txns_24h)
        .bind(p.pair_created_at.as_ref().map(ts))
        .bind(ts(&p.observed_at))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_pools_for_mints(
        &self,
        mints: &[String],
    ) -> Result<Vec<PoolRow>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "SELECT {POOL_COLS} FROM solprint.radar_pools WHERE network = $1 AND mint = ANY($2)
              ORDER BY mint, liquidity_usd DESC NULLS LAST, pool"
        ))
        .bind(NETWORK)
        .bind(mints)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(pool_row).collect())
    }

    pub async fn radar_pool(&self, pool: &str) -> Result<Option<PoolRow>, StorageError> {
        let row = sqlx::query(const_sql!(
            "SELECT {POOL_COLS} FROM solprint.radar_pools WHERE network = $1 AND pool = $2"
        ))
        .bind(NETWORK)
        .bind(pool)
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(row.as_ref().map(pool_row))
    }

    /// Whether an address is a pool RADAR knows (pool accounts are
    /// infrastructure, never traders).
    pub async fn radar_is_known_pool(&self, address: &str) -> Result<bool, StorageError> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM solprint.radar_pools WHERE network = $1 AND pool = $2)",
        )
        .bind(NETWORK)
        .bind(address)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    /// Whether the DATA ENGINE has seen this address as a token account owned
    /// by someone else (a token account is never a trader wallet).
    pub async fn radar_is_foreign_token_account(
        &self,
        address: &str,
    ) -> Result<bool, StorageError> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM solprint.de_balance_changes
                             WHERE account = $1 AND asset <> 'SOL' AND owner IS NOT NULL AND owner <> $1)",
        )
        .bind(address)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    pub async fn radar_set_pool_validation(
        &self,
        pool: &str,
        validation: &str,
        program_id: Option<&str>,
        protocol: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE solprint.radar_pools SET validation = $3, program_id = $4, protocol = $5, validated_at = now()
              WHERE network = $1 AND pool = $2",
        )
        .bind(NETWORK)
        .bind(pool)
        .bind(validation)
        .bind(program_id)
        .bind(protocol)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_insert_observations(
        &self,
        obs: &[ObservationWrite],
    ) -> Result<(), StorageError> {
        if obs.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        for o in obs {
            sqlx::query(
                "INSERT INTO solprint.radar_market_observations
                    (network, cycle_id, mint, pool, source, promotional, source_rank, liquidity_usd, volume_24h_usd,
                     txns_1h, txns_24h, pair_created_at, organic_score, decision, decision_reasons, observed_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8::numeric, $9::numeric, $10, $11, $12, $13::numeric, $14, $15, $16)",
            )
            .bind(NETWORK)
            .bind(o.cycle_id)
            .bind(&o.mint)
            .bind(&o.pool)
            .bind(&o.source)
            .bind(o.promotional)
            .bind(o.source_rank)
            .bind(&o.liquidity_usd)
            .bind(&o.volume_24h_usd)
            .bind(o.txns_1h)
            .bind(o.txns_24h)
            .bind(o.pair_created_at.as_ref().map(ts))
            .bind(&o.organic_score)
            .bind(&o.decision)
            .bind(&o.decision_reasons)
            .bind(ts(&o.observed_at))
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(())
    }

    /// Mints of recent verified swaps already in the DATA ENGINE (local, no
    /// provider): the "existing activity" discovery source. Quote assets are
    /// excluded by the caller.
    pub async fn radar_local_activity_mints(
        &self,
        since: &UtcTimestamp,
        limit: i64,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        let rows = sqlx::query(
            "SELECT mint, count(*) AS swaps FROM (
                SELECT input_mint AS mint FROM solprint.de_swaps
                 WHERE canonical AND classification_status = 'VERIFIED' AND decoded_at >= $1 AND input_mint IS NOT NULL
                UNION ALL
                SELECT output_mint FROM solprint.de_swaps
                 WHERE canonical AND classification_status = 'VERIFIED' AND decoded_at >= $1 AND output_mint IS NOT NULL
             ) m GROUP BY mint ORDER BY swaps DESC, mint LIMIT $2",
        )
        .bind(ts(since))
        .bind(limit.clamp(1, 1_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| (r.get("mint"), r.get("swaps")))
            .collect())
    }

    /// Pools named by local swaps of a mint (for markets known only from
    /// local activity).
    pub async fn radar_local_pools_for_mint(
        &self,
        mint: &str,
        limit: i64,
    ) -> Result<Vec<(String, String, i64)>, StorageError> {
        let rows = sqlx::query(
            "SELECT l.pool, l.protocol, count(*) AS swaps FROM solprint.de_route_legs l
               JOIN solprint.de_swaps s ON s.swap_id = l.swap_id
              WHERE s.canonical AND l.pool IS NOT NULL AND (l.input_mint = $1 OR l.output_mint = $1)
              GROUP BY l.pool, l.protocol ORDER BY swaps DESC, l.pool LIMIT $2",
        )
        .bind(mint)
        .bind(limit.clamp(1, 100))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| (r.get("pool"), r.get("protocol"), r.get("swaps")))
            .collect())
    }

    pub async fn radar_mark_market_sampled(
        &self,
        mint: &str,
        pool: &str,
        until_signature: Option<&str>,
        wallets_found: i32,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        sqlx::query(
            "UPDATE solprint.radar_pools SET last_sampled_at = now(), samples_completed = samples_completed + 1,
                    sample_until_signature = COALESCE($3, sample_until_signature)
              WHERE network = $1 AND pool = $2",
        )
        .bind(NETWORK)
        .bind(pool)
        .bind(until_signature)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        sqlx::query(
            "UPDATE solprint.radar_markets SET last_sampled_at = now(), samples_completed = samples_completed + 1,
                    wallets_found = wallets_found + $3, coverage_state = 'SAMPLED', updated_at = now()
              WHERE network = $1 AND mint = $2",
        )
        .bind(NETWORK)
        .bind(mint)
        .bind(wallets_found.max(0))
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(())
    }

    /// Distinct wallets with valid evidence from a pool's swaps that were
    /// queued by one backfill (market sample yield).
    pub async fn radar_backfill_wallet_yield(
        &self,
        backfill_id: Uuid,
    ) -> Result<i64, StorageError> {
        sqlx::query_scalar(
            "SELECT count(DISTINCT address) FROM solprint.radar_wallet_evidence
              WHERE backfill_id = $1 AND valid",
        )
        .bind(backfill_id)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    // ---- jobs --------------------------------------------------------------------------------

    /// Create a job; `None` when an open job for the same class and subject
    /// already exists (duplicate investigations are refused, not queued).
    pub async fn radar_create_job(&self, j: &NewJob) -> Result<Option<Uuid>, StorageError> {
        let row = sqlx::query(
            "INSERT INTO solprint.radar_jobs
                (job_id, network, work_class, subject_kind, subject, origin, bucket, cycle_id, priority, state,
                 signature_limit, lookback_days, start_before_signature, until_signature, min_slot, commitment,
                 estimated_credits, next_run_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'PENDING', $10, $11, $12, $13, $14, $15, $16, COALESCE($17, now()))
             ON CONFLICT (network, work_class, subject) WHERE state IN ('PENDING', 'RUNNING', 'WAITING_DATA', 'DEFERRED')
             DO NOTHING RETURNING job_id",
        )
        .bind(j.job_id)
        .bind(NETWORK)
        .bind(&j.work_class)
        .bind(&j.subject_kind)
        .bind(&j.subject)
        .bind(&j.origin)
        .bind(&j.bucket)
        .bind(j.cycle_id)
        .bind(j.priority.clamp(0, 1_000))
        .bind(j.signature_limit)
        .bind(j.lookback_days)
        .bind(&j.start_before_signature)
        .bind(&j.until_signature)
        .bind(j.min_slot.map(i64_of))
        .bind(&j.commitment)
        .bind(j.estimated_credits.max(0))
        .bind(j.next_run_at.as_ref().map(ts))
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(row.map(|r| r.get("job_id")))
    }

    pub async fn radar_job(&self, id: Uuid) -> Result<Option<JobRow>, StorageError> {
        let row = sqlx::query(const_sql!(
            "SELECT {JOB_COLS} FROM solprint.radar_jobs j
               LEFT JOIN solprint.de_backfills b ON b.backfill_id = j.backfill_id
              WHERE j.job_id = $1"
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(row.as_ref().map(job_row))
    }

    pub async fn radar_jobs(&self, f: &JobFilter) -> Result<Vec<JobRow>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "SELECT {JOB_COLS} FROM solprint.radar_jobs j
               LEFT JOIN solprint.de_backfills b ON b.backfill_id = j.backfill_id
              WHERE j.network = $1
                AND ($2::text IS NULL OR j.state = $2)
                AND ($3::text IS NULL OR j.work_class = $3)
                AND ($4::text IS NULL OR j.subject = $4)
              ORDER BY (j.state IN ('PENDING', 'RUNNING', 'WAITING_DATA', 'DEFERRED')) DESC,
                       j.priority DESC, j.created_at DESC
              LIMIT $5"
        ))
        .bind(NETWORK)
        .bind(&f.state)
        .bind(&f.work_class)
        .bind(&f.subject)
        .bind(f.limit.clamp(1, 500))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(job_row).collect())
    }

    /// Lease up to `limit` runnable jobs of the given classes, highest
    /// priority first, at most `max_running` running per call's class set.
    pub async fn radar_claim_jobs(
        &self,
        owner: Uuid,
        classes: &[String],
        max_running: i64,
        lease: Duration,
        limit: i64,
    ) -> Result<Vec<JobRow>, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let running: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM solprint.radar_jobs WHERE network = $1 AND work_class = ANY($2)
                AND state IN ('RUNNING', 'WAITING_DATA')",
        )
        .bind(NETWORK)
        .bind(classes)
        .fetch_one(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        let take = (max_running - running).min(limit);
        if take <= 0 {
            tx.commit().await.map_err(|x| e(&x))?;
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE solprint.radar_jobs SET state = 'RUNNING', lease_owner = $3,
                    lease_expires_at = now() + make_interval(secs => $4), attempts = attempts + 1,
                    started_at = COALESCE(started_at, now()), defer_reason = NULL, updated_at = now()
              WHERE job_id IN (SELECT job_id FROM solprint.radar_jobs
                                WHERE network = $1 AND work_class = ANY($2) AND state IN ('PENDING', 'DEFERRED')
                                  AND next_run_at <= now() AND NOT cancel_requested
                                ORDER BY priority DESC, created_at LIMIT $5 FOR UPDATE SKIP LOCKED)
              RETURNING job_id",
        )
        .bind(NETWORK)
        .bind(classes)
        .bind(owner)
        .bind(secs(lease))
        .bind(take)
        .fetch_all(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        tx.commit().await.map_err(|x| e(&x))?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(j) = self.radar_job(id).await? {
                out.push(j);
            }
        }
        Ok(out)
    }

    /// Hand a leased job to the DATA ENGINE: it now waits for its backfill.
    pub async fn radar_job_waiting(
        &self,
        id: Uuid,
        owner: Uuid,
        backfill_id: Uuid,
        signatures_reused: i32,
    ) -> Result<bool, StorageError> {
        let r = sqlx::query(
            "UPDATE solprint.radar_jobs SET state = 'WAITING_DATA', backfill_id = $3, signatures_reused = $4,
                    lease_owner = NULL, lease_expires_at = NULL, updated_at = now()
              WHERE job_id = $1 AND lease_owner = $2 AND state = 'RUNNING'",
        )
        .bind(id)
        .bind(owner)
        .bind(backfill_id)
        .bind(signatures_reused)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(r.rows_affected() == 1)
    }

    /// Defer a job (budget, capacity, cooldown) without consuming it.
    pub async fn radar_defer_job(
        &self,
        id: Uuid,
        reason: &str,
        retry_in: Duration,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE solprint.radar_jobs SET state = 'DEFERRED', defer_reason = $2, lease_owner = NULL,
                    lease_expires_at = NULL, attempts = GREATEST(attempts - 1, 0),
                    next_run_at = now() + make_interval(secs => $3), updated_at = now()
              WHERE job_id = $1 AND state IN ('PENDING', 'RUNNING', 'DEFERRED')",
        )
        .bind(id)
        .bind(reason)
        .bind(secs(retry_in))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    pub async fn radar_finish_job(
        &self,
        id: Uuid,
        state: &str,
        result_code: &str,
        credits_spent: i32,
        signatures_found: i32,
    ) -> Result<bool, StorageError> {
        let r = sqlx::query(
            "UPDATE solprint.radar_jobs SET state = $2, result_code = $3, credits_spent = $4, signatures_found = $5,
                    lease_owner = NULL, lease_expires_at = NULL, completed_at = now(), updated_at = now()
              WHERE job_id = $1 AND state IN ('PENDING', 'RUNNING', 'WAITING_DATA', 'DEFERRED')",
        )
        .bind(id)
        .bind(state)
        .bind(result_code)
        .bind(credits_spent.max(0))
        .bind(signatures_found.max(0))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(r.rows_affected() == 1)
    }

    /// Request cancellation. Pending/deferred jobs end now; a waiting job's
    /// backfill is asked to stop and the job ends when it does. Evidence
    /// already collected is kept.
    pub async fn radar_cancel_job(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let r = sqlx::query(
            "UPDATE solprint.radar_jobs SET cancel_requested = TRUE,
                    state = CASE WHEN state IN ('PENDING', 'DEFERRED') THEN 'CANCELLED' ELSE state END,
                    result_code = CASE WHEN state IN ('PENDING', 'DEFERRED') THEN 'CANCELLED_BY_OPERATOR' ELSE result_code END,
                    completed_at = CASE WHEN state IN ('PENDING', 'DEFERRED') THEN now() ELSE completed_at END,
                    updated_at = now()
              WHERE job_id = $1 AND state IN ('PENDING', 'RUNNING', 'WAITING_DATA', 'DEFERRED')
              RETURNING backfill_id",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        let Some(r) = r else {
            tx.commit().await.map_err(|x| e(&x))?;
            return Ok(false);
        };
        let bf: Option<Uuid> = r.get("backfill_id");
        if let Some(bf) = bf {
            sqlx::query(
                "UPDATE solprint.de_backfills SET cancel_requested = TRUE,
                        status = CASE WHEN status IN ('PENDING', 'PAUSED', 'PAUSED_BY_BUDGET', 'PAUSED_BY_PROVIDER', 'PAUSED_BY_QUEUE') THEN 'CANCELLED' ELSE status END,
                        completed_at = CASE WHEN status IN ('PENDING', 'PAUSED', 'PAUSED_BY_BUDGET', 'PAUSED_BY_PROVIDER', 'PAUSED_BY_QUEUE') THEN now() ELSE completed_at END,
                        updated_at = now()
                  WHERE backfill_id = $1 AND status NOT IN ('COMPLETED', 'COMPLETED_WITH_GAPS', 'FAILED', 'CANCELLED')",
            )
            .bind(bf)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(true)
    }

    /// Jobs waiting on the DATA ENGINE.
    pub async fn radar_waiting_jobs(&self, limit: i64) -> Result<Vec<JobRow>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "SELECT {JOB_COLS} FROM solprint.radar_jobs j
               LEFT JOIN solprint.de_backfills b ON b.backfill_id = j.backfill_id
              WHERE j.network = $1 AND j.state = 'WAITING_DATA' ORDER BY j.updated_at LIMIT $2"
        ))
        .bind(NETWORK)
        .bind(limit.clamp(1, 1_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(job_row).collect())
    }

    /// Return expired leases to the queue (crash recovery).
    pub async fn radar_recover_leases(&self) -> Result<u64, StorageError> {
        Ok(sqlx::query(
            "UPDATE solprint.radar_jobs SET state = 'PENDING', lease_owner = NULL, lease_expires_at = NULL, updated_at = now()
              WHERE state = 'RUNNING' AND lease_expires_at < now()",
        )
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?
        .rows_affected())
    }

    /// Release this owner's leases (shutdown): the jobs go back to pending.
    pub async fn radar_release_owner(&self, owner: Uuid) -> Result<u64, StorageError> {
        Ok(sqlx::query(
            "UPDATE solprint.radar_jobs SET state = 'PENDING', lease_owner = NULL, lease_expires_at = NULL,
                    attempts = GREATEST(attempts - 1, 0), updated_at = now()
              WHERE state = 'RUNNING' AND lease_owner = $1",
        )
        .bind(owner)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?
        .rows_affected())
    }

    pub async fn radar_job_counts(&self) -> Result<JobCounts, StorageError> {
        let by_state = sqlx::query(
            "SELECT state, count(*) AS n FROM solprint.radar_jobs WHERE network = $1 GROUP BY state ORDER BY state",
        )
        .bind(NETWORK)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?
        .iter()
        .map(|r| (r.get("state"), r.get("n")))
        .collect();
        let open_by_class: Vec<(String, i64)> = sqlx::query(
            "SELECT work_class, count(*) AS n FROM solprint.radar_jobs
              WHERE network = $1 AND state IN ('PENDING', 'RUNNING', 'WAITING_DATA', 'DEFERRED')
              GROUP BY work_class ORDER BY work_class",
        )
        .bind(NETWORK)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?
        .iter()
        .map(|r| (r.get("work_class"), r.get("n")))
        .collect();
        let deferred_by_reason = sqlx::query(
            "SELECT COALESCE(defer_reason, 'UNSPECIFIED') AS reason, count(*) AS n FROM solprint.radar_jobs
              WHERE network = $1 AND state = 'DEFERRED' GROUP BY 1 ORDER BY 1",
        )
        .bind(NETWORK)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?
        .iter()
        .map(|r| (r.get("reason"), r.get("n")))
        .collect();
        let oldest: Option<OffsetDateTime> = sqlx::query_scalar(
            "SELECT min(created_at) FROM solprint.radar_jobs WHERE network = $1 AND state IN ('PENDING', 'DEFERRED')",
        )
        .bind(NETWORK)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))?;
        let open_total = open_by_class.iter().map(|(_, n)| n).sum();
        Ok(JobCounts {
            by_state,
            open_by_class,
            deferred_by_reason,
            oldest_pending_at: opt_utc(oldest),
            open_total,
        })
    }

    /// Estimated credits RADAR jobs spent since `since` (the billing cycle start).
    pub async fn radar_credits_spent_since(
        &self,
        since: &UtcTimestamp,
    ) -> Result<i64, StorageError> {
        sqlx::query_scalar(
            "SELECT COALESCE(sum(GREATEST(j.credits_spent,
                        CASE WHEN j.state IN ('WAITING_DATA') AND b.backfill_id IS NOT NULL
                             THEN b.credits_spent + b.queued_count ELSE 0 END)), 0)::bigint
               FROM solprint.radar_jobs j LEFT JOIN solprint.de_backfills b ON b.backfill_id = j.backfill_id
              WHERE j.network = $1 AND j.created_at >= $2",
        )
        .bind(NETWORK)
        .bind(ts(since))
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    /// Credits by work class since `since` (discovery versus investigation).
    pub async fn radar_credits_by_class(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        Ok(sqlx::query(
            "SELECT work_class, COALESCE(sum(credits_spent), 0)::bigint AS credits FROM solprint.radar_jobs
              WHERE network = $1 AND created_at >= $2 GROUP BY work_class ORDER BY work_class",
        )
        .bind(NETWORK)
        .bind(ts(since))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?
        .iter()
        .map(|r| (r.get("work_class"), r.get("credits")))
        .collect())
    }

    /// Estimated credits of open (not yet spent) RADAR work.
    pub async fn radar_open_estimates(&self) -> Result<i64, StorageError> {
        sqlx::query_scalar(
            "SELECT COALESCE(sum(estimated_credits), 0)::bigint FROM solprint.radar_jobs
              WHERE network = $1 AND state IN ('RUNNING', 'WAITING_DATA')",
        )
        .bind(NETWORK)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    /// Progress of a backfill and of the ingestion of what it queued.
    pub async fn radar_backfill_progress(
        &self,
        backfill_id: Uuid,
    ) -> Result<Option<BackfillProgress>, StorageError> {
        let Some(b) = sqlx::query(
            "SELECT status, stop_reason, coverage_state, discovered_count, queued_count, duplicate_count,
                    credits_spent, oldest_slot_seen, newest_slot_seen, history_exhausted
               FROM solprint.de_backfills WHERE backfill_id = $1",
        )
        .bind(backfill_id)
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?
        else {
            return Ok(None);
        };
        let t = sqlx::query(
            "WITH sigs AS (SELECT DISTINCT network, signature FROM solprint.de_observations WHERE backfill_id = $1)
             SELECT
               count(*) FILTER (WHERE j.state IN ('PENDING', 'FETCHING', 'FETCH_RETRY')) AS pending,
               count(*) FILTER (WHERE j.state IN ('UNAVAILABLE', 'UNSUPPORTED', 'QUARANTINED', 'CANCELLED')) AS unavailable,
               count(t.signature) AS retrieved,
               count(t.signature) FILTER (WHERE t.execution_status = 'FAILED') AS failed,
               count(t.signature) FILTER (WHERE t.decode_status = 'VERIFIED') AS verified,
               count(t.signature) FILTER (WHERE t.decode_status IN ('UNSUPPORTED', 'AMBIGUOUS', 'INSUFFICIENT_EVIDENCE', 'PARTIALLY_DECODED', 'INVALID')) AS unsupported,
               min(t.block_time) AS oldest_bt, max(t.block_time) AS newest_bt,
               (SELECT o.signature FROM solprint.de_observations o WHERE o.backfill_id = $1
                 ORDER BY o.slot DESC NULLS LAST, o.observation_id LIMIT 1) AS newest_sig
               FROM sigs s
               LEFT JOIN solprint.de_jobs j ON j.network = s.network AND j.signature = s.signature
               LEFT JOIN solprint.de_transactions t ON t.network = s.network AND t.signature = s.signature",
        )
        .bind(backfill_id)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(Some(BackfillProgress {
            status: b.get("status"),
            stop_reason: b.get("stop_reason"),
            coverage_state: b.get("coverage_state"),
            discovered: b.get("discovered_count"),
            queued: b.get("queued_count"),
            duplicates: b.get("duplicate_count"),
            credits_spent: b.get("credits_spent"),
            oldest_slot: opt_u64(b.get("oldest_slot_seen")),
            newest_slot: opt_u64(b.get("newest_slot_seen")),
            history_exhausted: b.get("history_exhausted"),
            transactions_pending: t.get("pending"),
            transactions_unavailable: t.get("unavailable"),
            transactions_retrieved: t.get("retrieved"),
            transactions_failed: t.get("failed"),
            transactions_verified: t.get("verified"),
            transactions_unsupported: t.get("unsupported"),
            newest_signature: t.get("newest_sig"),
            oldest_block_time: opt_utc(t.get("oldest_bt")),
            newest_block_time: opt_utc(t.get("newest_bt")),
        }))
    }

    /// The newest local slot with a block time (for lookback estimates).
    pub async fn radar_reference_slot(&self) -> Result<Option<(u64, UtcTimestamp)>, StorageError> {
        let r = sqlx::query(
            "SELECT slot, block_time FROM solprint.de_transactions WHERE block_time IS NOT NULL
              ORDER BY slot DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(r.map(|r| (u64_of(r.get("slot")), utc(r.get("block_time")))))
    }

    /// Wallet history signatures already stored locally (reuse metric).
    pub async fn radar_local_signature_count(&self, address: &str) -> Result<i64, StorageError> {
        sqlx::query_scalar(
            "SELECT count(DISTINCT signature) FROM solprint.de_swaps WHERE wallet = $1 AND canonical",
        )
        .bind(address)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    /// Newest and oldest signatures already covered for a wallet by its
    /// RADAR history backfills: the next stage continues before the oldest,
    /// and refreshes stop at the newest.
    pub async fn radar_wallet_signature_bounds(
        &self,
        address: &str,
    ) -> Result<(Option<String>, Option<String>), StorageError> {
        let r = sqlx::query(
            "WITH bf AS (SELECT backfill_id FROM solprint.radar_jobs
                          WHERE network = $1 AND subject = $2 AND backfill_id IS NOT NULL
                            AND work_class IN ('INITIAL_HISTORY', 'EXPANDED_HISTORY', 'EXTENDED_HISTORY', 'ACTIVE_MONITORING', 'DORMANT_RECHECK'))
             SELECT (SELECT o.signature FROM solprint.de_observations o WHERE o.backfill_id IN (SELECT backfill_id FROM bf)
                      ORDER BY o.slot DESC NULLS LAST LIMIT 1) AS newest,
                    (SELECT o.signature FROM solprint.de_observations o WHERE o.backfill_id IN (SELECT backfill_id FROM bf)
                      ORDER BY o.slot ASC NULLS LAST LIMIT 1) AS oldest",
        )
        .bind(NETWORK)
        .bind(address)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok((r.get("newest"), r.get("oldest")))
    }

    /// Aggregate retrieval coverage of every history backfill of a wallet.
    pub async fn radar_wallet_retrieval(
        &self,
        address: &str,
    ) -> Result<BackfillProgress, StorageError> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT backfill_id FROM solprint.radar_jobs WHERE network = $1 AND subject = $2 AND backfill_id IS NOT NULL
               AND work_class IN ('INITIAL_HISTORY', 'EXPANDED_HISTORY', 'EXTENDED_HISTORY', 'ACTIVE_MONITORING', 'DORMANT_RECHECK')",
        )
        .bind(NETWORK)
        .bind(address)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        let mut total = BackfillProgress::default();
        for id in ids {
            if let Some(p) = self.radar_backfill_progress(id).await? {
                total.discovered += p.discovered;
                total.queued += p.queued;
                total.duplicates += p.duplicates;
                total.credits_spent += p.credits_spent;
                total.transactions_pending += p.transactions_pending;
                total.transactions_unavailable += p.transactions_unavailable;
                total.transactions_retrieved += p.transactions_retrieved;
                total.transactions_failed += p.transactions_failed;
                total.transactions_verified += p.transactions_verified;
                total.transactions_unsupported += p.transactions_unsupported;
                total.history_exhausted |= p.history_exhausted;
                total.oldest_block_time = match (total.oldest_block_time, p.oldest_block_time) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                total.newest_block_time = match (total.newest_block_time, p.newest_block_time) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
            }
        }
        Ok(total)
    }

    // ---- DATA ENGINE reads -------------------------------------------------------------------

    pub async fn radar_swap(&self, swap_id: &str) -> Result<Option<SwapForRadar>, StorageError> {
        let row = sqlx::query(const_sql!(
            "SELECT {SWAP_COLS} FROM solprint.de_swaps s
               JOIN solprint.de_transactions t ON t.network = s.network AND t.signature = s.signature
              WHERE s.swap_id = $1"
        ))
        .bind(swap_id)
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(row.as_ref().map(swap_row))
    }

    /// Canonical verified swaps attributed to a wallet, oldest first
    /// (bounded). Aggregator legs are rows of their route, never swaps here.
    pub async fn radar_wallet_swaps(
        &self,
        address: &str,
        limit: i64,
    ) -> Result<Vec<SwapForRadar>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "SELECT {SWAP_COLS} FROM (
                SELECT * FROM solprint.de_swaps
                 WHERE wallet = $1 AND canonical AND classification_status = 'VERIFIED'
                 ORDER BY block_time DESC NULLS LAST, slot DESC LIMIT $2) s
               JOIN solprint.de_transactions t ON t.network = s.network AND t.signature = s.signature
              ORDER BY s.slot, s.transaction_index NULLS LAST, s.instruction_ref"
        ))
        .bind(address)
        .bind(limit.clamp(1, 20_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(swap_row).collect())
    }

    /// The wallet's token balances at the start and end of the given
    /// transactions, summed over the token accounts it owns there.
    pub async fn radar_wallet_token_balances(
        &self,
        address: &str,
        signatures: &[String],
    ) -> Result<Vec<TokenBalanceRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT signature, asset,
                    CASE WHEN bool_and(pre_evidence <> 'UNKNOWN') THEN sum(COALESCE(pre_amount, 0))::text END AS pre,
                    CASE WHEN bool_and(post_evidence <> 'UNKNOWN') THEN sum(COALESCE(post_amount, 0))::text END AS post
               FROM solprint.de_balance_changes
              WHERE network = $1 AND owner = $2 AND asset <> 'SOL' AND signature = ANY($3)
              GROUP BY signature, asset",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(signatures)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| TokenBalanceRow {
                signature: r.get("signature"),
                mint: r.get("asset"),
                pre: r.get("pre"),
                post: r.get("post"),
            })
            .collect())
    }

    /// Which backfill (and RADAR job) observed a signature, if any.
    pub async fn radar_signature_origin(
        &self,
        signature: &str,
    ) -> Result<SignatureOrigin, StorageError> {
        let r = sqlx::query(
            "SELECT o.backfill_id, o.source, j.job_id, j.work_class, j.subject
               FROM solprint.de_observations o
               LEFT JOIN solprint.radar_jobs j ON j.backfill_id = o.backfill_id
              WHERE o.network = $1 AND o.signature = $2
              ORDER BY (j.job_id IS NULL), o.observation_id LIMIT 1",
        )
        .bind(NETWORK)
        .bind(signature)
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(r.map_or(
            SignatureOrigin {
                backfill_id: None,
                job_id: None,
                work_class: None,
                subject: None,
                source: "UNKNOWN".into(),
            },
            |r| SignatureOrigin {
                backfill_id: r.get("backfill_id"),
                job_id: r.get("job_id"),
                work_class: r.get("work_class"),
                subject: r.get("subject"),
                source: r.get("source"),
            },
        ))
    }

    /// DATA ENGINE events not yet delivered to RADAR, in commit order.
    pub async fn radar_pending_events(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::data_engine::OutboxEventRow>, StorageError> {
        self.de_outbox_pending(RADAR_CONSUMER, limit.clamp(1, 1_000))
            .await
    }

    /// Record delivery of an event that needed no registry change.
    pub async fn radar_ack_event(&self, event_id: Uuid) -> Result<(), StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        mark_delivered(&mut tx, event_id).await?;
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(())
    }

    // ---- registry writes ---------------------------------------------------------------------

    /// Apply one piece of evidence and acknowledge the DATA ENGINE event in
    /// the same transaction. A revision older than the stored one is ignored.
    /// New wallets get a `WalletCandidateDiscovered` event (`discovered`).
    pub async fn radar_apply_evidence(
        &self,
        event_id: Option<Uuid>,
        ev: &EvidenceWrite,
        new_wallet: Option<(Uuid, &RadarEventWrite)>,
    ) -> Result<EvidenceOutcome, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let mut outcome = EvidenceOutcome::Unchanged;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM solprint.radar_wallets WHERE network = $1 AND address = $2)",
        )
        .bind(NETWORK)
        .bind(&ev.address)
        .fetch_one(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if !exists {
            let Some((wallet_id, discovered)) = new_wallet.filter(|_| ev.valid) else {
                if let Some(id) = event_id {
                    mark_delivered(&mut tx, id).await?;
                }
                tx.commit().await.map_err(|x| e(&x))?;
                return Ok(EvidenceOutcome::Unchanged);
            };
            let inserted = sqlx::query(
                "INSERT INTO solprint.radar_wallets
                    (wallet_id, network, address, lifecycle, eligibility, first_discovered_at, first_discovery_source,
                     last_discovered_at)
                 VALUES ($1, $2, $3, 'DISCOVERED', 'NEEDS_MORE_EVIDENCE', now(), $4, now())
                 ON CONFLICT (network, address) DO NOTHING",
            )
            .bind(wallet_id)
            .bind(NETWORK)
            .bind(&ev.address)
            .bind(&ev.discovery_source)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?
            .rows_affected()
                == 1;
            if inserted {
                sqlx::query(
                    "INSERT INTO solprint.radar_wallet_transitions (network, address, from_state, to_state, trigger, reasons, policy_version)
                     VALUES ($1, $2, NULL, 'DISCOVERED', 'EVIDENCE_OBSERVED', ARRAY[$3]::text[], 'registry-1')",
                )
                .bind(NETWORK)
                .bind(&ev.address)
                .bind(&ev.discovery_source)
                .execute(&mut *tx)
                .await
                .map_err(|x| e(&x))?;
                insert_event(&mut tx, discovered).await?;
                outcome = EvidenceOutcome::NewWallet;
            }
        }
        let row = sqlx::query(
            "INSERT INTO solprint.radar_wallet_evidence
                (network, address, swap_id, signature, slot, block_time, token_mint, quote_mint, pool, protocol,
                 economic_role, attribution, finality, discovery_source, job_id, backfill_id, swap_revision, valid,
                 invalidation_reason, invalidated_at, evidence_available_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19,
                     CASE WHEN $18 THEN NULL ELSE now() END, $20)
             ON CONFLICT (network, address, swap_id) DO UPDATE SET
                swap_revision = EXCLUDED.swap_revision,
                finality = EXCLUDED.finality,
                slot = EXCLUDED.slot, block_time = EXCLUDED.block_time,
                token_mint = EXCLUDED.token_mint, quote_mint = EXCLUDED.quote_mint, pool = COALESCE(EXCLUDED.pool, solprint.radar_wallet_evidence.pool),
                economic_role = EXCLUDED.economic_role, attribution = EXCLUDED.attribution,
                valid = EXCLUDED.valid,
                invalidation_reason = CASE WHEN EXCLUDED.valid THEN NULL ELSE EXCLUDED.invalidation_reason END,
                invalidated_at = CASE WHEN EXCLUDED.valid THEN NULL
                                      ELSE COALESCE(solprint.radar_wallet_evidence.invalidated_at, now()) END,
                updated_at = now()
             WHERE EXCLUDED.swap_revision >= solprint.radar_wallet_evidence.swap_revision
               AND (solprint.radar_wallet_evidence.valid IS DISTINCT FROM EXCLUDED.valid
                    OR solprint.radar_wallet_evidence.swap_revision <> EXCLUDED.swap_revision
                    OR solprint.radar_wallet_evidence.finality <> EXCLUDED.finality
                    OR solprint.radar_wallet_evidence.attribution <> EXCLUDED.attribution)
             RETURNING (xmax = 0) AS inserted",
        )
        .bind(NETWORK)
        .bind(&ev.address)
        .bind(&ev.swap_id)
        .bind(&ev.signature)
        .bind(i64_of(ev.slot))
        .bind(ev.block_time.as_ref().map(ts))
        .bind(&ev.token_mint)
        .bind(&ev.quote_mint)
        .bind(&ev.pool)
        .bind(&ev.protocol)
        .bind(&ev.economic_role)
        .bind(&ev.attribution)
        .bind(&ev.finality)
        .bind(&ev.discovery_source)
        .bind(ev.job_id)
        .bind(ev.backfill_id)
        .bind(ev.swap_revision)
        .bind(ev.valid)
        .bind(&ev.invalidation_reason)
        .bind(ts(&ev.evidence_available_at))
        .fetch_optional(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if let Some(r) = row {
            let inserted: bool = r.get("inserted");
            if outcome == EvidenceOutcome::Unchanged {
                outcome = if inserted {
                    EvidenceOutcome::NewEvidence
                } else {
                    EvidenceOutcome::Updated
                };
            }
            if inserted {
                sqlx::query(
                    "UPDATE solprint.radar_wallets SET last_discovered_at = now() WHERE network = $1 AND address = $2",
                )
                .bind(NETWORK)
                .bind(&ev.address)
                .execute(&mut *tx)
                .await
                .map_err(|x| e(&x))?;
            }
            refresh_wallet_counts(&mut tx, &ev.address).await?;
        }
        if let Some(id) = event_id {
            mark_delivered(&mut tx, id).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(outcome)
    }

    /// Invalidate every evidence row of a swap (retraction) and acknowledge
    /// the event. Returns the affected wallets.
    pub async fn radar_invalidate_swap(
        &self,
        event_id: Option<Uuid>,
        swap_id: &str,
        reason: &str,
    ) -> Result<Vec<String>, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let wallets: Vec<String> = sqlx::query_scalar(
            "UPDATE solprint.radar_wallet_evidence SET valid = FALSE, invalidation_reason = $2,
                    invalidated_at = COALESCE(invalidated_at, now()), updated_at = now()
              WHERE swap_id = $1 AND valid RETURNING address",
        )
        .bind(swap_id)
        .bind(reason)
        .fetch_all(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        for w in &wallets {
            refresh_wallet_counts(&mut tx, w).await?;
        }
        if let Some(id) = event_id {
            mark_delivered(&mut tx, id).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(wallets)
    }

    /// Record that a signature became final for every evidence row naming
    /// it, and acknowledge the event.
    pub async fn radar_finalize_signature(
        &self,
        event_id: Option<Uuid>,
        signature: &str,
    ) -> Result<u64, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let wallets: Vec<String> = sqlx::query_scalar(
            "UPDATE solprint.radar_wallet_evidence SET finality = 'FINALIZED', updated_at = now()
              WHERE network = $1 AND signature = $2 AND finality <> 'FINALIZED' RETURNING address",
        )
        .bind(NETWORK)
        .bind(signature)
        .fetch_all(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        for w in &wallets {
            refresh_wallet_counts(&mut tx, w).await?;
        }
        if let Some(id) = event_id {
            mark_delivered(&mut tx, id).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(u64::try_from(wallets.len()).unwrap_or(u64::MAX))
    }

    /// Owner-directed registration (diagnostics): never required for
    /// autonomous operation. Returns `false` when the wallet already exists.
    pub async fn radar_register_operator_wallet(
        &self,
        wallet_id: Uuid,
        address: &str,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let inserted = sqlx::query(
            "INSERT INTO solprint.radar_wallets
                (wallet_id, network, address, lifecycle, eligibility, first_discovered_at, first_discovery_source, last_discovered_at)
             VALUES ($1, $2, $3, 'DISCOVERED', 'NEEDS_MORE_EVIDENCE', now(), 'OPERATOR', now())
             ON CONFLICT (network, address) DO NOTHING",
        )
        .bind(wallet_id)
        .bind(NETWORK)
        .bind(address)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?
        .rows_affected()
            == 1;
        if inserted {
            sqlx::query(
                "INSERT INTO solprint.radar_wallet_transitions (network, address, from_state, to_state, trigger, reasons, policy_version)
                 VALUES ($1, $2, NULL, 'DISCOVERED', 'OPERATOR_REQUEST', ARRAY['OPERATOR'], 'registry-1')",
            )
            .bind(NETWORK)
            .bind(address)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(inserted)
    }

    pub async fn radar_set_operator_excluded(
        &self,
        address: &str,
        excluded: bool,
    ) -> Result<bool, StorageError> {
        let r = sqlx::query(
            "UPDATE solprint.radar_wallets SET operator_excluded = $3, needs_evaluation = TRUE,
                    revision = revision + 1, updated_at = now()
              WHERE network = $1 AND address = $2",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(excluded)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn radar_mark_dirty(&self, address: &str) -> Result<bool, StorageError> {
        let r = sqlx::query(
            "UPDATE solprint.radar_wallets SET needs_evaluation = TRUE, revision = revision + 1, updated_at = now()
              WHERE network = $1 AND address = $2",
        )
        .bind(NETWORK)
        .bind(address)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(r.rows_affected() == 1)
    }

    /// Reactivate an archived/dormant/rejected wallet by operator request.
    pub async fn radar_reactivate(
        &self,
        address: &str,
        trigger: &str,
        event: Option<&RadarEventWrite>,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let prev: Option<String> = sqlx::query_scalar(
            "SELECT lifecycle FROM solprint.radar_wallets WHERE network = $1 AND address = $2
                AND lifecycle IN ('ARCHIVED', 'DORMANT', 'REJECTED') FOR UPDATE",
        )
        .bind(NETWORK)
        .bind(address)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        let Some(prev) = prev else {
            tx.commit().await.map_err(|x| e(&x))?;
            return Ok(false);
        };
        sqlx::query(
            "UPDATE solprint.radar_wallets SET lifecycle = 'DISCOVERED', needs_evaluation = TRUE,
                    operator_excluded = FALSE, monitoring_mode = 'INACTIVE', revision = revision + 1, updated_at = now()
              WHERE network = $1 AND address = $2",
        )
        .bind(NETWORK)
        .bind(address)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        sqlx::query(
            "INSERT INTO solprint.radar_wallet_transitions (network, address, from_state, to_state, trigger, reasons, policy_version)
             VALUES ($1, $2, $3, 'DISCOVERED', $4, ARRAY['REACTIVATED'], 'registry-1')",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(&prev)
        .bind(trigger)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if let Some(ev) = event {
            insert_event(&mut tx, ev).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(true)
    }

    /// Apply an evaluation atomically. Returns `false` (nothing written) when
    /// the wallet changed since it was read; the caller re-evaluates later.
    pub async fn radar_save_evaluation(&self, w: &EvaluationWrite) -> Result<bool, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let updated = sqlx::query(
            "UPDATE solprint.radar_wallets SET
                lifecycle = $4, eligibility = $5, eligibility_reasons = $6, qualification = $7,
                qualification_reasons = $8, priority_score = $9::numeric, priority_policy_version = $10,
                history_stage = $11, history_coverage_state = $12, history_coverage_start = $13,
                history_coverage_end = $14, last_verified_trade_at = $15, next_investigation_at = $16,
                valid_evidence_count = $17, verified_swap_count = $18, distinct_token_count = $19,
                last_investigated_at = CASE WHEN $20 THEN now() ELSE last_investigated_at END,
                last_evaluated_at = now(), needs_evaluation = FALSE, revision = revision + 1, updated_at = now()
              WHERE network = $1 AND address = $2 AND revision = $3",
        )
        .bind(NETWORK)
        .bind(&w.address)
        .bind(w.expected_revision)
        .bind(&w.lifecycle)
        .bind(&w.eligibility)
        .bind(&w.eligibility_reasons)
        .bind(&w.qualification)
        .bind(&w.qualification_reasons)
        .bind(&w.priority_score)
        .bind(&w.policy_version)
        .bind(&w.history_stage)
        .bind(&w.history_coverage_state)
        .bind(w.history_coverage_start.as_ref().map(ts))
        .bind(w.history_coverage_end.as_ref().map(ts))
        .bind(w.last_verified_trade_at.as_ref().map(ts))
        .bind(w.next_investigation_at.as_ref().map(ts))
        .bind(w.valid_evidence_count)
        .bind(w.verified_swap_count)
        .bind(w.distinct_token_count)
        .bind(w.investigated)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?
        .rows_affected()
            == 1;
        if !updated {
            tx.rollback().await.map_err(|x| e(&x))?;
            return Ok(false);
        }
        for t in &w.transitions {
            sqlx::query(
                "INSERT INTO solprint.radar_wallet_transitions
                    (network, address, from_state, to_state, trigger, reasons, metrics, policy_version)
                 VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8)",
            )
            .bind(NETWORK)
            .bind(&w.address)
            .bind(&t.from_state)
            .bind(&t.to_state)
            .bind(&t.trigger)
            .bind(&t.reasons)
            .bind(&t.metrics_json)
            .bind(&w.policy_version)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        let last_hash: Option<String> = sqlx::query_scalar(
            "SELECT input_hash FROM solprint.radar_wallet_snapshots WHERE network = $1 AND address = $2
              ORDER BY computed_at DESC, snapshot_id DESC LIMIT 1",
        )
        .bind(NETWORK)
        .bind(&w.address)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if last_hash.as_deref() != Some(w.input_hash.as_str()) {
            sqlx::query(
                "INSERT INTO solprint.radar_wallet_snapshots
                    (network, address, policy_version, input_hash, priority_score, qualification, metrics)
                 VALUES ($1, $2, $3, $4, $5::numeric, $6, $7::jsonb)",
            )
            .bind(NETWORK)
            .bind(&w.address)
            .bind(&w.policy_version)
            .bind(&w.input_hash)
            .bind(&w.priority_score)
            .bind(w.qualification.as_deref().unwrap_or("INSUFFICIENT_ACTIVITY"))
            .bind(&w.metrics_json)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        for ev in &w.events {
            insert_event(&mut tx, ev).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(true)
    }

    // ---- registry reads ----------------------------------------------------------------------

    pub async fn radar_wallet(&self, address: &str) -> Result<Option<WalletRow>, StorageError> {
        let row = sqlx::query(const_sql!(
            "SELECT {WALLET_COLS} FROM solprint.radar_wallets WHERE network = $1 AND address = $2"
        ))
        .bind(NETWORK)
        .bind(address)
        .fetch_optional(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(row.as_ref().map(wallet_row))
    }

    /// Wallets that need evaluation: dirty ones first, then those whose
    /// periodic reevaluation is due (bounded; never a full registry scan).
    pub async fn radar_wallets_to_evaluate(
        &self,
        reevaluate_after: Duration,
        limit: i64,
    ) -> Result<Vec<WalletRow>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "(SELECT {WALLET_COLS} FROM solprint.radar_wallets WHERE network = $1 AND needs_evaluation
               ORDER BY updated_at LIMIT $3)
             UNION ALL
             (SELECT {WALLET_COLS} FROM solprint.radar_wallets
               WHERE network = $1 AND NOT needs_evaluation AND lifecycle NOT IN ('ARCHIVED')
                 AND (last_evaluated_at IS NULL OR last_evaluated_at < now() - make_interval(secs => $2))
               ORDER BY last_evaluated_at NULLS FIRST LIMIT $3)"
        ))
        .bind(NETWORK)
        .bind(secs(reevaluate_after))
        .bind(limit.clamp(1, 1_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        let mut out: Vec<WalletRow> = rows.iter().map(wallet_row).collect();
        out.truncate(usize::try_from(limit.max(0)).unwrap_or(0));
        Ok(out)
    }

    pub async fn radar_wallets(&self, f: &WalletFilter) -> Result<Vec<WalletRow>, StorageError> {
        let (sort_expr, cursor_cast) = match f.sort {
            WalletSort::Priority => ("COALESCE(priority_score, -1)", "::numeric"),
            WalletSort::Discovered => ("first_discovered_at", "::timestamptz"),
            WalletSort::LastTrade => (
                "COALESCE(last_verified_trade_at, '-infinity'::timestamptz)",
                "::timestamptz",
            ),
        };
        let (ak, aa) = f
            .after
            .as_ref()
            .map_or((None, None), |(k, a)| (Some(k.clone()), Some(a.clone())));
        let rows = sqlx::query(const_sql!(
            "SELECT {WALLET_COLS} FROM solprint.radar_wallets
              WHERE network = $1
                AND ($2::text IS NULL OR lifecycle = $2)
                AND ($3::text IS NULL OR eligibility = $3)
                AND ($4::text IS NULL OR address IN (SELECT address FROM solprint.radar_wallet_evidence
                                                      WHERE network = $1 AND discovery_source = $4))
                AND ($5::text IS NULL OR monitoring_mode = $5)
                AND ($6::text IS NULL OR history_coverage_state = $6)
                AND ($7::text IS NULL OR qualification = $7)
                AND ($8::numeric IS NULL OR priority_score >= $8::numeric)
                AND ($9::numeric IS NULL OR priority_score <= $9::numeric)
                AND ($10::timestamptz IS NULL OR last_verified_trade_at >= $10)
                AND ($11::text IS NULL OR ({sort_expr}, address) < ($11{cursor_cast}, $12))
              ORDER BY {sort_expr} DESC, address DESC LIMIT $13"
        ))
        .bind(NETWORK)
        .bind(&f.lifecycle)
        .bind(&f.eligibility)
        .bind(&f.source)
        .bind(&f.monitoring_mode)
        .bind(&f.coverage_state)
        .bind(&f.qualification)
        .bind(&f.min_priority)
        .bind(&f.max_priority)
        .bind(f.active_since.as_ref().map(ts))
        .bind(ak)
        .bind(aa)
        .bind(f.limit.clamp(1, 200))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(wallet_row).collect())
    }

    pub async fn radar_wallet_evidence(
        &self,
        address: &str,
        limit: i64,
    ) -> Result<Vec<EvidenceRow>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "SELECT {EVIDENCE_COLS} FROM solprint.radar_wallet_evidence WHERE network = $1 AND address = $2
              ORDER BY block_time DESC NULLS LAST, evidence_id DESC LIMIT $3"
        ))
        .bind(NETWORK)
        .bind(address)
        .bind(limit.clamp(1, 2_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(evidence_row).collect())
    }

    /// Distinct discovery sources of a wallet's evidence (any validity).
    pub async fn radar_wallet_sources(&self, address: &str) -> Result<Vec<String>, StorageError> {
        sqlx::query_scalar(
            "SELECT DISTINCT discovery_source FROM solprint.radar_wallet_evidence WHERE network = $1 AND address = $2
              ORDER BY 1",
        )
        .bind(NETWORK)
        .bind(address)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    pub async fn radar_wallet_transitions(
        &self,
        address: &str,
        limit: i64,
    ) -> Result<Vec<TransitionRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT transition_id, from_state, to_state, trigger, reasons, metrics::text AS metrics, policy_version,
                    transitioned_at
               FROM solprint.radar_wallet_transitions WHERE network = $1 AND address = $2
              ORDER BY transition_id DESC LIMIT $3",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(limit.clamp(1, 1_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| TransitionRow {
                transition_id: r.get("transition_id"),
                from_state: r.get("from_state"),
                to_state: r.get("to_state"),
                trigger: r.get("trigger"),
                reasons: r.get("reasons"),
                metrics_json: r.get("metrics"),
                policy_version: r.get("policy_version"),
                transitioned_at: utc(r.get("transitioned_at")),
            })
            .collect())
    }

    pub async fn radar_wallet_snapshots(
        &self,
        address: &str,
        limit: i64,
    ) -> Result<Vec<SnapshotRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT snapshot_id, policy_version, input_hash, priority_score::text AS priority_score, qualification,
                    metrics::text AS metrics, computed_at
               FROM solprint.radar_wallet_snapshots WHERE network = $1 AND address = $2
              ORDER BY computed_at DESC, snapshot_id DESC LIMIT $3",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| SnapshotRow {
                snapshot_id: r.get("snapshot_id"),
                policy_version: r.get("policy_version"),
                input_hash: r.get("input_hash"),
                priority_score: r.get("priority_score"),
                qualification: r.get("qualification"),
                metrics_json: r.get("metrics"),
                computed_at: utc(r.get("computed_at")),
            })
            .collect())
    }

    /// Wallets in the given lifecycle states (bounded), highest priority first.
    pub async fn radar_wallets_in(
        &self,
        states: &[String],
        limit: i64,
    ) -> Result<Vec<WalletRow>, StorageError> {
        let rows = sqlx::query(const_sql!(
            "SELECT {WALLET_COLS} FROM solprint.radar_wallets WHERE network = $1 AND lifecycle = ANY($2)
              ORDER BY priority_score DESC NULLS LAST, address LIMIT $3"
        ))
        .bind(NETWORK)
        .bind(states)
        .bind(limit.clamp(1, 10_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows.iter().map(wallet_row).collect())
    }

    pub async fn radar_count_in(&self, states: &[String]) -> Result<i64, StorageError> {
        sqlx::query_scalar(
            "SELECT count(*) FROM solprint.radar_wallets WHERE network = $1 AND lifecycle = ANY($2)",
        )
        .bind(NETWORK)
        .bind(states)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    /// A direct lifecycle change by maintenance (dormancy, archival,
    /// capacity), recorded with its transition and events.
    pub async fn radar_set_lifecycle(&self, c: &LifecycleChange) -> Result<bool, StorageError> {
        let LifecycleChange {
            address,
            from,
            to,
            trigger,
            reasons,
            monitoring_mode,
            event,
        } = c;
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let r = sqlx::query(
            "UPDATE solprint.radar_wallets SET lifecycle = $4, monitoring_mode = COALESCE($5, monitoring_mode),
                    revision = revision + 1, updated_at = now()
              WHERE network = $1 AND address = $2 AND lifecycle = $3",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(from)
        .bind(to)
        .bind(monitoring_mode)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if r.rows_affected() != 1 {
            tx.rollback().await.map_err(|x| e(&x))?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO solprint.radar_wallet_transitions (network, address, from_state, to_state, trigger, reasons, policy_version)
             VALUES ($1, $2, $3, $4, $5, $6, 'lifecycle-1')",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(from)
        .bind(to)
        .bind(trigger)
        .bind(reasons)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if let Some(ev) = event {
            insert_event(&mut tx, ev).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(true)
    }

    // ---- monitoring --------------------------------------------------------------------------

    pub async fn radar_monitoring(
        &self,
        active_only: bool,
        limit: i64,
    ) -> Result<Vec<MonitoringRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT assignment_id, address, mode, target_id, priority_at_start::text AS priority_at_start, started_at,
                    ended_at, end_reason
               FROM solprint.radar_monitoring WHERE network = $1 AND (NOT $2 OR ended_at IS NULL)
              ORDER BY (ended_at IS NULL) DESC, started_at DESC LIMIT $3",
        )
        .bind(NETWORK)
        .bind(active_only)
        .bind(limit.clamp(1, 1_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| MonitoringRow {
                assignment_id: r.get("assignment_id"),
                address: r.get("address"),
                mode: r.get("mode"),
                target_id: r.get("target_id"),
                priority_at_start: r.get("priority_at_start"),
                started_at: utc(r.get("started_at")),
                ended_at: opt_utc(r.get("ended_at")),
                end_reason: r.get("end_reason"),
            })
            .collect())
    }

    /// Most recent end of a monitoring assignment per wallet (cooldown).
    pub async fn radar_monitoring_last_ended(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<(String, UtcTimestamp)>, StorageError> {
        Ok(sqlx::query(
            "SELECT address, max(ended_at) AS ended FROM solprint.radar_monitoring
              WHERE network = $1 AND ended_at >= $2 GROUP BY address",
        )
        .bind(NETWORK)
        .bind(ts(since))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?
        .iter()
        .map(|r| (r.get("address"), utc(r.get("ended"))))
        .collect())
    }

    /// Start monitoring a wallet (and, for live mode, enable its DATA ENGINE
    /// target, created by RADAR if missing) in one transaction.
    pub async fn radar_start_monitoring(
        &self,
        address: &str,
        mode: &str,
        priority: Option<&str>,
        new_target_id: Uuid,
        event: Option<&RadarEventWrite>,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let target_id: Option<Uuid> = if mode == "LIVE_SUBSCRIPTION" {
            Some(
                sqlx::query_scalar(
                    "INSERT INTO solprint.de_targets (target_id, network, target_type, address, label, priority, commitment, discovery_source)
                     VALUES ($1, $2, 'WALLET_ADDRESS', $3, 'radar', 'P4', 'confirmed', 'RADAR')
                     ON CONFLICT (network, target_type, address) DO UPDATE SET enabled = TRUE, updated_at = now()
                     RETURNING target_id",
                )
                .bind(new_target_id)
                .bind(NETWORK)
                .bind(address)
                .fetch_one(&mut *tx)
                .await
                .map_err(|x| e(&x))?,
            )
        } else {
            None
        };
        let inserted = sqlx::query(
            "INSERT INTO solprint.radar_monitoring (network, address, mode, target_id, priority_at_start)
             VALUES ($1, $2, $3, $4, $5::numeric)
             ON CONFLICT (network, address) WHERE ended_at IS NULL DO NOTHING",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(mode)
        .bind(target_id)
        .bind(priority)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?
        .rows_affected()
            == 1;
        if !inserted {
            tx.rollback().await.map_err(|x| e(&x))?;
            return Ok(false);
        }
        let prev: Option<String> = sqlx::query_scalar(
            "SELECT lifecycle FROM solprint.radar_wallets WHERE network = $1 AND address = $2 FOR UPDATE",
        )
        .bind(NETWORK)
        .bind(address)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        sqlx::query(
            "UPDATE solprint.radar_wallets SET monitoring_mode = $3,
                    lifecycle = CASE WHEN lifecycle = 'PRIORITIZED' THEN 'MONITORED' ELSE lifecycle END,
                    revision = revision + 1, updated_at = now()
              WHERE network = $1 AND address = $2",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(mode)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if prev.as_deref() == Some("PRIORITIZED") {
            sqlx::query(
                "INSERT INTO solprint.radar_wallet_transitions (network, address, from_state, to_state, trigger, reasons, policy_version)
                 VALUES ($1, $2, 'PRIORITIZED', 'MONITORED', 'MONITORING_STARTED', ARRAY[$3], 'monitoring-1')",
            )
            .bind(NETWORK)
            .bind(address)
            .bind(mode)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        if let Some(ev) = event {
            insert_event(&mut tx, ev).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(true)
    }

    pub async fn radar_stop_monitoring(
        &self,
        address: &str,
        reason: &str,
        event: Option<&RadarEventWrite>,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool().begin().await.map_err(|x| e(&x))?;
        let target: Option<Option<Uuid>> = sqlx::query_scalar(
            "UPDATE solprint.radar_monitoring SET ended_at = now(), end_reason = $3
              WHERE network = $1 AND address = $2 AND ended_at IS NULL RETURNING target_id",
        )
        .bind(NETWORK)
        .bind(address)
        .bind(reason)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        let Some(target) = target else {
            tx.rollback().await.map_err(|x| e(&x))?;
            return Ok(false);
        };
        if let Some(t) = target {
            sqlx::query(
                "UPDATE solprint.de_targets SET enabled = FALSE, updated_at = now()
                  WHERE target_id = $1 AND discovery_source = 'RADAR'",
            )
            .bind(t)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        let prev: Option<String> = sqlx::query_scalar(
            "SELECT lifecycle FROM solprint.radar_wallets WHERE network = $1 AND address = $2 FOR UPDATE",
        )
        .bind(NETWORK)
        .bind(address)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        sqlx::query(
            "UPDATE solprint.radar_wallets SET monitoring_mode = 'INACTIVE',
                    lifecycle = CASE WHEN lifecycle = 'MONITORED' THEN 'PRIORITIZED' ELSE lifecycle END,
                    revision = revision + 1, updated_at = now()
              WHERE network = $1 AND address = $2",
        )
        .bind(NETWORK)
        .bind(address)
        .execute(&mut *tx)
        .await
        .map_err(|x| e(&x))?;
        if prev.as_deref() == Some("MONITORED") {
            sqlx::query(
                "INSERT INTO solprint.radar_wallet_transitions (network, address, from_state, to_state, trigger, reasons, policy_version)
                 VALUES ($1, $2, 'MONITORED', 'PRIORITIZED', 'MONITORING_STOPPED', ARRAY[$3], 'monitoring-1')",
            )
            .bind(NETWORK)
            .bind(address)
            .bind(reason)
            .execute(&mut *tx)
            .await
            .map_err(|x| e(&x))?;
        }
        if let Some(ev) = event {
            insert_event(&mut tx, ev).await?;
        }
        tx.commit().await.map_err(|x| e(&x))?;
        Ok(true)
    }

    /// Enabled DATA ENGINE wallet targets not owned by RADAR (they share the
    /// subscription ceiling).
    pub async fn radar_foreign_enabled_targets(&self) -> Result<i64, StorageError> {
        sqlx::query_scalar(
            "SELECT count(*) FROM solprint.de_targets WHERE network = $1 AND enabled
                AND target_type = 'WALLET_ADDRESS' AND discovery_source <> 'RADAR'",
        )
        .bind(NETWORK)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))
    }

    // ---- events ------------------------------------------------------------------------------

    pub async fn radar_events_recent(
        &self,
        limit: i64,
    ) -> Result<Vec<RadarEventRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT event_id, seq, event_type, aggregate_type, aggregate_id, aggregate_revision, correlation_id,
                    causation_id, source, observed_at, persisted_at, payload::text AS payload
               FROM solprint.radar_outbox ORDER BY seq DESC LIMIT $1",
        )
        .bind(limit.clamp(1, 1_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| RadarEventRow {
                event_id: r.get("event_id"),
                seq: r.get("seq"),
                event_type: r.get("event_type"),
                aggregate_type: r.get("aggregate_type"),
                aggregate_id: r.get("aggregate_id"),
                aggregate_revision: r.get("aggregate_revision"),
                correlation_id: r.get("correlation_id"),
                causation_id: r.get("causation_id"),
                source: r.get("source"),
                observed_at: utc(r.get("observed_at")),
                persisted_at: utc(r.get("persisted_at")),
                payload_json: r.get("payload"),
            })
            .collect())
    }

    /// RADAR events not yet delivered to `consumer`, in commit order.
    pub async fn radar_events_pending(
        &self,
        consumer: &str,
        limit: i64,
    ) -> Result<Vec<RadarEventRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT o.event_id, o.seq, o.event_type, o.aggregate_type, o.aggregate_id, o.aggregate_revision,
                    o.correlation_id, o.causation_id, o.source, o.observed_at, o.persisted_at, o.payload::text AS payload
               FROM solprint.radar_outbox o
               LEFT JOIN solprint.radar_outbox_deliveries d ON d.event_id = o.event_id AND d.consumer = $1
              WHERE d.event_id IS NULL ORDER BY o.seq LIMIT $2",
        )
        .bind(consumer)
        .bind(limit.clamp(1, 1_000))
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(rows
            .iter()
            .map(|r| RadarEventRow {
                event_id: r.get("event_id"),
                seq: r.get("seq"),
                event_type: r.get("event_type"),
                aggregate_type: r.get("aggregate_type"),
                aggregate_id: r.get("aggregate_id"),
                aggregate_revision: r.get("aggregate_revision"),
                correlation_id: r.get("correlation_id"),
                causation_id: r.get("causation_id"),
                source: r.get("source"),
                observed_at: utc(r.get("observed_at")),
                persisted_at: utc(r.get("persisted_at")),
                payload_json: r.get("payload"),
            })
            .collect())
    }

    pub async fn radar_events_mark(
        &self,
        consumer: &str,
        event_id: Uuid,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO solprint.radar_outbox_deliveries (consumer, event_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(consumer)
        .bind(event_id)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(())
    }

    // ---- counts and retention ----------------------------------------------------------------

    pub async fn radar_counts(&self) -> Result<RadarCounts, StorageError> {
        let pairs = |rows: Vec<sqlx::postgres::PgRow>| -> Vec<(String, i64)> {
            rows.iter().map(|r| (r.get("k"), r.get("n"))).collect()
        };
        let lifecycle = sqlx::query(
            "SELECT lifecycle AS k, count(*) AS n FROM solprint.radar_wallets WHERE network = $1 GROUP BY 1 ORDER BY 1",
        )
        .bind(NETWORK)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        let eligibility = sqlx::query(
            "SELECT eligibility AS k, count(*) AS n FROM solprint.radar_wallets WHERE network = $1 GROUP BY 1 ORDER BY 1",
        )
        .bind(NETWORK)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        let markets = sqlx::query(
            "SELECT selection_state AS k, count(*) AS n FROM solprint.radar_markets WHERE network = $1 GROUP BY 1 ORDER BY 1",
        )
        .bind(NETWORK)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        let monitoring = sqlx::query(
            "SELECT mode AS k, count(*) AS n FROM solprint.radar_monitoring WHERE network = $1 AND ended_at IS NULL GROUP BY 1 ORDER BY 1",
        )
        .bind(NETWORK)
        .fetch_all(self.pool())
        .await
        .map_err(|x| e(&x))?;
        let r = sqlx::query(
            "SELECT
                (SELECT count(*) FROM solprint.radar_wallets WHERE network = $1) AS wallets,
                (SELECT count(*) FROM solprint.radar_wallets WHERE network = $1 AND first_discovered_at >= now() - interval '24 hours') AS new24,
                (SELECT count(*) FROM solprint.radar_wallet_evidence WHERE network = $1) AS evidence,
                (SELECT count(*) FROM solprint.radar_wallet_evidence WHERE network = $1 AND NOT valid) AS invalidated,
                (SELECT count(*) FROM solprint.radar_markets WHERE network = $1) AS markets,
                (SELECT count(*) FROM solprint.radar_pools WHERE network = $1) AS pools,
                (SELECT count(*) FROM solprint.radar_pools WHERE network = $1 AND validation = 'SUPPORTED') AS pools_ok,
                (SELECT count(*) FROM solprint.radar_wallet_transitions WHERE network = $1 AND 'REACTIVATED' = ANY(reasons)) AS react,
                (SELECT max(started_at) FROM solprint.radar_cycles WHERE network = $1) AS last_cycle,
                (SELECT max(finished_at) FROM solprint.radar_cycles WHERE network = $1 AND status IN ('COMPLETED', 'PARTIAL')) AS last_done,
                (SELECT count(*) FROM solprint.radar_cycles WHERE network = $1 AND started_at >= now() - interval '24 hours') AS cycles24",
        )
        .bind(NETWORK)
        .fetch_one(self.pool())
        .await
        .map_err(|x| e(&x))?;
        Ok(RadarCounts {
            wallets_total: r.get("wallets"),
            wallets_by_lifecycle: pairs(lifecycle),
            wallets_by_eligibility: pairs(eligibility),
            wallets_new_24h: r.get("new24"),
            evidence_total: r.get("evidence"),
            evidence_invalidated: r.get("invalidated"),
            markets_total: r.get("markets"),
            markets_by_state: pairs(markets),
            pools_total: r.get("pools"),
            pools_supported: r.get("pools_ok"),
            monitoring_active: pairs(monitoring),
            reactivations: r.get("react"),
            last_cycle_at: opt_utc(r.get("last_cycle")),
            last_completed_cycle_at: opt_utc(r.get("last_done")),
            cycles_24h: r.get("cycles24"),
        })
    }

    /// Bounded retention. Never deletes wallets, evidence or transitions;
    /// keeps every wallet's latest snapshot.
    pub async fn radar_retention(
        &self,
        observation_days: u32,
        snapshot_days: u32,
        job_days: u32,
        consumers: &[String],
    ) -> Result<RadarRetentionOutcome, StorageError> {
        let days = |d: u32| i32::try_from(d).unwrap_or(i32::MAX);
        let observations_deleted = sqlx::query(
            "DELETE FROM solprint.radar_market_observations WHERE observed_at < now() - make_interval(days => $1)",
        )
        .bind(days(observation_days))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?
        .rows_affected();
        let snapshots_deleted = sqlx::query(
            "DELETE FROM solprint.radar_wallet_snapshots s WHERE s.computed_at < now() - make_interval(days => $1)
               AND s.snapshot_id <> (SELECT x.snapshot_id FROM solprint.radar_wallet_snapshots x
                                      WHERE x.network = s.network AND x.address = s.address
                                      ORDER BY x.computed_at DESC, x.snapshot_id DESC LIMIT 1)",
        )
        .bind(days(snapshot_days))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?
        .rows_affected();
        let jobs_deleted = sqlx::query(
            "DELETE FROM solprint.radar_jobs WHERE state IN ('COMPLETED', 'CANCELLED', 'FAILED')
               AND completed_at < now() - make_interval(days => $1)
               AND NOT EXISTS (SELECT 1 FROM solprint.radar_wallet_evidence ev WHERE ev.job_id = radar_jobs.job_id)",
        )
        .bind(days(job_days))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?
        .rows_affected();
        let events_deleted = sqlx::query(
            "DELETE FROM solprint.radar_outbox o WHERE o.persisted_at < now() - make_interval(days => $1)
               AND (SELECT count(*) FROM solprint.radar_outbox_deliveries d
                     WHERE d.event_id = o.event_id AND d.consumer = ANY($2)) = cardinality($2)",
        )
        .bind(days(job_days))
        .bind(consumers)
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?
        .rows_affected();
        let cycles_deleted = sqlx::query(
            "DELETE FROM solprint.radar_cycles c WHERE c.started_at < now() - make_interval(days => $1)
               AND NOT EXISTS (SELECT 1 FROM solprint.radar_market_observations o WHERE o.cycle_id = c.cycle_id)
               AND NOT EXISTS (SELECT 1 FROM solprint.radar_jobs j WHERE j.cycle_id = c.cycle_id)",
        )
        .bind(days(observation_days))
        .execute(self.pool())
        .await
        .map_err(|x| e(&x))?
        .rows_affected();
        Ok(RadarRetentionOutcome {
            observations_deleted,
            snapshots_deleted,
            jobs_deleted,
            events_deleted,
            cycles_deleted,
        })
    }
}
