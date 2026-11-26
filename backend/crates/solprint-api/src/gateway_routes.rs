//! `/api/v1/gateway/*`: GATEWAY monitoring.
//!
//! Every handler reads GATEWAY's in-memory snapshot or stored aggregates.
//! None of them contacts a provider, so refreshing a dashboard never costs
//! credits or rate-limit budget.

use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use solprint_core::UtcTimestamp;
use solprint_gateway::breaker::CircuitState;
use solprint_gateway::budget::BudgetState;
use solprint_gateway::health::HealthState;
use solprint_gateway::snapshot::{GatewaySnapshot, Percentiles, ProviderSnapshot};
use solprint_gateway::stream::{ConnectionState, Continuity};
use solprint_gateway::{Priority, ProviderId as GwProvider};
use time::OffsetDateTime;

use crate::contracts::gateway::{
    BudgetDto, BudgetStateDto, BudgetSummary, CacheNamespaceDto, CacheSummary, CheckpointDto,
    CircuitDto, CircuitStateDto, ContinuityDto, ErrorCountDto, GatewayBudgetsResponse,
    GatewayCacheResponse, GatewayHealthState, GatewayMetricsResponse, GatewayProvider,
    GatewayProviderSummary, GatewayProvidersResponse, GatewayStatus, GatewayStreamsResponse,
    GatewayUsageResponse, HealthDto, LaneDto, LatencyDto, LimiterDto, OperationMetricsDto,
    PersistenceDto, PriorityCountDto, PriorityDto, ProviderUsageDto, SchedulerDto,
    StreamConnectionDto, StreamStateDto, StreamSummary, SubscriptionDto, TrafficDto, UsageHourDto,
};
use crate::contracts::{ExecutionCapability, ProviderId};
use crate::state::AppState;

pub(crate) const fn provider_id(p: GwProvider) -> ProviderId {
    match p {
        GwProvider::Helius => ProviderId::Helius,
        GwProvider::Jupiter => ProviderId::Jupiter,
        GwProvider::DexScreener => ProviderId::DexScreener,
        GwProvider::SolanaRpc => ProviderId::SolanaRpc,
    }
}

fn provider_from_str(s: &str) -> Option<ProviderId> {
    GwProvider::ALL
        .iter()
        .find(|p| p.as_str() == s)
        .map(|p| provider_id(*p))
}

const fn provider_name(p: GwProvider) -> &'static str {
    match p {
        GwProvider::Helius => "Helius",
        GwProvider::Jupiter => "Jupiter",
        GwProvider::DexScreener => "DEX Screener",
        GwProvider::SolanaRpc => "Solana RPC",
    }
}

pub(crate) const fn health_state(h: HealthState) -> GatewayHealthState {
    match h {
        HealthState::Disabled => GatewayHealthState::Disabled,
        HealthState::Unknown => GatewayHealthState::Unknown,
        HealthState::Healthy => GatewayHealthState::Healthy,
        HealthState::Degraded => GatewayHealthState::Degraded,
        HealthState::Unavailable => GatewayHealthState::Unavailable,
        HealthState::BudgetExhausted => GatewayHealthState::BudgetExhausted,
    }
}

const fn circuit_state(c: CircuitState) -> CircuitStateDto {
    match c {
        CircuitState::Closed => CircuitStateDto::Closed,
        CircuitState::Open => CircuitStateDto::Open,
        CircuitState::HalfOpen => CircuitStateDto::HalfOpen,
    }
}

const fn budget_state(b: BudgetState) -> BudgetStateDto {
    match b {
        BudgetState::Normal => BudgetStateDto::Normal,
        BudgetState::Conservation => BudgetStateDto::Conservation,
        BudgetState::Critical => BudgetStateDto::Critical,
        BudgetState::NormalBudgetExhausted => BudgetStateDto::NormalBudgetExhausted,
        BudgetState::HardLimitReached => BudgetStateDto::HardLimitReached,
        BudgetState::AccountUsageUnknown => BudgetStateDto::AccountUsageUnknown,
    }
}

const fn priority(p: Priority) -> PriorityDto {
    match p {
        Priority::P0 => PriorityDto::P0,
        Priority::P1 => PriorityDto::P1,
        Priority::P2 => PriorityDto::P2,
        Priority::P3 => PriorityDto::P3,
        Priority::P4 => PriorityDto::P4,
        Priority::P5 => PriorityDto::P5,
    }
}

