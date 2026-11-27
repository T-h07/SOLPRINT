//! Centralized rate limiting (GCRA, integer nanoseconds).
//!
//! One limiter per *rate group*, not per endpoint: Jupiter documents a single
//! shared bucket for price, token and swap requests, so they share
//! `jupiter.general`. A provider-signalled limit (HTTP 429 with a reset or
//! `Retry-After`) blocks the whole group until it expires.

use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::context::ProviderId;
use crate::error::GatewayError;

/// A rate: `count` requests per `period`, with a burst allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub count: u32,
    pub period: Duration,
    /// Requests that may be sent back-to-back after an idle period (≥ 1).
    pub burst: u32,
}

impl Rate {
    #[must_use]
    pub const fn per_second(count: u32) -> Self {
        Self {
            count,
            period: Duration::from_secs(1),
            burst: count,
        }
    }

    /// `count` per minute, released evenly (burst 1) so a 60-second sliding
    /// window can never see more than `count`.
    #[must_use]
    pub const fn per_minute_even(count: u32) -> Self {
        Self {
            count,
            period: Duration::from_secs(60),
            burst: 1,
        }
    }

    fn emission_interval(&self) -> Duration {
        let count = self.count.max(1);
        self.period / count
    }
}

#[derive(Debug)]
struct State {
    /// Theoretical arrival time of the next conforming request.
    tat: Instant,
    blocked_until: Option<Instant>,
    admitted: u64,
    delayed: u64,
    rejected: u64,
    upstream_limited: u64,
    waited: Duration,
}

/// Snapshot for monitoring.
#[derive(Debug, Clone)]
pub struct LimiterSnapshot {
    pub group: &'static str,
    pub provider: ProviderId,
    pub rate: Rate,
    pub admitted: u64,
    pub delayed: u64,
    pub rejected: u64,
    pub upstream_limited: u64,
    pub total_wait: Duration,
    pub blocked_for: Option<Duration>,
}

#[derive(Debug)]
pub struct RateLimiter {
    group: &'static str,
    provider: ProviderId,
    rate: Rate,
    state: Mutex<State>,
}

impl RateLimiter {
    #[must_use]
    pub fn new(group: &'static str, provider: ProviderId, rate: Rate) -> Self {
        Self {
            group,
            provider,
            rate,
            state: Mutex::new(State {
                tat: Instant::now(),
                blocked_until: None,
                admitted: 0,
                delayed: 0,
                rejected: 0,
                upstream_limited: 0,
                waited: Duration::ZERO,
            }),
        }
    }

    #[must_use]
    pub const fn group(&self) -> &'static str {
        self.group
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Reserve a slot, returning how long to wait before sending. Refuses
    /// without consuming the slot when the wait would pass `deadline`.
    fn reserve(&self, now: Instant, deadline: Instant) -> Result<Duration, GatewayError> {
        let mut s = self.lock();
        let t = self.rate.emission_interval();
        let tau = t * self.rate.burst.max(1).saturating_sub(1);
        let mut earliest = s.tat.max(now);
        if let Some(until) = s.blocked_until {
            if until > now {
                earliest = earliest.max(until);
            } else {
                s.blocked_until = None;
            }
        }
        // Conforming if earliest - tau <= now.
        let send_at = earliest.checked_sub(tau).unwrap_or(now).max(now);
        let send_at = match s.blocked_until {
            Some(until) if until > send_at => until,
            _ => send_at,
        };
        if send_at > deadline {
            s.rejected = s.rejected.saturating_add(1);
            return Err(GatewayError::RateLimited {
                provider: self.provider,
                retry_after: Some(send_at.saturating_duration_since(now)),
            });
        }
        s.tat = earliest + t;
        s.admitted = s.admitted.saturating_add(1);
        let wait = send_at.saturating_duration_since(now);
        if !wait.is_zero() {
            s.delayed = s.delayed.saturating_add(1);
            s.waited = s.waited.saturating_add(wait);
        }
        Ok(wait)
    }

    /// Wait for a slot, honouring the deadline and cancellation.
    pub async fn acquire(
        &self,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<(), GatewayError> {
        let wait = self.reserve(Instant::now(), deadline)?;
        if wait.is_zero() {
            return Ok(());
        }
        tokio::select! {
            () = tokio::time::sleep(wait) => Ok(()),
            () = cancel.cancelled() => Err(GatewayError::Cancelled),
        }
    }

    /// The provider said we are limited: block the group for `for_`
    /// (clamped to 1 s – 5 min so a bogus header cannot wedge the group).
    pub fn block_for(&self, for_: Duration) {
        let for_ = for_.clamp(Duration::from_secs(1), Duration::from_secs(300));
        let mut s = self.lock();
        let until = Instant::now() + for_;
        s.blocked_until = Some(s.blocked_until.map_or(until, |u| u.max(until)));
        s.upstream_limited = s.upstream_limited.saturating_add(1);
    }

    #[must_use]
    pub fn snapshot(&self) -> LimiterSnapshot {
        let s = self.lock();
        let now = Instant::now();
        LimiterSnapshot {
            group: self.group,
            provider: self.provider,
            rate: self.rate,
            admitted: s.admitted,
            delayed: s.delayed,
            rejected: s.rejected,
            upstream_limited: s.upstream_limited,
            total_wait: s.waited,
            blocked_for: s
                .blocked_until
                .filter(|u| *u > now)
                .map(|u| u.saturating_duration_since(now)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn per_second_rate_allows_burst_then_spaces_requests() {
        let l = RateLimiter::new("t", ProviderId::Helius, Rate::per_second(5));
        let far = Instant::now() + Duration::from_secs(60);
        let start = Instant::now();
        for _ in 0..5 {
            assert_eq!(l.reserve(Instant::now(), far).unwrap(), Duration::ZERO);
        }
        assert_eq!(
            l.reserve(Instant::now(), far).unwrap(),
            Duration::from_millis(200)
        );
        let cancel = CancellationToken::new();
        l.acquire(far, &cancel).await.unwrap();
        assert!(start.elapsed() >= Duration::from_millis(400));
    }

    #[tokio::test(start_paused = true)]
    async fn per_minute_even_never_exceeds_the_window() {
        let l = RateLimiter::new("j", ProviderId::Jupiter, Rate::per_minute_even(60));
        let far = Instant::now() + Duration::from_secs(3_600);
        let cancel = CancellationToken::new();
        let start = Instant::now();
        for _ in 0..61 {
            l.acquire(far, &cancel).await.unwrap();
        }
        // 61 requests need at least 60 intervals of 1 s.
        assert!(start.elapsed() >= Duration::from_secs(60));
    }

    #[tokio::test(start_paused = true)]
    async fn refuses_when_the_wait_passes_the_deadline_without_consuming() {
        let l = RateLimiter::new("j", ProviderId::Jupiter, Rate::per_minute_even(1));
        let soon = Instant::now() + Duration::from_secs(1);
        l.reserve(Instant::now(), soon).unwrap();
        assert!(matches!(
            l.reserve(Instant::now(), soon),
            Err(GatewayError::RateLimited { .. })
        ));
        assert_eq!(l.snapshot().admitted, 1);
        assert_eq!(l.snapshot().rejected, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn upstream_block_is_clamped_and_respected() {
        let l = RateLimiter::new("d", ProviderId::DexScreener, Rate::per_second(10));
        l.block_for(Duration::from_secs(3_600));
        let blocked = l.snapshot().blocked_for.unwrap();
        assert!(blocked <= Duration::from_secs(300));
        let soon = Instant::now() + Duration::from_secs(5);
        assert!(l.reserve(Instant::now(), soon).is_err());
    }
}
