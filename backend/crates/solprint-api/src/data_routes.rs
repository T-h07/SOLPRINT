//! `/api/v1/data/*`: read-only DATA ENGINE views.
//!
//! Handlers read PostgreSQL and the engine's in-process counters only. None
//! of them contacts a provider or enqueues work: ingestion, backfills and
//! reprocessing are owner operations through the `solprint data-engine` CLI.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use solprint_core::{TransactionSignature, UtcTimestamp, WalletAddress};
use solprint_data_engine::{DECODER_VERSION, Registry};
use solprint_storage::data_engine::{
    ActivityRow, BalanceWrite, DeBackfill, DeCounts, DeTarget, EvidenceMetaRow, IssueRow,
    SwapFilter, SwapRow, TxFilter, TxSummaryRow,
};
use uuid::Uuid;

use crate::contracts::ExecutionCapability;
use crate::contracts::data::{
    ActivityDto, BackfillDto, BalanceChangeDto, DataBackfillsResponse, DataDecodersResponse,
    DataEngineSettingsDto, DataEngineStatus, DataEventsResponse, DataEvidenceTotals,
    DataIssuesResponse, DataOutboxTotals, DataProcessMetrics, DataQueueDto, DataSwapTotals,
    DataSwapsResponse, DataTargetsResponse, DataTransactionDetail, DataTransactionTotals,
    DataTransactionsResponse, DecoderDto, DomainEventDto, EvidenceDto, LabelCount, QualityIssueDto,
    RouteLegDto, SwapAssetDto, SwapDto, SwapFeeDto, SwapValuationDto, TargetDto,
    TransactionSummaryDto,
};
use crate::state::AppState;

type ApiResult<T> = Result<Json<T>, StatusCode>;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

fn now() -> String {
    UtcTimestamp::now().to_rfc3339()
}

fn ts(t: UtcTimestamp) -> String {
    t.to_rfc3339()
}

fn ots(t: Option<UtcTimestamp>) -> Option<String> {
    t.map(ts)
}

fn u32s<T: TryInto<u32>>(v: T) -> u32 {
    v.try_into().unwrap_or(u32::MAX)
}

fn unavailable<E: std::fmt::Display>(e: E) -> StatusCode {
    tracing::warn!(target: "solprint::api::data", error = %e, "data read failed");
    StatusCode::SERVICE_UNAVAILABLE
}

fn label_counts(v: &[(String, i64)]) -> Vec<LabelCount> {
    v.iter()
        .map(|(label, count)| LabelCount {
            label: label.clone(),
            count: u32s((*count).max(0)),
        })
        .collect()
}

fn enum_param(v: Option<&String>) -> Result<Option<String>, StatusCode> {
    match v {
        None => Ok(None),
        Some(s)
            if !s.is_empty()
                && s.len() <= 32
                && s.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') =>
        {
            Ok(Some(s.clone()))
        }
        Some(_) => Err(StatusCode::BAD_REQUEST),
    }
}

fn address_param(v: Option<&String>) -> Result<Option<String>, StatusCode> {
    v.map(|s| {
        WalletAddress::parse(s)
            .map(|_| s.clone())
            .map_err(|_| StatusCode::BAD_REQUEST)
    })
    .transpose()
}

fn limit(v: Option<i64>) -> i64 {
    v.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// Keyset cursor `"{slot}:{id}"`.
fn cursor(v: Option<&String>) -> Result<Option<(u64, String)>, StatusCode> {
    let Some(c) = v else { return Ok(None) };
    let (slot, id) = c.split_once(':').ok_or(StatusCode::BAD_REQUEST)?;
    let slot = slot.parse::<u64>().map_err(|_| StatusCode::BAD_REQUEST)?;
    if id.is_empty()
        || id.len() > 160
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'.')
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Some((slot, id.to_owned())))
}

// ---- status ------------------------------------------------------------------------------