const PRIORITIES: [PriorityDto; 6] = [
    PriorityDto::P0,
    PriorityDto::P1,
    PriorityDto::P2,
    PriorityDto::P3,
    PriorityDto::P4,
    PriorityDto::P5,
];

const fn stream_state(s: ConnectionState) -> StreamStateDto {
    match s {
        ConnectionState::Disconnected => StreamStateDto::Disconnected,
        ConnectionState::Connecting => StreamStateDto::Connecting,
        ConnectionState::Subscribing => StreamStateDto::Subscribing,
        ConnectionState::Connected => StreamStateDto::Connected,
        ConnectionState::Degraded => StreamStateDto::Degraded,
        ConnectionState::Reconnecting => StreamStateDto::Reconnecting,
        ConnectionState::Disabled => StreamStateDto::Disabled,
        ConnectionState::BudgetExhausted => StreamStateDto::BudgetExhausted,
        ConnectionState::ShuttingDown => StreamStateDto::ShuttingDown,
    }
}

const fn continuity(c: Continuity) -> ContinuityDto {
    match c {
        Continuity::Continuous => ContinuityDto::Continuous,
        Continuity::Recovered => ContinuityDto::Recovered,
        Continuity::Gap => ContinuityDto::Gap,
        Continuity::Unknown => ContinuityDto::Unknown,
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn ts(t: UtcTimestamp) -> String {
    t.to_rfc3339()
}

const fn latency(p: Percentiles) -> LatencyDto {
    LatencyDto {
        p50_us: p.p50_us,
        p95_us: p.p95_us,
        p99_us: p.p99_us,
    }
}

fn provider_detail(p: &ProviderSnapshot) -> GatewayProvider {
    GatewayProvider {
        id: provider_id(p.provider),
        name: provider_name(p.provider).to_owned(),
        enabled: p.enabled,
        disabled_reason: p.disabled_reason.map(str::to_owned),
        health: HealthDto {
            state: health_state(p.health.state),
            reason: p.health.reason.map(str::to_owned),
            last_success_at: p.health.last_success_at.map(ts),
            last_failure_at: p.health.last_failure_at.map(ts),
            last_error_code: p.health.last_error_code.map(str::to_owned),
            recent_failures: p.health.recent_failures,
            recent_total: p.health.recent_total,
        },
        circuit: CircuitDto {
            state: circuit_state(p.breaker.state),
            consecutive_failures: p.breaker.consecutive_failures,
            retry_in_ms: p.breaker.retry_in.map(ms),
            times_opened: p.breaker.times_opened,
        },
        lane: p.lane.map(|l| LaneDto {
            max_in_flight: u32_of(l.max_in_flight),
            queue_limit: u32_of(l.queue_limit),
            in_flight: u32_of(l.in_flight),
            queued: u32_of(l.queued),
            peak_in_flight: u32_of(l.peak_in_flight),
            admitted: l.admitted,
            queued_total: l.queued_total,
            rejected: l.rejected,
            timed_out: l.timed_out,
        }),
        limiters: p
            .limiters
            .iter()
            .map(|l| LimiterDto {
                group: l.group.to_owned(),
                rate_count: l.rate.count,
                rate_period_ms: ms(l.rate.period),
                burst: l.rate.burst,
                admitted: l.admitted,
                delayed: l.delayed,
                rejected: l.rejected,
                upstream_limited: l.upstream_limited,
                blocked_for_ms: l.blocked_for.map(ms),
            })
            .collect(),
        traffic: TrafficDto {
            requests: p.usage.requests,
            credits: p.usage.credits,
            bytes: p.usage.bytes,
            success: p.rollup.success,
            errors: p.rollup.errors,
            cache_hits: p.rollup.cache_hits,
            coalesced: p.rollup.coalesced,
            retries: p.rollup.retries,
            fallbacks: p.rollup.fallbacks,
        },
        latency: latency(p.latency),
    }
}

fn budget_detail(
    s: &GatewaySnapshot,
    policy: &solprint_gateway::budget::BudgetPolicy,
) -> Option<BudgetDto> {
    let b = s.budget.as_ref()?;
    Some(BudgetDto {
        provider: provider_id(b.provider),
        state: budget_state(b.state),
        admits_up_to: b.admits_up_to.map(priority),
        cycle_start: ts(b.cycle_start),
        cycle_end: ts(b.cycle_end),
        credit_limit: b.credit_limit,
        reserve_credits: b.reserve_credits,
        normal_credits: b.normal_credits,
        conservation_at: policy.conservation_at(),
        critical_at: policy.critical_at(),
        credits_used: b.credits_used,
        credits_pending_persist: b.credits_pending_persist,
        credits_remaining: b.credits_remaining,
        projected_cycle_credits: b.projected_cycle_credits,
        stream_bytes: b.stream_bytes,
        stream_credits: b.stream_credits,
        denied_by_priority: PRIORITIES
            .iter()
            .zip(b.denied_by_priority)
            .map(|(&priority, count)| PriorityCountDto { priority, count })
            .collect(),
        ledger_loaded: b.ledger_loaded,
        last_persisted_at: b.last_persisted_at.map(ts),
        persist_failures: b.persist_failures,
    })
}

pub(crate) async fn status(State(state): State<AppState>) -> Json<GatewayStatus> {
    let s = state.gateway().snapshot();
    Json(GatewayStatus {
        enabled: s.enabled,
        execution_capability: ExecutionCapability::None,
        generated_at: ts(s.generated_at),
        started_at: ts(s.started_at),
        uptime_seconds: s.uptime.as_secs(),
        providers: s
            .providers
            .iter()
            .map(|p| GatewayProviderSummary {
                id: provider_id(p.provider),
                name: provider_name(p.provider).to_owned(),
                enabled: p.enabled,
                health: health_state(p.health.state),
                circuit: circuit_state(p.breaker.state),
            })
            .collect(),
        budget: s.budget.as_ref().map(|b| BudgetSummary {
            provider: provider_id(b.provider),
            state: budget_state(b.state),
            credits_used: b.credits_used,
            credit_limit: b.credit_limit,
            credits_remaining: b.credits_remaining,
        }),
        streams: StreamSummary {
            enabled: s.streams.is_some(),
            disabled_reason: s.streams_disabled_reason.map(str::to_owned),
            state: s.streams.as_ref().map(|st| stream_state(st.state)),
            subscriptions: s
                .streams
                .as_ref()
                .map_or(0, |st| u32_of(st.subscriptions.len())),
        },
        persistence: PersistenceDto {
            ledger_loaded: s.persistence.ledger_loaded,
            last_flush_at: s.persistence.last_flush_at.map(ts),
            last_flush_ok: s.persistence.last_flush_ok,
            flush_failures: s.persistence.flush_failures,
            last_prune_at: s.persistence.last_prune_at.map(ts),
        },
        scheduler: SchedulerDto {
            global_limit: u32_of(s.scheduler.global_limit),
            global_waiting: u32_of(s.scheduler.global_waiting),
        },
        cache: CacheSummary {
            entries: s.cache.entries,
            max_entries: s.cache.max_entries,
        },
    })
}

pub(crate) async fn providers(State(state): State<AppState>) -> Json<GatewayProvidersResponse> {
    let s = state.gateway().snapshot();
    Json(GatewayProvidersResponse {
        generated_at: ts(s.generated_at),
        providers: s.providers.iter().map(provider_detail).collect(),
    })
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageQuery {
    hours: Option<u16>,
}

pub(crate) async fn usage(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Json<GatewayUsageResponse> {
    let hours = q.hours.unwrap_or(24).clamp(1, 168);
    let since_odt = OffsetDateTime::now_utc() - time::Duration::hours(i64::from(hours));
    let since = UtcTimestamp::from_offset_datetime(since_odt);
    let s = state.gateway().snapshot();
    let stored = state.gateway().usage_history(&since).await;
    let history_available = stored.is_ok();
    let history = stored
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| {
            Some(UsageHourDto {
                provider: provider_from_str(&r.provider)?,
                hour_start: ts(r.hour_start),
                requests: r.requests,
                credits: r.credits,
                bytes: r.bytes,
            })
        })
        .collect();
    Json(GatewayUsageResponse {
        generated_at: ts(s.generated_at),
        since: ts(since),
        process: s
            .providers
            .iter()
            .map(|p| ProviderUsageDto {
                provider: provider_id(p.provider),
                requests: p.usage.requests,
                credits: p.usage.credits,
                bytes: p.usage.bytes,
            })
            .collect(),
        history,
        history_available,
    })
}

pub(crate) async fn metrics(State(state): State<AppState>) -> Json<GatewayMetricsResponse> {
    let s = state.gateway().snapshot();
    Json(GatewayMetricsResponse {
        generated_at: ts(s.generated_at),
        operations: s
            .operations
            .iter()
            .map(|o| OperationMetricsDto {
                provider: provider_id(o.provider),
                operation: o.operation.to_owned(),
                success: o.stats.success,
                errors: o.stats.errors,
                cache_hits: o.stats.cache_hits,
                coalesced: o.stats.coalesced,
                retries: o.stats.retries,
                fallbacks: o.stats.fallbacks,
                latency: latency(o.latency),
                errors_by_code: o
                    .stats
                    .errors_by_code
                    .iter()
                    .map(|(code, count)| ErrorCountDto {
                        code: (*code).to_owned(),
                        count: *count,
                    })
                    .collect(),
                last_error_code: o.stats.last_error.map(|(c, _)| c.to_owned()),
                last_error_at: o.stats.last_error.map(|(_, t)| ts(t)),
            })
            .collect(),
    })
}

pub(crate) async fn budgets(State(state): State<AppState>) -> Json<GatewayBudgetsResponse> {
    let s = state.gateway().snapshot();
    let policy = state.gateway().settings().helius.budget;
    Json(GatewayBudgetsResponse {
        generated_at: ts(s.generated_at),
        budgets: budget_detail(&s, &policy).into_iter().collect(),
    })
}

pub(crate) async fn streams(State(state): State<AppState>) -> Json<GatewayStreamsResponse> {
    let s = state.gateway().snapshot();
    let stored = state.gateway().stored_checkpoints().await;
    let checkpoints_available = stored.is_ok();
    let checkpoints = stored
        .unwrap_or_default()
        .into_iter()
        .map(|c| CheckpointDto {
            stream_key: c.stream_key,
            kind: c.kind,
            commitment: c.commitment,
            last_slot: c.last_slot,
            last_signature: c.last_signature,
            continuity: c.continuity,
            updated_at: ts(c.updated_at),
        })
        .collect();
    let (connection, subscriptions) = match &s.streams {
        None => (None, Vec::new()),
        Some(st) => (
            Some(StreamConnectionDto {
                state: stream_state(st.state),
                generation: st.generation,
                connected_since: st.connected_since.map(ts),
                last_message_at: st.last_message_at.map(ts),
                reconnects: st.reconnects,
                bytes_received: st.bytes_received,
                credits_charged: st.credits_charged,
                events_emitted: st.events_emitted,
                events_dropped: st.events_dropped,
                gaps: st.gaps,
                recovered: st.recovered,
                duplicates_suppressed: st.duplicates_suppressed,
                last_error: st.last_error.map(str::to_owned),
                max_subscriptions: u32_of(st.max_subscriptions),
            }),
            st.subscriptions
                .iter()
                .map(|sub| SubscriptionDto {
                    id: sub.id.0,
                    kind: sub.kind.to_owned(),
                    commitment: sub
                        .commitment
                        .map(|c| solprint_gateway::rpc::commitment_str(c).to_owned()),
                    durable_key: sub.durable_key.clone(),
                    active: sub.active,
                    notifications: sub.notifications,
                    dropped: sub.dropped,
                    continuity: continuity(sub.continuity),
                    last_slot: sub.last_slot,
                })
                .collect(),
        ),
    };
    Json(GatewayStreamsResponse {
        generated_at: ts(s.generated_at),
        enabled: s.streams.is_some(),
        disabled_reason: s.streams_disabled_reason.map(str::to_owned),
        connection,
        subscriptions,
        checkpoints,
        checkpoints_available,
    })
}

pub(crate) async fn cache(State(state): State<AppState>) -> Json<GatewayCacheResponse> {
    let s = state.gateway().snapshot();
    Json(GatewayCacheResponse {
        generated_at: ts(s.generated_at),
        entries: s.cache.entries,
        max_entries: s.cache.max_entries,
        namespaces: s
            .cache
            .namespaces
            .iter()
            .map(|(ns, st)| CacheNamespaceDto {
                namespace: (*ns).to_owned(),
                hits: st.hits,
                stale_hits: st.stale_hits,
                negative_hits: st.negative_hits,
                misses: st.misses,
                inserts: st.inserts,
            })
            .collect(),
    })
}
