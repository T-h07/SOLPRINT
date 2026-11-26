//! Provider health derived from real traffic only.
//!
//! GATEWAY never probes a provider just to colour a dashboard: health is
//! computed from the outcomes of requests callers actually made, plus the
//! circuit breaker and budget state. A provider with no traffic since start
//! is `UNKNOWN`, never `HEALTHY`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use solprint_core::UtcTimestamp;
use tokio::time::Instant;

use crate::breaker::CircuitState;
use crate::budget::BudgetState;

const WINDOW: usize = 20;
const RATE_LIMIT_MEMORY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// Turned off by configuration or missing a required credential.
    Disabled,
    /// No request has completed since start.
    Unknown,
    Healthy,
    /// Some recent failures or recent upstream rate limiting.
    Degraded,
    /// Circuit open or every recent request failed.
    Unavailable,
    /// Budget no longer admits ordinary traffic.
    BudgetExhausted,
}

impl HealthState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "DISABLED",
            Self::Unknown => "UNKNOWN",
            Self::Healthy => "HEALTHY",
            Self::Degraded => "DEGRADED",
            Self::Unavailable => "UNAVAILABLE",
            Self::BudgetExhausted => "BUDGET_EXHAUSTED",
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    recent: VecDeque<bool>,
    last_success_at: Option<UtcTimestamp>,
    last_failure_at: Option<UtcTimestamp>,
    last_error_code: Option<&'static str>,
    last_rate_limited: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub state: HealthState,
    pub reason: Option<&'static str>,
    pub last_success_at: Option<UtcTimestamp>,
    pub last_failure_at: Option<UtcTimestamp>,
    pub last_error_code: Option<&'static str>,
    pub recent_failures: u32,
    pub recent_total: u32,
}

#[derive(Debug, Default)]
pub struct HealthTracker {
    inner: Mutex<Inner>,
}

impl HealthTracker {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn success(&self) {
        let mut s = self.lock();
        push(&mut s.recent, true);
        s.last_success_at = Some(UtcTimestamp::now());
    }

    /// Record a provider-side failure (`code` is a bounded error label).
    pub fn failure(&self, code: &'static str) {
        let mut s = self.lock();
        push(&mut s.recent, false);
        s.last_failure_at = Some(UtcTimestamp::now());
        s.last_error_code = Some(code);
    }

    pub fn rate_limited(&self) {
        let mut s = self.lock();
        s.last_rate_limited = Some(Instant::now());
        s.last_error_code = Some("RATE_LIMITED");
        s.last_failure_at = Some(UtcTimestamp::now());
    }

    #[must_use]
    pub fn snapshot(
        &self,
        enabled: Result<(), &'static str>,
        circuit: CircuitState,
        budget: Option<BudgetState>,
    ) -> HealthSnapshot {
        let s = self.lock();
        let total = u32::try_from(s.recent.len()).unwrap_or(u32::MAX);
        let failures =
            u32::try_from(s.recent.iter().filter(|ok| !**ok).count()).unwrap_or(u32::MAX);
        let recently_limited = s
            .last_rate_limited
            .is_some_and(|t| t.elapsed() <= RATE_LIMIT_MEMORY);
        let (state, reason) = if let Err(reason) = enabled {
            (HealthState::Disabled, Some(reason))
        } else if circuit == CircuitState::Open {
            (HealthState::Unavailable, Some("circuit open"))
        } else if matches!(
            budget,
            Some(BudgetState::HardLimitReached | BudgetState::NormalBudgetExhausted)
        ) {
            (
                HealthState::BudgetExhausted,
                Some("credit budget exhausted"),
            )
        } else if total == 0 && !recently_limited {
            (HealthState::Unknown, Some("no requests since start"))
        } else if total > 0 && failures == total {
            (HealthState::Unavailable, Some("all recent requests failed"))
        } else if circuit == CircuitState::HalfOpen {
            (HealthState::Degraded, Some("circuit half-open"))
        } else if recently_limited {
            (HealthState::Degraded, Some("rate limited by provider"))
        } else if failures > 0 {
            (HealthState::Degraded, Some("recent failures"))
        } else {
            (HealthState::Healthy, None)
        };
        HealthSnapshot {
            state,
            reason,
            last_success_at: s.last_success_at,
            last_failure_at: s.last_failure_at,
            last_error_code: s.last_error_code,
            recent_failures: failures,
            recent_total: total,
        }
    }
}

fn push(q: &mut VecDeque<bool>, v: bool) {
    if q.len() == WINDOW {
        q.pop_front();
    }
    q.push_back(v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_is_unknown_until_traffic_and_tracks_outcomes() {
        let h = HealthTracker::default();
        let snap = |h: &HealthTracker| h.snapshot(Ok(()), CircuitState::Closed, None).state;
        assert_eq!(snap(&h), HealthState::Unknown);
        h.success();
        assert_eq!(snap(&h), HealthState::Healthy);
        h.failure("TIMEOUT");
        assert_eq!(snap(&h), HealthState::Degraded);
        for _ in 0..WINDOW {
            h.success();
        }
        assert_eq!(snap(&h), HealthState::Healthy);
        for _ in 0..WINDOW {
            h.failure("CONNECT");
        }
        assert_eq!(snap(&h), HealthState::Unavailable);
        assert_eq!(
            h.snapshot(Err("no credential"), CircuitState::Closed, None)
                .state,
            HealthState::Disabled
        );
        assert_eq!(
            h.snapshot(Ok(()), CircuitState::Open, None).state,
            HealthState::Unavailable
        );
    }

    #[tokio::test]
    async fn budget_exhaustion_and_rate_limits_are_visible() {
        let h = HealthTracker::default();
        h.success();
        assert_eq!(
            h.snapshot(
                Ok(()),
                CircuitState::Closed,
                Some(BudgetState::NormalBudgetExhausted)
            )
            .state,
            HealthState::BudgetExhausted
        );
        h.rate_limited();
        assert_eq!(
            h.snapshot(Ok(()), CircuitState::Closed, Some(BudgetState::Normal))
                .state,
            HealthState::Degraded
        );
    }
}