fn totals(
    c: &DeCounts,
    capacity: u64,
    retention_days: u32,
) -> (
    DataQueueDto,
    DataTransactionTotals,
    DataSwapTotals,
    DataOutboxTotals,
    DataEvidenceTotals,
) {
    let depth: i64 = c
        .jobs_by_state
        .iter()
        .filter(|(s, _)| matches!(s.as_str(), "PENDING" | "FETCHING" | "FETCH_RETRY"))
        .map(|(_, n)| *n)
        .sum();
    (
        DataQueueDto {
            depth: u32s(depth.max(0)),
            capacity: u32s(capacity),
            by_state: label_counts(&c.jobs_by_state),
            oldest_pending_at: ots(c.oldest_pending_job_at),
        },
        DataTransactionTotals {
            total: u32s(c.transactions.max(0)),
            by_finality: label_counts(&c.transactions_by_finality),
            by_decode_status: label_counts(&c.transactions_by_status),
            last_decoded_at: ots(c.last_decoded_at),
        },
        DataSwapTotals {
            canonical: u32s(c.swaps_canonical.max(0)),
            verified: u32s(c.swaps_verified.max(0)),
            retracted: u32s(c.swaps_retracted.max(0)),
            by_protocol: label_counts(&c.swaps_by_protocol),
        },
        DataOutboxTotals {
            events: u32s(c.outbox_events.max(0)),
            undelivered: u32s(c.outbox_undelivered.max(0)),
        },
        DataEvidenceTotals {
            bytes: u64::try_from(c.evidence_bytes.max(0)).unwrap_or(0),
            pruned: u32s(c.evidence_pruned.max(0)),
            retention_days,
        },
    )
}

pub(crate) async fn status(State(state): State<AppState>) -> Json<DataEngineStatus> {
    let cfg = state.data_engine_config();
    let engine = state.data_engine();
    // Prefer the engine's cached aggregates; fall back to one local query.
    let cached = engine.and_then(|e| e.counts());
    let (counts_at, counts) = match cached {
        Some(c) => (Some(c.at), Some(c.counts)),
        None => match state.database().de_counts().await {
            Ok(c) => (Some(UtcTimestamp::now()), Some(c)),
            Err(e) => {
                tracing::warn!(target: "solprint::api::data", error = %e, "data counts unavailable");
                (None, None)
            }
        },
    };
    let t = counts
        .as_ref()
        .map(|c| totals(c, cfg.queue_capacity, cfg.evidence_retention_days));
    let process = engine.map(|e| {
        let m = e.metrics().snapshot();
        DataProcessMetrics {
            jobs_claimed: m.jobs_claimed,
            fetched: m.fetched,
            fetched_cached: m.fetched_cached,
            persisted: m.persisted,
            unchanged: m.unchanged,
            retries: m.retries,
            unavailable: m.unavailable,
            quarantined: m.quarantined,
            budget_pauses: m.budget_pauses,
            credits_charged: m.credits_charged,
            finality_checks: m.finality_checks,
            stream_observations: m.stream_observations,
            stream_gaps: m.stream_gaps,
            backfill_pages: m.backfill_pages,
            outbox_delivered: m.outbox_delivered,
            mean_decode_us: m.decode_micros_total.checked_div(m.persisted + m.unchanged),
            last_error_code: m.last_error_code,
            last_error_at: ots(m.last_error_at),
        }
    });
    let (queue, transactions, swaps, outbox, evidence) = match t {
        Some((q, tx, s, o, ev)) => (Some(q), Some(tx), Some(s), Some(o), Some(ev)),
        None => (None, None, None, None, None),
    };
    Json(DataEngineStatus {
        running: engine.is_some(),
        execution_capability: ExecutionCapability::None,
        decoder_version: DECODER_VERSION.to_owned(),
        generated_at: now(),
        counts_at: ots(counts_at),
        counts_available: counts.is_some(),
        queue,
        transactions,
        swaps,
        open_issues_by_severity: counts
            .as_ref()
            .map(|c| label_counts(&c.open_issues_by_severity))
            .unwrap_or_default(),
        outbox,
        evidence,
        targets_enabled: counts.as_ref().map(|c| u32s(c.targets_enabled.max(0))),
        targets_total: counts.as_ref().map(|c| u32s(c.targets_total.max(0))),
        backfills_active: counts.as_ref().map(|c| u32s(c.backfills_active.max(0))),
        process,
        settings: Some(DataEngineSettingsDto {
            auto_ingest_enabled: cfg.auto_ingest_enabled,
            backfill_enabled: cfg.backfill_enabled,
            price_enrichment_enabled: cfg.price_enrichment_enabled,
            default_commitment: cfg.default_commitment.as_str().to_owned(),
            fetch_concurrency: u32s(cfg.fetch_concurrency),
            max_active_targets: u32s(cfg.max_active_targets),
        }),
    })
}

