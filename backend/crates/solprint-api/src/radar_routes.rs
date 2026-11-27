//! `/api/v1/radar/*`: read-only RADAR views.
//!
//! Handlers read PostgreSQL, GATEWAY's in-memory budget snapshot and RADAR's
//! in-process counters only. None of them contacts a provider or schedules
//! work: discovery cycles, investigations, exclusions and reactivation are
//! owner operations through the `solprint radar` CLI.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use solprint_core::{UtcTimestamp, WalletAddress};
use solprint_radar::budget;
use solprint_radar::model::{
    Bucket, CLASSIFICATION_RULESET, LIFECYCLE_POLICY, PRIORITY_POLICY, Stage,
};
use solprint_storage::radar::{
    CycleRow, EvidenceRow, JobCounts, JobFilter, JobRow, MarketFilter, MarketRow, MonitoringRow,
    PoolRow, WalletFilter, WalletRow, WalletSort,
};

use crate::contracts::data::LabelCount;
use crate::contracts::radar::{
    RadarBudgetDto, RadarCycleDto, RadarCyclesResponse, RadarEventDto, RadarEventsResponse,
    RadarEvidenceDto, RadarEvidenceResponse, RadarHistoryResponse, RadarJobDto, RadarJobTotals,
    RadarJobsResponse, RadarMarketDto, RadarMarketTotals, RadarMarketsResponse, RadarMonitoringDto,
    RadarMonitoringResponse, RadarPoolDto, RadarProcessMetrics, RadarRegistryTotals,
    RadarRetrievalDto, RadarSettingsDto, RadarSnapshotDto, RadarSnapshotsResponse, RadarSourceDto,
    RadarSourcesResponse, RadarStageDto, RadarStatus, RadarTransitionDto, RadarWalletDetail,
    RadarWalletDto, RadarWalletMetricsDto, RadarWalletsResponse,
};
use crate::state::AppState;

type ApiResult<T> = Result<Json<T>, StatusCode>;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

fn ts(t: UtcTimestamp) -> String {
    t.to_rfc3339()
}

fn ots(t: Option<UtcTimestamp>) -> Option<String> {
    t.map(ts)
}

/// The instant `d` before now.
fn ago(d: time::Duration) -> UtcTimestamp {
    UtcTimestamp::from_offset_datetime(*UtcTimestamp::now().as_offset_datetime() - d)
}

fn u32s<T: TryInto<u32>>(v: T) -> u32 {
    v.try_into().unwrap_or(u32::MAX)
}

/// Non-negative signed count to `u32`.
fn n32(v: impl Into<i64>) -> u32 {
    u32s(v.into().max(0))
}

fn unavailable<E: std::fmt::Display>(e: E) -> StatusCode {
    tracing::warn!(target: "solprint::api::radar", error = %e, "radar read failed");
    StatusCode::SERVICE_UNAVAILABLE
}

fn label_counts(v: &[(String, i64)]) -> Vec<LabelCount> {
    v.iter()
        .map(|(label, count)| LabelCount {
            label: label.clone(),
            count: n32(*count),
        })
        .collect()
}

fn enum_param(v: Option<&String>) -> Result<Option<String>, StatusCode> {
    match v {
        None => Ok(None),
        Some(s)
            if !s.is_empty()
                && s.len() <= 40
                && s.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') =>
        {
            Ok(Some(s.clone()))
        }
        Some(_) => Err(StatusCode::BAD_REQUEST),
    }
}

fn address(s: &str) -> Result<String, StatusCode> {
    WalletAddress::parse(s)
        .map(|_| s.to_owned())
        .map_err(|_| StatusCode::BAD_REQUEST)
}

fn address_param(v: Option<&String>) -> Result<Option<String>, StatusCode> {
    v.map(|s| address(s)).transpose()
}

fn decimal_param(v: Option<&String>) -> Result<Option<String>, StatusCode> {
    match v {
        None => Ok(None),
        Some(s)
            if !s.is_empty()
                && s.len() <= 12
                && s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                && s.bytes().filter(|b| *b == b'.').count() <= 1 =>
        {
            Ok(Some(s.clone()))
        }
        Some(_) => Err(StatusCode::BAD_REQUEST),
    }
}

