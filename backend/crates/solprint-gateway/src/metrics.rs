//! Metrics with bounded cardinality.
//!
//! Every label is a `&'static str` chosen by GATEWAY code (provider,
//! operation, error code), never a runtime value such as an address or a
//! mint, so the number of series is fixed at compile time.
//!
//! Latency uses a fixed log-scale histogram. Percentiles are reported as the
//! upper bound of the bucket that contains them (an over-estimate, never an
//! under-estimate). The same layout is persisted per minute, so the API can
//! recompute percentiles over any stored window.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use solprint_core::UtcTimestamp;
use solprint_storage::{MetricDelta, UsageDelta};
use time::OffsetDateTime;

use crate::context::ProviderId;

/// Upper bounds (µs) of the latency buckets; the last bucket is open-ended.
pub const LATENCY_BOUNDS_US: [u64; 15] = [
    250, 500, 1_000, 2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000, 1_000_000,
    2_500_000, 5_000_000, 10_000_000,
];
pub const LATENCY_BUCKETS: usize = LATENCY_BOUNDS_US.len() + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Histogram {
    pub counts: [u64; LATENCY_BUCKETS],
}

impl Histogram {
    pub fn record(&mut self, latency: Duration) {
        let us = u64::try_from(latency.as_micros()).unwrap_or(u64::MAX);
        let idx = LATENCY_BOUNDS_US
            .iter()
            .position(|b| us <= *b)
            .unwrap_or(LATENCY_BUCKETS - 1);
        self.counts[idx] = self.counts[idx].saturating_add(1);
    }

    pub fn merge(&mut self, other: &[u64]) {
        for (a, b) in self.counts.iter_mut().zip(other) {
            *a = a.saturating_add(*b);
        }
    }

    #[must_use]
    pub fn total(&self) -> u64 {
        self.counts.iter().fold(0_u64, |a, b| a.saturating_add(*b))
    }

    /// Upper bound (µs) of the bucket holding the `per_mille`-th sample
    /// (500 = p50, 950 = p95, 990 = p99). `None` when empty; `u64::MAX` for
    /// the open-ended bucket.
    #[must_use]
    pub fn percentile_us(&self, per_mille: u64) -> Option<u64> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let rank = (u128::from(total) * u128::from(per_mille)).div_ceil(1_000);
        let rank = u64::try_from(rank).unwrap_or(u64::MAX).max(1);
        let mut seen = 0_u64;
        for (i, c) in self.counts.iter().enumerate() {
            seen = seen.saturating_add(*c);
            if seen >= rank {
                return Some(LATENCY_BOUNDS_US.get(i).copied().unwrap_or(u64::MAX));
            }
        }
        Some(u64::MAX)
    }
}

/// Outcome of one operation, for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    CacheHit,
    Error(&'static str),
}

#[derive(Debug, Clone, Default)]
struct MinuteDelta {
    success: u64,
    errors: u64,
    cache_hits: u64,
    hist: Histogram,
}