pub(crate) async fn decoders() -> Json<DataDecodersResponse> {
    let reg = Registry::standard();
    Json(DataDecodersResponse {
        decoder_version: DECODER_VERSION.to_owned(),
        network: solprint_data_engine::programs::NETWORK.to_owned(),
        decoders: reg
            .iter()
            .map(|d| DecoderDto {
                id: d.id().to_owned(),
                protocol: d.protocol().to_owned(),
                program_id: d.program_id().to_owned(),
            })
            .collect(),
    })
}

// ---- transactions ----------------------------------------------------------------------------

fn tx_summary(r: TxSummaryRow) -> TransactionSummaryDto {
    TransactionSummaryDto {
        signature: r.signature,
        slot: r.slot,
        block_time: ots(r.block_time),
        execution_status: r.execution_status,
        fee_lamports: r.fee_lamports,
        fee_payer: r.fee_payer,
        finality: r.finality,
        decode_status: r.decode_status,
        decoder_version: r.decoder_version,
        protocols: r.protocols,
        activity_kinds: r.activity_kinds,
        swap_count: u32s(r.swap_count.max(0)),
        revision: u32s(r.current_revision.max(0)),
        first_observed_at: ts(r.first_observed_at),
        decoded_at: ts(r.decoded_at),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct TxQuery {
    protocol: Option<String>,
    status: Option<String>,
    finality: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

pub(crate) async fn transactions(
    State(state): State<AppState>,
    Query(q): Query<TxQuery>,
) -> ApiResult<DataTransactionsResponse> {
    let lim = limit(q.limit);
    let f = TxFilter {
        protocol: enum_param(q.protocol.as_ref())?,
        status: enum_param(q.status.as_ref())?,
        finality: enum_param(q.finality.as_ref())?,
        before: cursor(q.cursor.as_ref())?,
        limit: lim,
    };
    let rows = state
        .database()
        .de_transactions(&f)
        .await
        .map_err(unavailable)?;
    let next_cursor = (i64::try_from(rows.len()).unwrap_or(0) == lim)
        .then(|| rows.last().map(|r| format!("{}:{}", r.slot, r.signature)))
        .flatten();
    Ok(Json(DataTransactionsResponse {
        generated_at: now(),
        items: rows.into_iter().map(tx_summary).collect(),
        next_cursor,
    }))
}

fn evidence(e: EvidenceMetaRow) -> EvidenceDto {
    EvidenceDto {
        revision: u32s(e.revision.max(0)),
        commitment: e.commitment,
        provider: e.provider,
        slot: e.slot,
        fetched_at: ts(e.fetched_at),
        content_sha256: e.content_sha256,
        byte_len: u32s(e.byte_len.max(0)),
        pruned: e.pruned,
        canonical: e.canonical,
        superseded_reason: e.superseded_reason,
    }
}

fn balance(b: BalanceWrite) -> BalanceChangeDto {
    BalanceChangeDto {
        account_index: u32s(b.account_index.max(0)),
        asset: b.asset,
        account: b.account,
        owner: b.owner,
        token_program: b.token_program,
        decimals: b.decimals.map(|d| u32s(d.max(0))),
        pre_amount: b.pre_amount,
        post_amount: b.post_amount,
        delta: b.delta,
        pre_evidence: b.pre_evidence,
        post_evidence: b.post_evidence,
        ownership_evidence: b.ownership_evidence,
    }
}

fn activity(a: ActivityRow) -> ActivityDto {
    let w = a.write;
    ActivityDto {
        activity_id: w.activity_id,
        index: u32s(w.activity_index.max(0)),
        kind: w.kind,
        status: w.status,
        protocol: w.protocol,
        program_id: w.program_id,
        instruction_ref: w.instruction_ref,
        actor: w.actor,
        attribution: w.attribution,
        mint: w.mint,
        amount: w.amount,
        source_account: w.source_account,
        destination_account: w.destination_account,
        counterparty: w.counterparty,
        reasons: w.reasons,
        decoder: w.decoder,
        canonical: a.canonical,
    }
}

fn swap(r: SwapRow, enrichment: bool) -> SwapDto {
    let w = r.write;
    SwapDto {
        swap_id: w.swap_id,
        signature: r.signature,
        slot: r.slot,
        block_time: ots(r.block_time),
        wallet: w.wallet,
        attribution: w.attribution,
        authority: w.authority,
        beneficiary: w.beneficiary,
        fee_payer: r.fee_payer,
        input: SwapAssetDto {
            mint: w.input_mint,
            amount: w.input_amount,
            decimals: w.input_decimals.map(|d| u32s(d.max(0))),
            asset_kind: w.input_asset_kind,
        },
        output: SwapAssetDto {
            mint: w.output_mint,
            amount: w.output_amount,
            decimals: w.output_decimals.map(|d| u32s(d.max(0))),
            asset_kind: w.output_asset_kind,
        },
        exchange_ratio: w.exchange_ratio,
        protocol: w.protocol,
        venue_kind: w.venue_kind,
        aggregator: w.aggregator,
        route_complete: w.route_complete,
        instruction_ref: w.instruction_ref,
        classification_status: w.classification_status,
        reasons: w.reasons,
        finality: r.finality,
        canonical: r.canonical,
        retraction_reason: r.retraction_reason,
        revision: u32s(r.revision.max(0)),
        decoder_version: r.decoder_version,
        legs: w
            .legs
            .into_iter()
            .map(|l| RouteLegDto {
                index: u32s(l.leg_index.max(0)),
                protocol: l.protocol,
                program_id: l.program_id,
                pool: l.pool,
                input_mint: l.input_mint,
                input_amount: l.input_amount,
                output_mint: l.output_mint,
                output_amount: l.output_amount,
                instruction_ref: l.instruction_ref,
                evidence: l.evidence_kind,
            })
            .collect(),
        fees: w
            .fees
            .into_iter()
            .map(|f| SwapFeeDto {
                kind: f.kind,
                mint: f.mint,
                amount: f.amount,
                provenance: f.provenance,
                recipient: f.recipient,
            })
            .collect(),
        valuation: SwapValuationDto {
            state: if enrichment {
                "PRICE_UNAVAILABLE"
            } else {
                "NOT_ENRICHED"
            }
            .to_owned(),
            usd_value: None,
            price_source: None,
            price_observed_at: None,
        },
        first_observed_at: ts(r.first_observed_at),
        decoded_at: ts(r.decoded_at),
        finalized_at: ots(r.finalized_at),
        corrected_at: ots(r.corrected_at),
    }
}

fn issue(i: IssueRow) -> QualityIssueDto {
    QualityIssueDto {
        issue_id: i.issue_id,
        signature: i.signature,
        target_id: i.target_id.map(|t| t.to_string()),
        backfill_id: i.backfill_id.map(|t| t.to_string()),
        code: i.code,
        severity: i.severity,
        detail: i.detail,
        decoder_version: i.decoder_version,
        occurrences: u32s(i.occurrences.max(0)),
        first_seen_at: ts(i.first_seen_at),
        last_seen_at: ts(i.last_seen_at),
        resolved_at: ots(i.resolved_at),
    }
}

pub(crate) async fn transaction(
    State(state): State<AppState>,
    Path(signature): Path<String>,
) -> ApiResult<DataTransactionDetail> {
    TransactionSignature::parse(&signature).map_err(|_| StatusCode::BAD_REQUEST)?;
    let d = state
        .database()
        .de_transaction(&signature)
        .await
        .map_err(unavailable)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let enrichment = state.data_engine_config().price_enrichment_enabled;
    Ok(Json(DataTransactionDetail {
        generated_at: now(),
        summary: tx_summary(d.summary),
        transaction_index: d.transaction_index.map(|i| u32s(i.max(0))),
        version: d.version,
        err_json: d.err_json,
        signers: d.signers,
        compute_units: d.compute_units,
        finality_checks: u32s(d.finality_checks.max(0)),
        finalized_at: ots(d.finalized_at),
        evidence: d.evidence.into_iter().map(evidence).collect(),
        balances: d.balances.into_iter().map(balance).collect(),
        activities: d.activities.into_iter().map(activity).collect(),
        swaps: d.swaps.into_iter().map(|s| swap(s, enrichment)).collect(),
        issues: d.issues.into_iter().map(issue).collect(),
    }))
}

// ---- swaps ------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct SwapQuery {
    wallet: Option<String>,
    mint: Option<String>,
    protocol: Option<String>,
    status: Option<String>,
    include_retracted: Option<bool>,
    verified_only: Option<bool>,
    cursor: Option<String>,
    limit: Option<i64>,
}

pub(crate) async fn swaps(
    State(state): State<AppState>,
    Query(q): Query<SwapQuery>,
) -> ApiResult<DataSwapsResponse> {
    let lim = limit(q.limit);
    let f = SwapFilter {
        wallet: address_param(q.wallet.as_ref())?,
        mint: address_param(q.mint.as_ref())?,
        protocol: enum_param(q.protocol.as_ref())?,
        status: enum_param(q.status.as_ref())?,
        include_retracted: q.include_retracted.unwrap_or(false),
        verified_only: q.verified_only.unwrap_or(false),
        before: cursor(q.cursor.as_ref())?,
        limit: lim,
    };
    let rows = state
        .database()
        .de_swaps(&f, None)
        .await
        .map_err(unavailable)?;
    let next_cursor = (i64::try_from(rows.len()).unwrap_or(0) == lim)
        .then(|| {
            rows.last()
                .map(|r| format!("{}:{}", r.slot, r.write.swap_id))
        })
        .flatten();
    let enrichment = state.data_engine_config().price_enrichment_enabled;
    Ok(Json(DataSwapsResponse {
        generated_at: now(),
        items: rows.into_iter().map(|s| swap(s, enrichment)).collect(),
        next_cursor,
    }))
}

// ---- issues, targets, backfills, events ---------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct IssueQuery {
    severity: Option<String>,
    signature: Option<String>,
    include_resolved: Option<bool>,
    limit: Option<i64>,
}

pub(crate) async fn issues(
    State(state): State<AppState>,
    Query(q): Query<IssueQuery>,
) -> ApiResult<DataIssuesResponse> {
    let severity = enum_param(q.severity.as_ref())?;
    if let Some(s) = &q.signature {
        TransactionSignature::parse(s).map_err(|_| StatusCode::BAD_REQUEST)?;
    }
    let rows = state
        .database()
        .de_issues(
            q.signature.as_deref(),
            severity.as_deref(),
            q.include_resolved.unwrap_or(false),
            limit(q.limit),
        )
        .await
        .map_err(unavailable)?;
    Ok(Json(DataIssuesResponse {
        generated_at: now(),
        items: rows.into_iter().map(issue).collect(),
    }))
}

fn target(t: DeTarget) -> TargetDto {
    TargetDto {
        target_id: t.target_id.to_string(),
        target_type: t.target_type,
        address: t.address,
        label: t.label,
        enabled: t.enabled,
        priority: t.priority,
        commitment: t.commitment,
        discovery_source: t.discovery_source,
        created_at: ts(t.created_at),
        last_observation_at: ots(t.last_observation_at),
        history_oldest_slot: t.history_oldest_slot,
        history_newest_slot: t.history_newest_slot,
        history_exhausted: t.history_exhausted,
        coverage_state: t.coverage_state,
    }
}

pub(crate) async fn targets(State(state): State<AppState>) -> ApiResult<DataTargetsResponse> {
    let rows = state.database().de_targets().await.map_err(unavailable)?;
    Ok(Json(DataTargetsResponse {
        generated_at: now(),
        max_active_targets: u32s(state.data_engine_config().max_active_targets),
        items: rows.into_iter().map(target).collect(),
    }))
}

fn backfill(b: DeBackfill) -> BackfillDto {
    let n = |v: i32| u32s(v.max(0));
    BackfillDto {
        backfill_id: b.backfill_id.to_string(),
        target_id: b.target_id.map(|t: Uuid| t.to_string()),
        address: b.address,
        commitment: b.commitment,
        status: b.status,
        stop_reason: b.stop_reason,
        coverage_state: b.coverage_state,
        cancel_requested: b.cancel_requested,
        pages_fetched: n(b.pages_fetched),
        max_pages: n(b.max_pages),
        discovered_count: n(b.discovered_count),
        queued_count: n(b.queued_count),
        duplicate_count: n(b.duplicate_count),
        max_signatures: n(b.max_signatures),
        credits_estimated: n(b.credits_estimated),
        credits_spent: n(b.credits_spent),
        min_slot: b.min_slot,
        oldest_slot_seen: b.oldest_slot_seen,
        newest_slot_seen: b.newest_slot_seen,
        history_exhausted: b.history_exhausted,
        last_error_code: b.last_error_code,
        created_at: ts(b.created_at),
        updated_at: ts(b.updated_at),
        started_at: ots(b.started_at),
        completed_at: ots(b.completed_at),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct LimitQuery {
    limit: Option<i64>,
}

pub(crate) async fn backfills(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<DataBackfillsResponse> {
    let rows = state
        .database()
        .de_backfills(limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(DataBackfillsResponse {
        generated_at: now(),
        items: rows.into_iter().map(backfill).collect(),
    }))
}

pub(crate) async fn events(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<DataEventsResponse> {
    let rows = state
        .database()
        .de_outbox_recent(limit(q.limit))
        .await
        .map_err(unavailable)?;
    Ok(Json(DataEventsResponse {
        generated_at: now(),
        items: rows
            .into_iter()
            .map(|e| DomainEventDto {
                event_id: e.event_id.to_string(),
                seq: e.seq,
                event_type: e.event_type,
                aggregate_type: e.aggregate_type,
                aggregate_id: e.aggregate_id,
                aggregate_revision: u32s(e.aggregate_revision.max(0)),
                quality_state: e.quality_state,
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
        assert_eq!(
            cursor(Some(&"12:swap:abc:3.1".to_owned())),
            Ok(Some((12, "swap:abc:3.1".to_owned())))
        );
        assert!(cursor(Some(&"x:abc".to_owned())).is_err());
        assert!(cursor(Some(&"1:a b".to_owned())).is_err());
        assert!(enum_param(Some(&"RAYDIUM_CLMM".to_owned())).is_ok());
        assert!(enum_param(Some(&"raydium'; drop".to_owned())).is_err());
        assert!(address_param(Some(&"not-an-address".to_owned())).is_err());
        assert_eq!(limit(Some(10_000)), MAX_LIMIT);
        assert_eq!(limit(Some(0)), 1);
    }
}
