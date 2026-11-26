//! Point-in-time view of GATEWAY's local state for the monitoring API.
//!
//! Building a snapshot never contacts a provider: it only reads in-memory
//! counters (and, through separate calls, stored aggregates).

use std::time::Duration;

use solprint_core::UtcTimestamp;
use time::OffsetDateTime;

use crate::breaker::BreakerSnapshot;
use crate::budget::BudgetSnapshot;
use crate::cache::CacheSnapshot;
use crate::context::ProviderId;
use crate::gateway::{Gateway, PersistenceStatus};
use crate::health::HealthSnapshot;
use crate::metrics::{OperationStats, UsageTotals};
use crate::ratelimit::LimiterSnapshot;
use crate::scheduler::LaneSnapshot;
use crate::stream::StreamSnapshot;

/// Latency percentiles in microseconds (bucket upper bounds; `None` without samples).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Percentiles {
    pub p50_us: Option<u64>,
    pub p95_us: Option<u64>,
    pub p99_us: Option<u64>,
}

impl Percentiles {
    #[must_use]
    pub fn of(stats: &OperationStats) -> Self {
        Self {
            p50_us: stats.latency.percentile_us(500),
            p95_us: stats.latency.percentile_us(950),
            p99_us: stats.latency.percentile_us(990),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OperationSnapshot {
    pub provider: ProviderId,
    pub operation: &'static str,
    pub stats: OperationStats,
    pub latency: Percentiles,
}

#[derive(Debug, Clone)]
pub struct ProviderSnapshot {
    pub provider: ProviderId,
    pub enabled: bool,
    pub disabled_reason: Option<&'static str>,
    pub health: HealthSnapshot,
    pub breaker: BreakerSnapshot,
    pub lane: Option<LaneSnapshot>,
    pub limiters: Vec<LimiterSnapshot>,
    pub usage: UsageTotals,
    pub rollup: OperationStats,
    pub latency: Percentiles,
}

#[derive(Debug, Clone)]
pub struct SchedulerSnapshot {
    pub global_limit: usize,
    pub global_waiting: usize,
}

#[derive(Debug, Clone)]
pub struct GatewaySnapshot {
    pub generated_at: UtcTimestamp,
    pub enabled: bool,
    pub started_at: UtcTimestamp,
    pub uptime: Duration,
    pub providers: Vec<ProviderSnapshot>,
    pub operations: Vec<OperationSnapshot>,
    pub budget: Option<BudgetSnapshot>,
    pub cache: CacheSnapshot,
    pub scheduler: SchedulerSnapshot,
    /// `None` when streams are disabled (see `streams_disabled_reason`).
    pub streams: Option<StreamSnapshot>,
    pub streams_disabled_reason: Option<&'static str>,
    pub persistence: PersistenceStatus,
}

impl Gateway {
    /// Snapshot of local state. Never contacts a provider.
    #[must_use]
    pub fn snapshot(&self) -> GatewaySnapshot {
        let now = OffsetDateTime::now_utc();
        let engine = self.engine();
        let lanes = engine.scheduler.snapshot();
        let limiters: Vec<LimiterSnapshot> = self
            .inner
            .limiters
            .all()
            .iter()
            .map(|l| l.snapshot())
            .collect();
        let budget_state = self.budget().map(|b| b.state(now));
        let providers = ProviderId::ALL
            .iter()
            .map(|&p| {
                let rt = engine.runtime(p);
                let breaker = rt.breaker.snapshot();
                let rollup = engine.metrics.provider_rollup(p);
                ProviderSnapshot {
                    provider: p,
                    enabled: rt.enabled.is_ok(),
                    disabled_reason: rt.enabled.err(),
                    health: rt.health.snapshot(
                        rt.enabled,
                        breaker.state,
                        if p == ProviderId::Helius {
                            budget_state
                        } else {
                            None
                        },
                    ),
                    breaker,
                    lane: lanes.iter().find(|l| l.provider == p).cloned(),
                    limiters: limiters
                        .iter()
                        .filter(|l| l.provider == p)
                        .cloned()
                        .collect(),
                    usage: engine.metrics.usage_totals(p),
                    latency: Percentiles::of(&rollup),
                    rollup,
                }
            })
            .collect();
        let operations = engine
            .metrics
            .operations()
            .into_iter()
            .map(|(provider, operation, stats)| OperationSnapshot {
                provider,
                operation,
                latency: Percentiles::of(&stats),
                stats,
            })
            .collect();
        let s = &self.inner.settings;
        let streams_disabled_reason = if !s.enabled {
            Some("GATEWAY_ENABLED=false")
        } else if !s.streams_enabled {
            Some("GATEWAY_STREAMS_ENABLED=false")
        } else if s.helius.api_key.is_none() {
            Some("HELIUS_API_KEY not configured")
        } else {
            None
        };
        let started = self.inner.started_at;
        let generated_at = UtcTimestamp::now();
        let uptime =
            Duration::try_from(*generated_at.as_offset_datetime() - *started.as_offset_datetime())
                .unwrap_or_default();
        GatewaySnapshot {
            generated_at,
            enabled: s.enabled,
            started_at: started,
            uptime,
            providers,
            operations,
            budget: self.budget().map(|b| b.snapshot(now)),
            cache: engine.cache.snapshot(),
            scheduler: SchedulerSnapshot {
                global_limit: engine.scheduler.global_limit(),
                global_waiting: engine.scheduler.global_waiting(),
            },
            streams: self.streams().map(crate::stream::StreamHandle::snapshot),
            streams_disabled_reason,
            persistence: self.persistence(),
        }
    }
}