fn limit(v: Option<i64>) -> i64 {
    v.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// Keyset cursor `"{key}|{address}"`; the key is a decimal or an RFC 3339
/// timestamp produced by the previous page.
fn cursor(v: Option<&String>) -> Result<Option<(String, String)>, StatusCode> {
    let Some(c) = v else { return Ok(None) };
    let (key, addr) = c.split_once('|').ok_or(StatusCode::BAD_REQUEST)?;
    let key_ok = !key.is_empty()
        && key.len() <= 40
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'+'));
    if !key_ok {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Some((key.to_owned(), address(addr)?)))
}

fn timestamp_cursor(v: Option<&String>) -> Result<Option<(UtcTimestamp, String)>, StatusCode> {
    let Some((key, addr)) = cursor(v)? else {
        return Ok(None);
    };
    let t = UtcTimestamp::parse_rfc3339(&key).map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Some((t, addr)))
}

// ---- mapping -----------------------------------------------------------------------------

fn wallet_dto(w: &WalletRow) -> RadarWalletDto {
    RadarWalletDto {
        address: w.address.clone(),
        lifecycle: w.lifecycle.clone(),
        eligibility: w.eligibility.clone(),
        eligibility_reasons: w.eligibility_reasons.clone(),
        qualification: w.qualification.clone(),
        qualification_reasons: w.qualification_reasons.clone(),
        priority_score: w.priority_score.clone(),
        priority_policy: w.priority_policy_version.clone(),
        history_stage: w.history_stage.clone(),
        coverage_state: w.history_coverage_state.clone(),
        coverage_start: ots(w.history_coverage_start),
        coverage_end: ots(w.history_coverage_end),
        monitoring_mode: w.monitoring_mode.clone(),
        first_discovery_source: w.first_discovery_source.clone(),
        first_discovered_at: ts(w.first_discovered_at),
        last_verified_trade_at: ots(w.last_verified_trade_at),
        last_investigated_at: ots(w.last_investigated_at),
        last_evaluated_at: ots(w.last_evaluated_at),
        next_investigation_at: ots(w.next_investigation_at),
        evidence: n32(w.discovery_evidence_count),
        valid_evidence: n32(w.valid_evidence_count),
        verified_swaps: n32(w.verified_swap_count),
        distinct_tokens: n32(w.distinct_token_count),
        operator_excluded: w.operator_excluded,
        revision: n32(w.revision),
    }
}

fn job_dto(j: &JobRow) -> RadarJobDto {
    RadarJobDto {
        job_id: j.job_id.to_string(),
        work_class: j.work_class.clone(),
        subject_kind: j.subject_kind.clone(),
        subject: j.subject.clone(),
        origin: j.origin.clone(),
        bucket: j.bucket.clone(),
        priority: n32(j.priority),
        state: j.state.clone(),
        defer_reason: j.defer_reason.clone(),
        result_code: j.result_code.clone(),
        signature_limit: n32(j.signature_limit),
        lookback_days: j.lookback_days.map(n32),
        estimated_credits: n32(j.estimated_credits),
        credits_spent: n32(j.credits_spent),
        signatures_found: n32(j.signatures_found),
        signatures_reused: n32(j.signatures_reused),
        attempts: n32(j.attempts),
        backfill_status: j.backfill_status.clone(),
        next_run_at: ts(j.next_run_at),
        created_at: ts(j.created_at),
        started_at: ots(j.started_at),
        completed_at: ots(j.completed_at),
    }
}

fn job_totals(c: &JobCounts) -> RadarJobTotals {
    RadarJobTotals {
        open: n32(c.open_total),
        by_state: label_counts(&c.by_state),
        open_by_class: label_counts(&c.open_by_class),
        deferred_by_reason: label_counts(&c.deferred_by_reason),
        oldest_pending_at: ots(c.oldest_pending_at),
    }
}

fn cycle_dto(c: &CycleRow) -> RadarCycleDto {
    RadarCycleDto {
        cycle_id: c.cycle_id.to_string(),
        trigger: c.trigger.clone(),
        status: c.status.clone(),
        budget_state: c.budget_state.clone(),
        deferred_reason: c.deferred_reason.clone(),
        observations: n32(c.observations),
        markets_new: n32(c.markets_new),
        markets_selected: n32(c.markets_selected),
        samples_scheduled: n32(c.samples_scheduled),
        credits_estimated: n32(c.credits_estimated),
        started_at: ts(c.started_at),
        finished_at: ots(c.finished_at),
    }
}