#[derive(Debug, Clone, Default)]
pub struct OperationStats {
    pub success: u64,
    pub errors: u64,
    pub cache_hits: u64,
    pub coalesced: u64,
    pub retries: u64,
    pub fallbacks: u64,
    pub latency: Histogram,
    pub errors_by_code: Vec<(&'static str, u64)>,
    pub last_error: Option<(&'static str, UtcTimestamp)>,
}

#[derive(Debug, Default)]
struct OpEntry {
    stats: OperationStats,
    pending: HashMap<i64, MinuteDelta>,
}

#[derive(Debug, Default)]
struct UsageEntry {
    requests: u64,
    credits: u64,
    bytes: u64,
}

#[derive(Debug, Default)]
struct Inner {
    ops: HashMap<(ProviderId, &'static str), OpEntry>,
    usage: HashMap<(ProviderId, &'static str, i64), UsageEntry>,
    usage_totals: [UsageEntry; 4],
}

/// Process-lifetime totals per provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct UsageTotals {
    pub requests: u64,
    pub credits: u64,
    pub bytes: u64,
}

#[derive(Debug, Default)]
pub struct Metrics {
    inner: Mutex<Inner>,
}

fn minute_of(now: OffsetDateTime) -> i64 {
    now.unix_timestamp().div_euclid(60) * 60
}

fn ts_of_minute(minute: i64) -> UtcTimestamp {
    OffsetDateTime::from_unix_timestamp(minute)
        .map_or_else(|_| UtcTimestamp::now(), UtcTimestamp::from_offset_datetime)
}

impl Metrics {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn record(
        &self,
        provider: ProviderId,
        operation: &'static str,
        outcome: Outcome,
        latency: Duration,
    ) {
        let minute = minute_of(OffsetDateTime::now_utc());
        let mut inner = self.lock();
        let e = inner.ops.entry((provider, operation)).or_default();
        let d = e.pending.entry(minute).or_default();
        match outcome {
            Outcome::Success => {
                e.stats.success = e.stats.success.saturating_add(1);
                e.stats.latency.record(latency);
                d.success = d.success.saturating_add(1);
                d.hist.record(latency);
            }
            Outcome::CacheHit => {
                e.stats.cache_hits = e.stats.cache_hits.saturating_add(1);
                d.cache_hits = d.cache_hits.saturating_add(1);
            }
            Outcome::Error(code) => {
                e.stats.errors = e.stats.errors.saturating_add(1);
                e.stats.latency.record(latency);
                d.errors = d.errors.saturating_add(1);
                d.hist.record(latency);
                match e.stats.errors_by_code.iter_mut().find(|(c, _)| *c == code) {
                    Some((_, n)) => *n = n.saturating_add(1),
                    None => e.stats.errors_by_code.push((code, 1)),
                }
                e.stats.last_error = Some((code, UtcTimestamp::now()));
            }
        }
    }

    pub fn note_coalesced(&self, provider: ProviderId, operation: &'static str) {
        let mut inner = self.lock();
        let e = inner.ops.entry((provider, operation)).or_default();
        e.stats.coalesced = e.stats.coalesced.saturating_add(1);
    }

    pub fn note_retry(&self, provider: ProviderId, operation: &'static str) {
        let mut inner = self.lock();
        let e = inner.ops.entry((provider, operation)).or_default();
        e.stats.retries = e.stats.retries.saturating_add(1);
    }

    pub fn note_fallback(&self, operation: &'static str) {
        let mut inner = self.lock();
        let e = inner
            .ops
            .entry((ProviderId::SolanaRpc, operation))
            .or_default();
        e.stats.fallbacks = e.stats.fallbacks.saturating_add(1);
    }

    /// Count one billable request (or stream frame batch) for usage history.
    pub fn record_usage(
        &self,
        provider: ProviderId,
        category: &'static str,
        requests: u64,
        credits: u64,
        bytes: u64,
    ) {
        let minute = minute_of(OffsetDateTime::now_utc());
        let mut inner = self.lock();
        let u = inner.usage.entry((provider, category, minute)).or_default();
        u.requests = u.requests.saturating_add(requests);
        u.credits = u.credits.saturating_add(credits);
        u.bytes = u.bytes.saturating_add(bytes);
        let t = &mut inner.usage_totals[provider.index()];
        t.requests = t.requests.saturating_add(requests);
        t.credits = t.credits.saturating_add(credits);
        t.bytes = t.bytes.saturating_add(bytes);
    }

    #[must_use]
    pub fn usage_totals(&self, provider: ProviderId) -> UsageTotals {
        let inner = self.lock();
        let t = &inner.usage_totals[provider.index()];
        UsageTotals {
            requests: t.requests,
            credits: t.credits,
            bytes: t.bytes,
        }
    }

    #[must_use]
    pub fn operations(&self) -> Vec<(ProviderId, &'static str, OperationStats)> {
        let inner = self.lock();
        let mut out: Vec<_> = inner
            .ops
            .iter()
            .map(|((p, op), e)| (*p, *op, e.stats.clone()))
            .collect();
        out.sort_by_key(|(p, op, _)| (*p, *op));
        out
    }

    #[must_use]
    pub fn provider_rollup(&self, provider: ProviderId) -> OperationStats {
        let inner = self.lock();
        let mut acc = OperationStats::default();
        for ((p, _), e) in &inner.ops {
            if *p != provider {
                continue;
            }
            acc.success = acc.success.saturating_add(e.stats.success);
            acc.errors = acc.errors.saturating_add(e.stats.errors);
            acc.cache_hits = acc.cache_hits.saturating_add(e.stats.cache_hits);
            acc.coalesced = acc.coalesced.saturating_add(e.stats.coalesced);
            acc.retries = acc.retries.saturating_add(e.stats.retries);
            acc.fallbacks = acc.fallbacks.saturating_add(e.stats.fallbacks);
            acc.latency.merge(&e.stats.latency.counts);
            if let Some(le) = e.stats.last_error
                && acc.last_error.is_none_or(|(_, t)| le.1 > t)
            {
                acc.last_error = Some(le);
            }
        }
        acc
    }

    /// Drain per-minute deltas for persistence. On failure, hand them back
    /// with [`Self::restore`].
    #[must_use]
    pub fn drain(&self) -> (Vec<UsageDelta>, Vec<MetricDelta>) {
        let mut inner = self.lock();
        let usage = inner
            .usage
            .drain()
            .map(|((p, cat, minute), u)| UsageDelta {
                provider: p.as_str().to_owned(),
                bucket_start: ts_of_minute(minute),
                category: cat.to_owned(),
                requests: u.requests,
                credits: u.credits,
                bytes: u.bytes,
            })
            .collect();
        let mut metrics = Vec::new();
        for ((p, op), e) in &mut inner.ops {
            for (minute, d) in e.pending.drain() {
                metrics.push(MetricDelta {
                    provider: p.as_str().to_owned(),
                    operation: (*op).to_owned(),
                    bucket_start: ts_of_minute(minute),
                    success_count: d.success,
                    error_count: d.errors,
                    cache_hits: d.cache_hits,
                    latency_hist: d.hist.counts.to_vec(),
                });
            }
        }
        (usage, metrics)
    }

    /// Put back deltas whose persistence failed (bounded: at most one hour
    /// of minutes per series is kept, older minutes are dropped).
    pub fn restore(&self, usage: Vec<UsageDelta>, metrics: Vec<MetricDelta>) {
        let now_minute = minute_of(OffsetDateTime::now_utc());
        let keep =
            |ts: &UtcTimestamp| now_minute - ts.as_offset_datetime().unix_timestamp() <= 3_600;
        let mut inner = self.lock();
        for u in usage.into_iter().filter(|u| keep(&u.bucket_start)) {
            let (Some(p), Some(cat)) = (provider_of(&u.provider), static_label(&u.category)) else {
                continue;
            };
            let minute = u.bucket_start.as_offset_datetime().unix_timestamp();
            let e = inner.usage.entry((p, cat, minute)).or_default();
            e.requests = e.requests.saturating_add(u.requests);
            e.credits = e.credits.saturating_add(u.credits);
            e.bytes = e.bytes.saturating_add(u.bytes);
        }
        for m in metrics.into_iter().filter(|m| keep(&m.bucket_start)) {
            let (Some(p), Some(op)) = (provider_of(&m.provider), static_label(&m.operation)) else {
                continue;
            };
            let minute = m.bucket_start.as_offset_datetime().unix_timestamp();
            let e = inner.ops.entry((p, op)).or_default();
            let d = e.pending.entry(minute).or_default();
            d.success = d.success.saturating_add(m.success_count);
            d.errors = d.errors.saturating_add(m.error_count);
            d.cache_hits = d.cache_hits.saturating_add(m.cache_hits);
            d.hist.merge(&m.latency_hist);
        }
    }
}

fn provider_of(s: &str) -> Option<ProviderId> {
    ProviderId::ALL.into_iter().find(|p| p.as_str() == s)
}

/// Map a label back to its static form. Only labels GATEWAY itself emits
/// are accepted, which keeps cardinality bounded even on restore.
fn static_label(s: &str) -> Option<&'static str> {
    crate::operations::ALL.iter().copied().find(|l| *l == s)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_report_bucket_upper_bounds() {
        let mut h = Histogram::default();
        assert_eq!(h.percentile_us(500), None);
        for _ in 0..90 {
            h.record(Duration::from_micros(800)); // ≤ 1 ms
        }
        for _ in 0..9 {
            h.record(Duration::from_millis(40)); // ≤ 50 ms
        }
        h.record(Duration::from_secs(30)); // open bucket
        assert_eq!(h.percentile_us(500), Some(1_000));
        assert_eq!(h.percentile_us(950), Some(50_000));
        assert_eq!(h.percentile_us(990), Some(50_000));
        assert_eq!(h.percentile_us(1_000), Some(u64::MAX));
    }

    #[test]
    fn drain_and_restore_round_trip() {
        let m = Metrics::default();
        m.record(
            ProviderId::Helius,
            crate::operations::GET_SLOT,
            Outcome::Success,
            Duration::from_millis(3),
        );
        m.record(
            ProviderId::Helius,
            crate::operations::GET_SLOT,
            Outcome::Error("TIMEOUT"),
            Duration::from_secs(1),
        );
        m.record_usage(ProviderId::Helius, crate::operations::GET_SLOT, 2, 2, 512);
        let (u, ms) = m.drain();
        assert_eq!(u.len(), 1);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].success_count, 1);
        assert_eq!(ms[0].error_count, 1);
        assert_eq!(ms[0].latency_hist.len(), LATENCY_BUCKETS);
        let (u2, m2) = m.drain();
        assert!(u2.is_empty() && m2.is_empty());
        m.restore(u, ms);
        let (u3, m3) = m.drain();
        assert_eq!(u3[0].credits, 2);
        assert_eq!(m3[0].error_count, 1);
        let roll = m.provider_rollup(ProviderId::Helius);
        assert_eq!(roll.errors_by_code.len(), 0, "rollup does not merge codes");
        assert_eq!(roll.last_error.unwrap().0, "TIMEOUT");
        assert_eq!(m.usage_totals(ProviderId::Helius).bytes, 512);
    }
}
