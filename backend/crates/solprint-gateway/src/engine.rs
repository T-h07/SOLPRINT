//! The request pipeline every provider call goes through.
//!
//! ```text
//! cache ─hit─▶ return FRESH_CACHE
//!   │miss
//!   ▼
//! single-flight ─follower─▶ share leader's result
//!   │leader
//!   ▼
//! ┌ attempt loop (bounded retries, never past the deadline) ───────────┐
//! │ scheduler permit → rate limiter → circuit breaker → budget reserve │
//! │ → transport → classify → health / metrics / usage                  │
//! └────────────────────────────────────────────────────────────────────┘
//!   │ok → cache put → return DIRECT
//!   │err (+ AllowStaleOnError and a stale entry) → return STALE_CACHE
//! ```

use std::any::Any;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use solprint_core::UtcTimestamp;
use time::OffsetDateTime;
use tokio::time::Instant;

use crate::breaker::CircuitBreaker;
use crate::budget::CreditBudget;
use crate::cache::{CacheEntry, CacheTtl, Lookup, ResponseCache};
use crate::coalesce::{Shared, SingleFlight};
use crate::context::{
    CachePolicy, Freshness, GatewayResponse, Priority, Provenance, ProviderId, RequestContext,
};
use crate::error::GatewayError;
use crate::health::HealthTracker;
use crate::metrics::{Metrics, Outcome};
use crate::ratelimit::RateLimiter;
use crate::scheduler::Scheduler;

/// Retries after the first attempt, at most.
pub const MAX_RETRIES: u32 = 2;
const BACKOFF_BASE: Duration = Duration::from_millis(200);
const BACKOFF_CAP: Duration = Duration::from_secs(2);

/// What a provider fetch produced.
#[derive(Debug)]
pub struct Fetched<T> {
    /// `None` when the provider answered "does not exist".
    pub value: Option<T>,
    pub context_slot: Option<u64>,
    pub source_timestamp: Option<UtcTimestamp>,
}

impl<T> Fetched<T> {
    pub const fn found(value: T) -> Self {
        Self {
            value: Some(value),
            context_slot: None,
            source_timestamp: None,
        }
    }

    pub const fn not_found() -> Self {
        Self {
            value: None,
            context_slot: None,
            source_timestamp: None,
        }
    }

    #[must_use]
    pub const fn at_slot(mut self, slot: Option<u64>) -> Self {
        self.context_slot = slot;
        self
    }

    #[must_use]
    pub const fn with_source_time(mut self, ts: Option<UtcTimestamp>) -> Self {
        self.source_timestamp = ts;
        self
    }
}

/// Static description of one call.
#[derive(Debug, Clone)]
pub struct Call {
    pub provider: ProviderId,
    pub operation: &'static str,
    /// Usage category (normally the operation).
    pub category: &'static str,
    /// `None` disables caching and coalescing.
    pub cache_key: Option<String>,
    pub ttl: CacheTtl,
    /// Budget credits for one attempt (Helius only; 0 elsewhere).
    pub credits: u64,
    /// Whether the provider supplies a source timestamp or slot.
    pub source_age_known: bool,
    /// Result came from the fallback provider.
    pub fallback: bool,
}

/// Per-provider runtime components.
#[derive(Debug)]
pub struct ProviderRuntime {
    pub id: ProviderId,
    pub enabled: Result<(), &'static str>,
    pub breaker: CircuitBreaker,
    pub health: HealthTracker,
    pub budget: Option<CreditBudget>,
}

#[derive(Debug)]
pub struct Engine {
    pub providers: [ProviderRuntime; 4],
    pub scheduler: Scheduler,
    pub cache: ResponseCache,
    pub flights: SingleFlight,
    pub metrics: Metrics,
    rng: AtomicU64,
}

/// The value type stored in cache and shared between coalesced callers.
type Stored<T> = (
    Option<T>,
    Option<u64>,
    Option<UtcTimestamp>,
    UtcTimestamp,
    u64,
);