fn monitoring_dto(m: &MonitoringRow) -> RadarMonitoringDto {
    RadarMonitoringDto {
        address: m.address.clone(),
        mode: m.mode.clone(),
        data_engine_target: m.target_id.map(|t| t.to_string()),
        priority_at_start: m.priority_at_start.clone(),
        started_at: ts(m.started_at),
        ended_at: ots(m.ended_at),
        end_reason: m.end_reason.clone(),
    }
}

fn evidence_dto(e: &EvidenceRow) -> RadarEvidenceDto {
    RadarEvidenceDto {
        swap_id: e.swap_id.clone(),
        signature: e.signature.clone(),
        slot: e.slot,
        block_time: ots(e.block_time),
        token_mint: e.token_mint.clone(),
        quote_mint: e.quote_mint.clone(),
        pool: e.pool.clone(),
        protocol: e.protocol.clone(),
        economic_role: e.economic_role.clone(),
        attribution: e.attribution.clone(),
        finality: e.finality.clone(),
        discovery_source: e.discovery_source.clone(),
        swap_revision: n32(e.swap_revision),
        valid: e.valid,
        invalidation_reason: e.invalidation_reason.clone(),
        invalidated_at: ots(e.invalidated_at),
        discovered_at: ts(e.discovered_at),
    }
}

fn pool_dto(p: &PoolRow) -> RadarPoolDto {
    RadarPoolDto {
        pool: p.pool.clone(),
        quote_mint: p.quote_mint.clone(),
        dex_id: p.dex_id.clone(),
        validation: p.validation.clone(),
        protocol: p.protocol.clone(),
        liquidity_usd: p.liquidity_usd.clone(),
        volume_24h_usd: p.volume_24h_usd.clone(),
        txns_1h: p.txns_1h.map(n32),
        txns_24h: p.txns_24h.map(n32),
        observed_at: ts(p.last_observed_at),
        last_sampled_at: ots(p.last_sampled_at),
        samples_completed: n32(p.samples_completed),
    }
}

fn market_dto(m: &MarketRow, pools: &[PoolRow], max_age: std::time::Duration) -> RadarMarketDto {
    let now = *UtcTimestamp::now().as_offset_datetime();
    let stale = m
        .last_market_observation_at
        .is_none_or(|t| now - *t.as_offset_datetime() > max_age);
    RadarMarketDto {
        mint: m.mint.clone(),
        symbol: m.display_symbol.clone(),
        name: m.display_name.clone(),
        classification: m.classification.clone(),
        classification_reasons: m.classification_reasons.clone(),
        bucket: m.bucket.clone(),
        sources: m.sources.clone(),
        promotional_only: m.promotional_only,
        selection_state: m.selection_state.clone(),
        selection_reasons: m.selection_reasons.clone(),
        activity_state: m.activity_state.clone(),
        market_priority: m.market_priority.clone(),
        coverage_state: m.coverage_state.clone(),
        samples_completed: n32(m.samples_completed),
        wallets_found: n32(m.wallets_found),
        first_discovered_at: ts(m.first_discovered_at),
        last_observed_at: ots(m.last_market_observation_at),
        stale,
        last_sampled_at: ots(m.last_sampled_at),
        pools: pools
            .iter()
            .filter(|p| p.mint == m.mint)
            .map(pool_dto)
            .collect(),
    }
}

