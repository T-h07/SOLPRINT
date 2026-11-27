//! Per-provider circuit breaker.
//!
//! * `CLOSED`: requests flow; 5 consecutive provider failures open it.
//! * `OPEN`: requests are refused locally for the open interval, starting at
//!   30 s and doubling on each failed probe up to 5 min.
//! * `HALF_OPEN`: exactly one probe request is let through. Success closes
//!   the circuit and resets the interval; failure re-opens it.
//!
//! Only provider-side failures count (`GatewayError::is_provider_failure`):
//! a 404, a local refusal or a rate limit never opens the circuit.

use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;

use crate::context::ProviderId;
use crate::error::GatewayError;

pub const FAILURE_THRESHOLD: u32 = 5;
pub const BASE_OPEN: Duration = Duration::from_secs(30);
pub const MAX_OPEN: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "CLOSED",
            Self::Open => "OPEN",
            Self::HalfOpen => "HALF_OPEN",
        }
    }
}

#[derive(Debug)]
struct Inner {
    state: CircuitState,
    consecutive_failures: u32,
    open_until: Instant,
    open_for: Duration,
    probe_in_flight: bool,
    times_opened: u64,
}

/// Permission to send one request. Report its outcome with
/// [`CircuitBreaker::record`]; dropping it unreported (cancellation) frees
/// a half-open probe slot without changing state.
#[derive(Debug)]
#[must_use]
pub struct Ticket<'a> {
    breaker: &'a CircuitBreaker,
    probe: bool,
    reported: bool,
}

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        if self.probe && !self.reported {
            let mut s = self.breaker.lock();
            s.probe_in_flight = false;
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BreakerSnapshot {
    pub state: CircuitState,
    pub consecutive_failures: u32,
    pub open_for: Duration,
    pub retry_in: Option<Duration>,
    pub times_opened: u64,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    provider: ProviderId,
    inner: Mutex<Inner>,
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(provider: ProviderId) -> Self {
        Self {
            provider,
            inner: Mutex::new(Inner {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                open_until: Instant::now(),
                open_for: BASE_OPEN,
                probe_in_flight: false,
                times_opened: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Ask to send a request now.
    pub fn check(&self) -> Result<Ticket<'_>, GatewayError> {
        let now = Instant::now();
        let mut s = self.lock();
        match s.state {
            CircuitState::Closed => Ok(Ticket {
                breaker: self,
                probe: false,
                reported: false,
            }),
            CircuitState::Open if now < s.open_until => Err(GatewayError::CircuitOpen {
                provider: self.provider,
                retry_in: s.open_until.saturating_duration_since(now),
            }),
            CircuitState::Open | CircuitState::HalfOpen => {
                if s.probe_in_flight {
                    return Err(GatewayError::CircuitOpen {
                        provider: self.provider,
                        retry_in: Duration::from_secs(1),
                    });
                }
                s.state = CircuitState::HalfOpen;
                s.probe_in_flight = true;
                Ok(Ticket {
                    breaker: self,
                    probe: true,
                    reported: false,
                })
            }
        }
    }

    /// Report the outcome of a request admitted by `ticket`.
    pub fn record(&self, mut ticket: Ticket<'_>, outcome: Result<(), &GatewayError>) {
        ticket.reported = true;
        let failed = matches!(outcome, Err(e) if e.is_provider_failure());
        let mut s = self.lock();
        if ticket.probe {
            s.probe_in_flight = false;
        }
        if !failed {
            if ticket.probe || s.state != CircuitState::Closed {
                tracing::info!(provider = %self.provider, "circuit closed");
            }
            s.state = CircuitState::Closed;
            s.consecutive_failures = 0;
            s.open_for = BASE_OPEN;
            return;
        }
        s.consecutive_failures = s.consecutive_failures.saturating_add(1);
        let open = if ticket.probe {
            s.open_for = (s.open_for * 2).min(MAX_OPEN);
            true
        } else {
            s.state == CircuitState::Closed && s.consecutive_failures >= FAILURE_THRESHOLD
        };
        if open {
            s.state = CircuitState::Open;
            s.open_until = Instant::now() + s.open_for;
            s.times_opened = s.times_opened.saturating_add(1);
            tracing::warn!(
                provider = %self.provider,
                open_for_s = s.open_for.as_secs(),
                "circuit opened"
            );
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> BreakerSnapshot {
        let s = self.lock();
        let now = Instant::now();
        BreakerSnapshot {
            state: s.state,
            consecutive_failures: s.consecutive_failures,
            open_for: s.open_for,
            retry_in: (s.state == CircuitState::Open && s.open_until > now)
                .then(|| s.open_until.saturating_duration_since(now)),
            times_opened: s.times_opened,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn fail() -> GatewayError {
        GatewayError::Timeout {
            provider: ProviderId::Helius,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn opens_after_threshold_probes_and_backs_off() {
        let b = CircuitBreaker::new(ProviderId::Helius);
        for _ in 0..FAILURE_THRESHOLD {
            let t = b.check().unwrap();
            b.record(t, Err(&fail()));
        }
        assert_eq!(b.snapshot().state, CircuitState::Open);
        assert!(matches!(b.check(), Err(GatewayError::CircuitOpen { .. })));

        tokio::time::advance(BASE_OPEN).await;
        let probe = b.check().unwrap();
        // Only one probe at a time.
        assert!(b.check().is_err());
        b.record(probe, Err(&fail()));
        assert_eq!(b.snapshot().open_for, BASE_OPEN * 2);

        tokio::time::advance(BASE_OPEN * 2).await;
        let probe = b.check().unwrap();
        b.record(probe, Ok(()));
        let s = b.snapshot();
        assert_eq!(s.state, CircuitState::Closed);
        assert_eq!(s.open_for, BASE_OPEN);
        assert_eq!(s.times_opened, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn open_interval_is_capped() {
        let b = CircuitBreaker::new(ProviderId::Helius);
        for _ in 0..FAILURE_THRESHOLD {
            let t = b.check().unwrap();
            b.record(t, Err(&fail()));
        }
        for _ in 0..10 {
            tokio::time::advance(MAX_OPEN).await;
            let probe = b.check().unwrap();
            b.record(probe, Err(&fail()));
        }
        assert_eq!(b.snapshot().open_for, MAX_OPEN);
    }

    #[tokio::test(start_paused = true)]
    async fn client_errors_and_cancelled_probes_do_not_trip() {
        let b = CircuitBreaker::new(ProviderId::Jupiter);
        for _ in 0..20 {
            let t = b.check().unwrap();
            b.record(
                t,
                Err(&GatewayError::Http {
                    provider: ProviderId::Jupiter,
                    status: 404,
                }),
            );
        }
        assert_eq!(b.snapshot().state, CircuitState::Closed);
        for _ in 0..FAILURE_THRESHOLD {
            let t = b.check().unwrap();
            b.record(t, Err(&fail()));
        }
        tokio::time::advance(BASE_OPEN).await;
        drop(b.check().unwrap()); // probe abandoned
        assert!(b.check().is_ok(), "probe slot must be released");
    }
}