impl Engine {
    #[must_use]
    pub fn new(
        providers: [ProviderRuntime; 4],
        scheduler: Scheduler,
        cache: ResponseCache,
    ) -> Self {
        let seed = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() & u128::from(u64::MAX)),
        )
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self {
            providers,
            scheduler,
            cache,
            flights: SingleFlight::default(),
            metrics: Metrics::default(),
            rng: AtomicU64::new(seed | 1),
        }
    }

    #[must_use]
    pub fn runtime(&self, p: ProviderId) -> &ProviderRuntime {
        &self.providers[p.index()]
    }

    /// Uniform jitter in `[0, max]` from a lock-free xorshift generator.
    /// Not cryptographic; only spreads retries.
    pub fn jitter(&self, max: Duration) -> Duration {
        let mut x = self.rng.load(Ordering::Relaxed);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.store(x, Ordering::Relaxed);
        let max_ms = u64::try_from(max.as_millis()).unwrap_or(u64::MAX);
        if max_ms == 0 {
            return Duration::ZERO;
        }
        Duration::from_millis(x % (max_ms + 1))
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let exp = BACKOFF_BASE.saturating_mul(1_u32 << attempt.min(4));
        let base = exp.min(BACKOFF_CAP);
        base / 2 + self.jitter(base / 2)
    }

    /// Run `call`, using `fetch` for each upstream attempt.
    pub async fn execute<T, F, Fut>(
        &self,
        ctx: &RequestContext,
        limiter: &RateLimiter,
        call: Call,
        fetch: F,
    ) -> Result<GatewayResponse<Option<T>>, GatewayError>
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(Duration) -> Fut,
        Fut: Future<Output = Result<Fetched<T>, GatewayError>>,
    {
        let started = Instant::now();
        if ctx.cancel.is_cancelled() {
            return Err(GatewayError::Cancelled);
        }
        if ctx.remaining().is_zero() {
            return Err(GatewayError::DeadlineExceeded);
        }
        let rt = self.runtime(call.provider);
        if let Err(reason) = rt.enabled {
            return Err(GatewayError::Disabled {
                provider: call.provider,
                reason,
            });
        }

        // 1. Cache.
        let mut stale: Option<Arc<CacheEntry>> = None;
        if let Some(key) = &call.cache_key
            && ctx.cache != CachePolicy::Refresh
            && let Lookup::Hit(entry) = self.cache.get(call.operation, key).await
        {
            let usable_fresh = entry.satisfies(ctx.max_data_age);
            if usable_fresh || ctx.cache == CachePolicy::CacheOnly {
                self.cache.note_hit(call.operation, &entry);
                self.metrics.record(
                    call.provider,
                    call.operation,
                    Outcome::CacheHit,
                    started.elapsed(),
                );
                return from_entry::<T>(&entry, ctx, &call);
            }
            stale = Some(entry);
        }
        if ctx.cache == CachePolicy::CacheOnly {
            return Err(GatewayError::Unavailable);
        }

        // 2. Single flight (only for cacheable, i.e. idempotent and keyed, calls).
        let run = || async {
            self.attempts(ctx, limiter, &call, &fetch)
                .await
                .map(|s| Arc::new(s) as Shared)
        };
        let (result, coalesced) = match &call.cache_key {
            Some(key) => {
                let flight_key = format!("{}|{key}", call.operation);
                self.flights.run(&flight_key, run).await
            }
            None => (run().await, false),
        };
        if coalesced {
            self.metrics.note_coalesced(call.provider, call.operation);
        }

        match result {
            Ok(shared) => {
                let stored = shared
                    .downcast::<Stored<T>>()
                    .map_err(|_| GatewayError::Decode {
                        provider: call.provider,
                        detail: "internal type mismatch",
                    })?;
                let (value, slot, source_ts, observed_at, credits) = (*stored).clone();
                Ok(GatewayResponse {
                    value,
                    provider: call.provider,
                    provenance: if call.fallback {
                        Provenance::Fallback
                    } else {
                        Provenance::Direct
                    },
                    freshness: if call.source_age_known {
                        Freshness::Fresh
                    } else {
                        Freshness::UnknownSourceAge
                    },
                    observed_at,
                    source_timestamp: source_ts,
                    context_slot: slot,
                    age: Duration::ZERO,
                    correlation_id: ctx.correlation_id,
                    credits_charged: if coalesced { 0 } else { credits },
                })
            }
            Err(err) => {
                if ctx.cache == CachePolicy::AllowStaleOnError
                    && let Some(entry) = stale
                    && entry.value.is_some()
                    && entry.freshness() != Freshness::Expired
                {
                    self.cache.note_hit(call.operation, &entry);
                    tracing::debug!(
                        provider = %call.provider,
                        operation = call.operation,
                        error = err.code(),
                        "serving stale cache after provider error"
                    );
                    return from_entry::<T>(&entry, ctx, &call);
                }
                Err(err)
            }
        }
    }

    async fn attempts<T, F, Fut>(
        &self,
        ctx: &RequestContext,
        limiter: &RateLimiter,
        call: &Call,
        fetch: &F,
    ) -> Result<Stored<T>, GatewayError>
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(Duration) -> Fut,
        Fut: Future<Output = Result<Fetched<T>, GatewayError>>,
    {
        let rt = self.runtime(call.provider);
        let max_retries = if ctx.priority == Priority::P5 {
            0
        } else {
            MAX_RETRIES
        };
        let mut attempt = 0_u32;
        loop {
            let attempt_started = Instant::now();
            let result = self.attempt_once(ctx, limiter, call, rt, fetch).await;
            match result {
                Ok((fetched, credits)) => {
                    let observed_at = UtcTimestamp::now();
                    rt.health.success();
                    self.metrics.record(
                        call.provider,
                        call.operation,
                        Outcome::Success,
                        attempt_started.elapsed(),
                    );
                    if let Some(key) = &call.cache_key {
                        self.store(call, key.clone(), &fetched, observed_at).await;
                    }
                    return Ok((
                        fetched.value,
                        fetched.context_slot,
                        fetched.source_timestamp,
                        observed_at,
                        credits,
                    ));
                }
                Err(err) => {
                    self.metrics.record(
                        call.provider,
                        call.operation,
                        Outcome::Error(err.code()),
                        attempt_started.elapsed(),
                    );
                    if let GatewayError::RateLimited { retry_after, .. } = &err
                        && err.was_dispatched()
                    {
                        rt.health.rate_limited();
                        limiter.block_for(retry_after.unwrap_or(Duration::from_secs(1)));
                    } else if err.is_provider_failure() {
                        rt.health.failure(err.code());
                    }
                    let retryable = err.is_retryable() && attempt < max_retries;
                    let wait = match &err {
                        GatewayError::RateLimited {
                            retry_after: Some(r),
                            ..
                        } => *r,
                        _ => self.backoff(attempt),
                    };
                    if !retryable || wait >= ctx.remaining() || ctx.cancel.is_cancelled() {
                        return Err(err);
                    }
                    attempt += 1;
                    self.metrics.note_retry(call.provider, call.operation);
                    tokio::select! {
                        () = tokio::time::sleep(wait) => {}
                        () = ctx.cancel.cancelled() => return Err(GatewayError::Cancelled),
                    }
                }
            }
        }
    }

    async fn attempt_once<T, F, Fut>(
        &self,
        ctx: &RequestContext,
        limiter: &RateLimiter,
        call: &Call,
        rt: &ProviderRuntime,
        fetch: &F,
    ) -> Result<(Fetched<T>, u64), GatewayError>
    where
        F: Fn(Duration) -> Fut,
        Fut: Future<Output = Result<Fetched<T>, GatewayError>>,
    {
        let _permit = self
            .scheduler
            .acquire(call.provider, ctx.priority, ctx.deadline, &ctx.cancel)
            .await?;
        limiter.acquire(ctx.deadline, &ctx.cancel).await?;
        let ticket = rt.breaker.check()?;
        let reservation = match (&rt.budget, call.credits) {
            (Some(b), c) if c > 0 => {
                Some(b.try_reserve(ctx.priority, c, OffsetDateTime::now_utc())?)
            }
            _ => None,
        };
        let timeout = ctx.remaining();
        if timeout.is_zero() {
            if let (Some(b), Some(r)) = (&rt.budget, reservation) {
                b.refund(r);
            }
            return Err(GatewayError::DeadlineExceeded);
        }
        let outcome = tokio::select! {
            r = fetch(timeout) => r,
            () = ctx.cancel.cancelled() => Err(GatewayError::Cancelled),
        };
        let credits = reservation.map_or(0, |r| r.credits);
        let dispatched = match &outcome {
            Ok(_) => true,
            Err(e) => e.was_dispatched(),
        };
        if !dispatched && let (Some(b), Some(r)) = (&rt.budget, reservation) {
            b.refund(r);
        }
        if dispatched {
            self.metrics
                .record_usage(call.provider, call.category, 1, credits, 0);
        }
        match &outcome {
            Ok(_) => rt.breaker.record(ticket, Ok(())),
            Err(e) => rt.breaker.record(ticket, Err(e)),
        }
        outcome.map(|f| (f, if dispatched { credits } else { 0 }))
    }

    async fn store<T>(
        &self,
        call: &Call,
        key: String,
        fetched: &Fetched<T>,
        observed_at: UtcTimestamp,
    ) where
        T: Clone + Send + Sync + 'static,
    {
        let (fresh_for, keep_for) = match fetched.value {
            Some(_) => (call.ttl.fresh_for, call.ttl.keep_for),
            None => (call.ttl.negative_for, call.ttl.negative_for),
        };
        if keep_for.is_zero() {
            return;
        }
        let value = fetched
            .value
            .clone()
            .map(|v| Arc::new(v) as Arc<dyn Any + Send + Sync>);
        self.cache
            .put(
                call.operation,
                key,
                CacheEntry {
                    value,
                    provider: call.provider,
                    stored_at: Instant::now(),
                    observed_at,
                    source_timestamp: fetched.source_timestamp,
                    context_slot: fetched.context_slot,
                    fresh_for,
                    keep_for,
                },
            )
            .await;
    }
}

fn from_entry<T: Clone + Send + Sync + 'static>(
    entry: &CacheEntry,
    ctx: &RequestContext,
    call: &Call,
) -> Result<GatewayResponse<Option<T>>, GatewayError> {
    let value = match &entry.value {
        None => None,
        Some(v) => Some(
            v.clone()
                .downcast::<T>()
                .map_err(|_| GatewayError::Decode {
                    provider: call.provider,
                    detail: "cached type mismatch",
                })?
                .as_ref()
                .clone(),
        ),
    };
    let freshness = entry.freshness();
    Ok(GatewayResponse {
        value,
        provider: entry.provider,
        provenance: if freshness == Freshness::Fresh {
            Provenance::FreshCache
        } else {
            Provenance::StaleCache
        },
        freshness: if freshness == Freshness::Fresh && !call.source_age_known {
            Freshness::UnknownSourceAge
        } else {
            freshness
        },
        observed_at: entry.observed_at,
        source_timestamp: entry.source_timestamp,
        context_slot: entry.context_slot,
        age: entry.age(),
        correlation_id: ctx.correlation_id,
        credits_charged: 0,
    })
}