async fn budget_dto(state: &AppState) -> Result<RadarBudgetDto, StatusCode> {
    let db = state.database();
    let cfg = state.radar_config();
    let outstanding =
        u64::try_from(db.radar_open_estimates().await.map_err(unavailable)?.max(0)).unwrap_or(0);
    let view = solprint_radar::feed::budget_view(state.gateway());
    let adm = budget::admission(view.as_ref(), cfg.normal_budget_share_bps, outstanding);
    let since = ago(time::Duration::hours(24));
    let spent = db
        .radar_credits_spent_since(&since)
        .await
        .map_err(unavailable)?;
    let by_class = db
        .radar_credits_by_class(&since)
        .await
        .map_err(unavailable)?;
    let reason = adm.reason().map(str::to_owned).or_else(|| {
        (adm.mode == budget::Mode::Conservation).then(|| "BUDGET_CONSERVATION".to_owned())
    });
    Ok(RadarBudgetDto {
        gateway_state: view
            .as_ref()
            .map_or_else(|| "UNKNOWN".to_owned(), |v| v.state.clone()),
        ledger_loaded: view.as_ref().is_some_and(|v| v.ledger_loaded),
        mode: adm.mode_label().to_owned(),
        reason,
        share_bps: cfg.normal_budget_share_bps,
        allowance: adm.allowance,
        available: adm.available,
        outstanding_estimates: outstanding,
        spent_24h: u64::try_from(spent.max(0)).unwrap_or(0),
        spent_24h_by_class: label_counts(&by_class),
    })
}

fn settings_dto(state: &AppState) -> RadarSettingsDto {
    let c = state.radar_config();
    let stage = |s: Stage, days: u32, signatures: u32| RadarStageDto {
        stage: s.as_str().to_owned(),
        days,
        signatures,
    };
    let shares = [
        c.share_established_bps,
        c.share_emerging_bps,
        c.share_local_activity_bps,
        c.share_historical_bps,
        c.share_exploration_bps,
    ];
    RadarSettingsDto {
        market_discovery_interval_secs: u32s(c.market_discovery_interval.as_secs()),
        max_market_samples_per_cycle: c.max_market_samples_per_cycle,
        max_concurrent_sample_jobs: c.max_concurrent_sample_jobs,
        max_concurrent_history_jobs: c.max_concurrent_history_jobs,
        max_investigating_candidates: c.max_investigating_candidates,
        max_live_monitored_wallets: c.max_live_monitored_wallets,
        initial_sample_signatures: c.initial_sample_signatures,
        history_stages: vec![
            stage(
                Stage::A,
                c.history_stage_a_days,
                c.history_stage_a_signatures,
            ),
            stage(
                Stage::B,
                c.history_stage_b_days,
                c.history_stage_b_signatures,
            ),
            stage(
                Stage::C,
                c.history_stage_c_days,
                c.history_stage_c_signatures,
            ),
        ],
        bucket_shares_bps: Bucket::ALL
            .iter()
            .zip(shares)
            .map(|(b, s)| LabelCount {
                label: b.as_str().to_owned(),
                count: s,
            })
            .collect(),
        dormant_after_days: c.dormant_after_days,
        archive_after_days: c.archive_after_days,
        classification_ruleset: CLASSIFICATION_RULESET.to_owned(),
        priority_policy: PRIORITY_POLICY.to_owned(),
        lifecycle_policy: LIFECYCLE_POLICY.to_owned(),
    }
}

fn process_dto(state: &AppState) -> Option<RadarProcessMetrics> {
    state.radar().map(|r| {
        let m = r.metrics();
        RadarProcessMetrics {
            cycles: m.cycles,
            cycles_deferred: m.cycles_deferred,
            source_failures: m.source_failures,
            provider_requests: m.provider_requests,
            events_processed: m.events_processed,
            event_failures: m.event_failures,
            evidence_recorded: m.evidence_recorded,
            evidence_invalidated: m.evidence_invalidated,
            wallets_discovered: m.wallets_discovered,
            evaluations: m.evaluations,
            evaluation_conflicts: m.evaluation_conflicts,
            jobs_started: m.jobs_started,
            jobs_completed: m.jobs_completed,
            jobs_deferred: m.jobs_deferred,
            monitoring_changes: m.monitoring_changes,
            errors: m.errors,
            last_cycle_at: ots(m.last.cycle_at),
            last_event_at: ots(m.last.event_at),
            last_evaluation_at: ots(m.last.evaluation_at),
            last_error_code: m.last.error_code,
            last_error_at: ots(m.last.error_at),
        }
    })
}

// ---- handlers ----------------------------------------------------------------------------

pub async fn status(State(state): State<AppState>) -> ApiResult<RadarStatus> {
    let db = state.database();
    let control = db.radar_control().await.map_err(unavailable)?;
    let counts = db.radar_counts().await.map_err(unavailable)?;
    let jobs = db.radar_job_counts().await.map_err(unavailable)?;
    let last_cycle = db
        .radar_cycles(1)
        .await
        .map_err(unavailable)?
        .first()
        .map(cycle_dto);
    let cfg = state.radar_config();
    Ok(Json(RadarStatus {
        enabled: cfg.enabled,
        running: state.radar().is_some(),
        auto_discovery_enabled: cfg.auto_discovery_enabled,
        data_engine_running: state.data_engine().is_some(),
        live_monitoring_available: state
            .radar()
            .is_some_and(solprint_radar::RadarInfo::live_monitoring_available),
        paused: control.paused,
        pause_reason: control.pause_reason,
        cycle_requested: control.cycle_requested,
        budget: budget_dto(&state).await?,
        registry: RadarRegistryTotals {
            wallets: n32(counts.wallets_total),
            wallets_new_24h: n32(counts.wallets_new_24h),
            by_lifecycle: label_counts(&counts.wallets_by_lifecycle),
            by_eligibility: label_counts(&counts.wallets_by_eligibility),
            evidence: n32(counts.evidence_total),
            evidence_invalidated: n32(counts.evidence_invalidated),
            reactivations: n32(counts.reactivations),
        },
        markets: RadarMarketTotals {
            markets: n32(counts.markets_total),
            by_selection: label_counts(&counts.markets_by_state),
            pools: n32(counts.pools_total),
            pools_supported: n32(counts.pools_supported),
        },
        jobs: job_totals(&jobs),
        monitoring_active: label_counts(&counts.monitoring_active),
        last_cycle,
        last_completed_cycle_at: ots(counts.last_completed_cycle_at),
        cycles_24h: n32(counts.cycles_24h),
        process: process_dto(&state),
        settings: settings_dto(&state),
        timestamp: ts(UtcTimestamp::now()),
    }))
}

pub async fn metrics(State(state): State<AppState>) -> ApiResult<Option<RadarProcessMetrics>> {
    Ok(Json(process_dto(&state)))
}

pub async fn budget(State(state): State<AppState>) -> ApiResult<RadarBudgetDto> {
    Ok(Json(budget_dto(&state).await?))
}

#[derive(Debug, Deserialize)]
pub struct WalletQuery {
    lifecycle: Option<String>,
    eligibility: Option<String>,
    source: Option<String>,
    monitoring: Option<String>,
    coverage: Option<String>,
    qualification: Option<String>,
    min_priority: Option<String>,
    max_priority: Option<String>,
    /// Only wallets with a verified trade in the last N days (1–365).
    active_days: Option<u32>,
    /// `PRIORITY` (default), `DISCOVERED` or `LAST_TRADE`.
    sort: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

pub async fn wallets(
    State(state): State<AppState>,
    Query(q): Query<WalletQuery>,
) -> ApiResult<RadarWalletsResponse> {
    let sort = match enum_param(q.sort.as_ref())?.as_deref() {
        None | Some("PRIORITY") => WalletSort::Priority,
        Some("DISCOVERED") => WalletSort::Discovered,
        Some("LAST_TRADE") => WalletSort::LastTrade,
        Some(_) => return Err(StatusCode::BAD_REQUEST),
    };
    let active_since = match q.active_days {
        None => None,
        Some(d) if (1..=365).contains(&d) => Some(ago(time::Duration::days(i64::from(d)))),
        Some(_) => return Err(StatusCode::BAD_REQUEST),
    };
    let lim = limit(q.limit);
    let f = WalletFilter {
        lifecycle: enum_param(q.lifecycle.as_ref())?,
        eligibility: enum_param(q.eligibility.as_ref())?,
        source: enum_param(q.source.as_ref())?,
        monitoring_mode: enum_param(q.monitoring.as_ref())?,
        coverage_state: enum_param(q.coverage.as_ref())?,
        qualification: enum_param(q.qualification.as_ref())?,
        min_priority: decimal_param(q.min_priority.as_ref())?,
        max_priority: decimal_param(q.max_priority.as_ref())?,
        active_since,
        sort,
        after: cursor(q.cursor.as_ref())?,
        limit: lim,
    };
    let rows = state
        .database()
        .radar_wallets(&f)
        .await
        .map_err(unavailable)?;
    let next_cursor = (i64::try_from(rows.len()).unwrap_or(i64::MAX) == lim)
        .then(|| rows.last())
        .flatten()
        .map(|w| {
            let key = match sort {
                WalletSort::Priority => w.priority_score.clone().unwrap_or_else(|| "-1".into()),
                WalletSort::Discovered => ts(w.first_discovered_at),
                WalletSort::LastTrade => w
                    .last_verified_trade_at
                    .map_or_else(|| "-infinity".to_owned(), ts),
            };
            format!("{key}|{}", w.address)
        });
    Ok(Json(RadarWalletsResponse {
        items: rows.iter().map(wallet_dto).collect(),
        next_cursor,
        sort: match sort {
            WalletSort::Priority => "PRIORITY",
            WalletSort::Discovered => "DISCOVERED",
            WalletSort::LastTrade => "LAST_TRADE",
        }
        .to_owned(),
    }))
}

pub async fn wallet(
    State(state): State<AppState>,
    Path(addr): Path<String>,
) -> ApiResult<RadarWalletDetail> {
    let addr = address(&addr)?;
    let db = state.database();
    let w = db
        .radar_wallet(&addr)
        .await
        .map_err(unavailable)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let sources = db.radar_wallet_sources(&addr).await.map_err(unavailable)?;
    let snap = db
        .radar_wallet_snapshots(&addr, 1)
        .await
        .map_err(unavailable)?;
    let metrics = snap
        .first()
        .and_then(|s| serde_json::from_str::<RadarWalletMetricsDto>(&s.metrics_json).ok());
    let r = db
        .radar_wallet_retrieval(&addr)
        .await
        .map_err(unavailable)?;
    let monitoring = db
        .radar_monitoring(false, 500)
        .await
        .map_err(unavailable)?
        .iter()
        .filter(|m| m.address == addr)
        .take(20)
        .map(monitoring_dto)
        .collect();
    let jobs = db
        .radar_jobs(&JobFilter {
            subject: Some(addr.clone()),
            limit: 20,
            ..JobFilter::default()
        })
        .await
        .map_err(unavailable)?;
    Ok(Json(RadarWalletDetail {
        wallet: wallet_dto(&w),
        discovery_sources: sources,
        metrics,
        snapshot_policy: snap.first().map(|s| s.policy_version.clone()),
        retrieval: RadarRetrievalDto {
            signatures_discovered: n32(r.discovered),
            transactions_retrieved: n32(r.transactions_retrieved),
            transactions_pending: n32(r.transactions_pending),
            transactions_unavailable: n32(r.transactions_unavailable),
            credits_spent: n32(r.credits_spent),
            history_exhausted: r.history_exhausted,
        },
        monitoring,
        recent_jobs: jobs.iter().map(job_dto).collect(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct LimitQuery {
    limit: Option<i64>,
}

pub async fn wallet_evidence(
    State(state): State<AppState>,
    Path(addr): Path<String>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<RadarEvidenceResponse> {
    let addr = address(&addr)?;
    let rows = state
        .database()
        .radar_wallet_evidence(&addr, limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(RadarEvidenceResponse {
        address: addr,
        items: rows.iter().map(evidence_dto).collect(),
    }))
}

pub async fn wallet_history(
    State(state): State<AppState>,
    Path(addr): Path<String>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<RadarHistoryResponse> {
    let addr = address(&addr)?;
    let rows = state
        .database()
        .radar_wallet_transitions(&addr, limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(RadarHistoryResponse {
        address: addr,
        items: rows
            .into_iter()
            .map(|t| RadarTransitionDto {
                from_state: t.from_state,
                to_state: t.to_state,
                trigger: t.trigger,
                reasons: t.reasons,
                policy: t.policy_version,
                transitioned_at: ts(t.transitioned_at),
            })
            .collect(),
    }))
}

pub async fn wallet_snapshots(
    State(state): State<AppState>,
    Path(addr): Path<String>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<RadarSnapshotsResponse> {
    let addr = address(&addr)?;
    let rows = state
        .database()
        .radar_wallet_snapshots(&addr, limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(RadarSnapshotsResponse {
        address: addr,
        items: rows
            .into_iter()
            .map(|s| RadarSnapshotDto {
                policy: s.policy_version,
                input_hash: s.input_hash,
                priority_score: s.priority_score,
                qualification: s.qualification,
                computed_at: ts(s.computed_at),
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct MarketQuery {
    selection: Option<String>,
    bucket: Option<String>,
    classification: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

pub async fn markets(
    State(state): State<AppState>,
    Query(q): Query<MarketQuery>,
) -> ApiResult<RadarMarketsResponse> {
    let lim = limit(q.limit);
    let f = MarketFilter {
        selection_state: enum_param(q.selection.as_ref())?,
        bucket: enum_param(q.bucket.as_ref())?,
        classification: enum_param(q.classification.as_ref())?,
        before: timestamp_cursor(q.cursor.as_ref())?,
        limit: lim,
    };
    let db = state.database();
    let rows = db.radar_markets(&f).await.map_err(unavailable)?;
    let mints: Vec<String> = rows.iter().map(|m| m.mint.clone()).collect();
    let pools = db
        .radar_pools_for_mints(&mints)
        .await
        .map_err(unavailable)?;
    let max_age = state.radar_config().max_market_observation_age;
    let next_cursor = (i64::try_from(rows.len()).unwrap_or(i64::MAX) == lim)
        .then(|| rows.last())
        .flatten()
        .map(|m| format!("{}|{}", ts(m.first_discovered_at), m.mint));
    Ok(Json(RadarMarketsResponse {
        items: rows
            .iter()
            .map(|m| market_dto(m, &pools, max_age))
            .collect(),
        next_cursor,
    }))
}

#[derive(Debug, Deserialize)]
pub struct SourcesQuery {
    /// Window in hours (1–720, default 24).
    window_hours: Option<u32>,
}

pub async fn sources(
    State(state): State<AppState>,
    Query(q): Query<SourcesQuery>,
) -> ApiResult<RadarSourcesResponse> {
    let hours = q.window_hours.unwrap_or(24);
    if !(1..=720).contains(&hours) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let since = ago(time::Duration::hours(i64::from(hours)));
    let db = state.database();
    let runs = db.radar_source_metrics(&since).await.map_err(unavailable)?;
    let yields = db.radar_source_yield(&since).await.map_err(unavailable)?;
    let mut names: Vec<String> = runs.iter().map(|r| r.source.clone()).collect();
    for y in &yields {
        if !names.contains(&y.source) {
            names.push(y.source.clone());
        }
    }
    names.sort();
    let items = names
        .into_iter()
        .map(|name| {
            let r = runs.iter().find(|r| r.source == name);
            let y = yields.iter().find(|y| y.source == name);
            let rv = |f: fn(&solprint_storage::radar::SourceMetricsRow) -> i64| {
                r.map_or(0, |r| n32(f(r)))
            };
            let yv =
                |f: fn(&solprint_storage::radar::SourceYieldRow) -> i64| y.map_or(0, |y| n32(f(y)));
            RadarSourceDto {
                runs: rv(|r| r.runs),
                runs_ok: rv(|r| r.runs_ok),
                runs_deferred: rv(|r| r.runs_deferred),
                runs_failed: rv(|r| r.runs_failed),
                items: rv(|r| r.items),
                markets_new: rv(|r| r.markets_new),
                provider_requests: rv(|r| r.provider_requests),
                credits_estimated: rv(|r| r.credits_estimated),
                last_run_at: r.and_then(|r| ots(r.last_run_at)),
                last_status: r.and_then(|r| r.last_status.clone()),
                evidence: yv(|y| y.evidence),
                unique_wallets: yv(|y| y.unique_wallets),
                eligible_wallets: yv(|y| y.eligible_wallets),
                investigated_wallets: yv(|y| y.investigated_wallets),
                qualified_wallets: yv(|y| y.qualified_wallets),
                source: name,
            }
        })
        .collect();
    Ok(Json(RadarSourcesResponse {
        window_hours: hours,
        items,
    }))
}

#[derive(Debug, Deserialize)]
pub struct JobQuery {
    state: Option<String>,
    work_class: Option<String>,
    subject: Option<String>,
    limit: Option<i64>,
}

pub async fn jobs(
    State(state): State<AppState>,
    Query(q): Query<JobQuery>,
) -> ApiResult<RadarJobsResponse> {
    let db = state.database();
    let f = JobFilter {
        state: enum_param(q.state.as_ref())?,
        work_class: enum_param(q.work_class.as_ref())?,
        subject: address_param(q.subject.as_ref())?,
        limit: limit(q.limit),
    };
    let rows = db.radar_jobs(&f).await.map_err(unavailable)?;
    let counts = db.radar_job_counts().await.map_err(unavailable)?;
    Ok(Json(RadarJobsResponse {
        totals: job_totals(&counts),
        items: rows.iter().map(job_dto).collect(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct MonitoringQuery {
    /// Include ended assignments (default: active only).
    include_ended: Option<bool>,
    limit: Option<i64>,
}

pub async fn monitoring(
    State(state): State<AppState>,
    Query(q): Query<MonitoringQuery>,
) -> ApiResult<RadarMonitoringResponse> {
    let rows = state
        .database()
        .radar_monitoring(!q.include_ended.unwrap_or(false), limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(RadarMonitoringResponse {
        capacity: state.radar_config().max_live_monitored_wallets,
        live_available: state
            .radar()
            .is_some_and(solprint_radar::RadarInfo::live_monitoring_available),
        items: rows.iter().map(monitoring_dto).collect(),
    }))
}

pub async fn cycles(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<RadarCyclesResponse> {
    let rows = state
        .database()
        .radar_cycles(limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(RadarCyclesResponse {
        items: rows.iter().map(cycle_dto).collect(),
    }))
}

pub async fn events(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<RadarEventsResponse> {
    let rows = state
        .database()
        .radar_events_recent(limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(RadarEventsResponse {
        items: rows
            .into_iter()
            .map(|e| RadarEventDto {
                event_id: e.event_id.to_string(),
                seq: e.seq,
                event_type: e.event_type,
                aggregate_id: e.aggregate_id,
                aggregate_revision: n32(e.aggregate_revision),
                source: e.source,
                observed_at: ts(e.observed_at),
                persisted_at: ts(e.persisted_at),
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_and_params_are_validated() {
        let a = "So11111111111111111111111111111111111111112".to_owned();
        assert!(cursor(Some(&format!("42.5|{a}"))).is_ok_and(|c| c.is_some()));
        assert!(cursor(Some(&format!("-infinity|{a}"))).is_ok());
        assert!(cursor(Some(&"42|not-an-address".to_owned())).is_err());
        assert!(cursor(Some(&format!("4'2|{a}"))).is_err());
        assert!(enum_param(Some(&"MONITORED".to_owned())).is_ok());
        assert!(enum_param(Some(&"monitored".to_owned())).is_err());
        assert!(decimal_param(Some(&"12.5".to_owned())).is_ok());
        assert!(decimal_param(Some(&"1.2.3".to_owned())).is_err());
        assert!(decimal_param(Some(&"-1".to_owned())).is_err());
    }

    #[test]
    fn stored_metrics_shape_parses() {
        let m = serde_json::json!({
            "policy": "radar-priority-1", "computedAt": "2026-11-27T00:00:00.000Z",
            "priority": {"score": "50", "activity": "0.5", "recency": "1", "evidenceQuality": "0.4",
                          "evidenceParts": ["1"], "performance": "0.2", "performanceStatus": "SHRUNK", "diversity": "0.1"},
            "activity": [{"window": "24h", "swaps": 1, "buys": 1, "sells": 0, "distinctTokens": 1, "tradingDays": 1, "undated": 0}],
            "screening": {"outcomes": 0, "disposals": 0, "disposalsMatched": 0, "matchedCoverageBps": 0, "openPositions": 1,
                          "unevaluatedCycles": 0, "ambiguousTokens": 0, "unknownCostTokens": 0, "byDenomination": []},
            "coverage": {"state": "PARTIAL_HISTORY", "signaturesDiscovered": 1, "transactionsRetrieved": 1, "transactionsUnavailable": 0,
                         "transactionsVerified": 1, "transactionsUnsupported": 0, "stageCompleted": "A", "swapsAnalysed": 1, "swapLimit": 2000},
            "evidence": {"total": 1, "valid": 1, "validFinalized": 1, "distinctTokens": 1}
        });
        let parsed: Result<RadarWalletMetricsDto, _> = serde_json::from_value(m);
        let out = parsed.map(|p| serde_json::to_value(p).unwrap_or_default());
        assert!(out.is_ok_and(|v| v["priority"]["evidence_quality"] == "0.4"));
    }
}
