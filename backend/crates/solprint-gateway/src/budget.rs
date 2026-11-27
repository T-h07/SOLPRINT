//! Persistent credit budget engine (Helius).
//!
//! Credits are charged **when a request is dispatched**, before the response
//! arrives, so a crash can only over-count, never under-count. A reservation
//! is refunded only when the request provably never left the process.
//!
//! The cycle limit is split into a *normal* budget and a *reserve*
//! (`HELIUS_RESERVE_PERCENT`, default 20 %). States are derived from the
//! projected usage after the reservation:
//!
//! | State                      | Used (of normal budget)  | Admitted priorities |
//! |----------------------------|--------------------------|---------------------|
//! | `NORMAL`                   | < 70 %                   | P0–P5               |
//! | `CONSERVATION`             | 70 % – < 90 %            | P0–P3               |
//! | `CRITICAL`                 | 90 % – < 100 %           | P0–P2               |
//! | `NORMAL_BUDGET_EXHAUSTED`  | in the reserve           | P0–P1               |
//! | `HARD_LIMIT_REACHED`       | cycle limit reached      | none                |
//! | `ACCOUNT_USAGE_UNKNOWN`    | ledger not loaded        | P0–P2               |
//!
//! All arithmetic is integer (`u64`/`u128`), saturating or checked.

use std::sync::Mutex;

use solprint_core::UtcTimestamp;
use time::{Date, Month, OffsetDateTime, Time};

use crate::context::{Priority, ProviderId};
use crate::error::GatewayError;

/// Usage of the normal budget (percent) at which `CONSERVATION` begins.
pub const CONSERVATION_AT_PERCENT: u64 = 70;
/// Usage of the normal budget (percent) at which `CRITICAL` begins.
pub const CRITICAL_AT_PERCENT: u64 = 90;
/// Documented Helius WebSocket metering: 2 credits per 0.1 MB uncompressed.
pub const STREAM_CREDITS_PER_MB: u64 = 20;
/// Documented Helius WebSocket metering: 1 credit per connection opened.
pub const STREAM_CONNECTION_CREDITS: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetState {
    Normal,
    Conservation,
    Critical,
    NormalBudgetExhausted,
    HardLimitReached,
    AccountUsageUnknown,
}

impl BudgetState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Conservation => "CONSERVATION",
            Self::Critical => "CRITICAL",
            Self::NormalBudgetExhausted => "NORMAL_BUDGET_EXHAUSTED",
            Self::HardLimitReached => "HARD_LIMIT_REACHED",
            Self::AccountUsageUnknown => "ACCOUNT_USAGE_UNKNOWN",
        }
    }

    /// Lowest-importance priority still admitted in this state.
    #[must_use]
    pub const fn admits_up_to(self) -> Option<Priority> {
        match self {
            Self::Normal => Some(Priority::P5),
            Self::Conservation => Some(Priority::P3),
            Self::Critical | Self::AccountUsageUnknown => Some(Priority::P2),
            Self::NormalBudgetExhausted => Some(Priority::P1),
            Self::HardLimitReached => None,
        }
    }

    #[must_use]
    pub fn admits(self, priority: Priority) -> bool {
        self.admits_up_to().is_some_and(|max| priority <= max)
    }
}

/// Budget parameters from configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetPolicy {
    pub monthly_credits: u64,
    pub reserve_percent: u8,
    pub cycle_start_day: u8,
}

impl BudgetPolicy {
    #[must_use]
    pub fn reserve_credits(&self) -> u64 {
        mul_div(
            self.monthly_credits,
            u64::from(self.reserve_percent.min(100)),
            100,
        )
    }

    #[must_use]
    pub fn normal_credits(&self) -> u64 {
        self.monthly_credits.saturating_sub(self.reserve_credits())
    }

    /// Cumulative usage at which `CONSERVATION` begins.
    #[must_use]
    pub fn conservation_at(&self) -> u64 {
        mul_div(self.normal_credits(), CONSERVATION_AT_PERCENT, 100)
    }

    /// Cumulative usage at which `CRITICAL` begins.
    #[must_use]
    pub fn critical_at(&self) -> u64 {
        mul_div(self.normal_credits(), CRITICAL_AT_PERCENT, 100)
    }

    /// State for a given cumulative usage (ledger loaded).
    #[must_use]
    pub fn classify(&self, used: u64) -> BudgetState {
        let normal = self.normal_credits();
        if used >= self.monthly_credits {
            BudgetState::HardLimitReached
        } else if used >= normal {
            BudgetState::NormalBudgetExhausted
        } else if used >= self.critical_at() {
            BudgetState::Critical
        } else if used >= self.conservation_at() {
            BudgetState::Conservation
        } else {
            BudgetState::Normal
        }
    }
}

/// `a * b / c` in `u128`, rounded down, saturating to `u64::MAX`.
fn mul_div(a: u64, b: u64, c: u64) -> u64 {
    if c == 0 {
        return 0;
    }
    u64::try_from(u128::from(a) * u128::from(b) / u128::from(c)).unwrap_or(u64::MAX)
}

/// `ceil(bytes * credits_per_mb / 1_000_000)`.
#[must_use]
pub fn stream_credits_for_bytes(bytes: u64) -> u64 {
    let num = u128::from(bytes) * u128::from(STREAM_CREDITS_PER_MB);
    u64::try_from(num.div_ceil(1_000_000)).unwrap_or(u64::MAX)
}

/// A billing cycle `[start, end)` in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillingCycle {
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
}

impl BillingCycle {
    /// The cycle containing `now` for a cycle that starts on `day` (1–28)
    /// of every month at 00:00 UTC.
    #[must_use]
    pub fn containing(now: OffsetDateTime, day: u8) -> Self {
        let day = day.clamp(1, 28);
        let now = now.to_offset(time::UtcOffset::UTC);
        let this = month_day(now.year(), now.month(), day);
        let start = if now >= this {
            this
        } else {
            let (y, m) = prev_month(now.year(), now.month());
            month_day(y, m, day)
        };
        let (ny, nm) = next_month(start.year(), start.month());
        Self {
            start,
            end: month_day(ny, nm, day),
        }
    }

    #[must_use]
    pub fn start_ts(&self) -> UtcTimestamp {
        UtcTimestamp::from_offset_datetime(self.start)
    }

    #[must_use]
    pub fn end_ts(&self) -> UtcTimestamp {
        UtcTimestamp::from_offset_datetime(self.end)
    }
}

fn month_day(year: i32, month: Month, day: u8) -> OffsetDateTime {
    // Day is clamped to 1..=28, which exists in every month.
    Date::from_calendar_date(year, month, day)
        .unwrap_or(Date::MIN)
        .with_time(Time::MIDNIGHT)
        .assume_utc()
}

const fn prev_month(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::January => (year - 1, Month::December),
        m => (year, m.previous()),
    }
}

const fn next_month(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::December => (year + 1, Month::January),
        m => (year, m.next()),
    }
}

/// Credits that are charged locally but not yet persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingFlush {
    pub cycle: BillingCycle,
    pub credits: u64,
}

/// A successful reservation. Hand it back to [`CreditBudget::refund`] only
/// if the request was never dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct Reservation {
    pub credits: u64,
    cycle_start: OffsetDateTime,
}

#[derive(Debug)]
struct Inner {
    cycle: BillingCycle,
    /// Cumulative credits in the current cycle, including `pending`.
    used: u64,
    /// Not yet persisted, per cycle (a rollover can leave the old cycle's
    /// remainder pending).
    pending: Vec<PendingFlush>,
    loaded: bool,
    stream_bytes: u64,
    stream_credits: u64,
    denied: [u64; 6],
    last_persisted_at: Option<UtcTimestamp>,
    persist_failures: u64,
}

/// Snapshot for monitoring.
#[derive(Debug, Clone)]
pub struct BudgetSnapshot {
    pub provider: ProviderId,
    pub state: BudgetState,
    pub cycle_start: UtcTimestamp,
    pub cycle_end: UtcTimestamp,
    pub credit_limit: u64,
    pub reserve_credits: u64,
    pub normal_credits: u64,
    pub credits_used: u64,
    pub credits_pending_persist: u64,
    pub credits_remaining: u64,
    /// Linear projection of usage at cycle end; `None` during the first hour.
    pub projected_cycle_credits: Option<u64>,
    pub stream_bytes: u64,
    pub stream_credits: u64,
    pub denied_by_priority: [u64; 6],
    pub ledger_loaded: bool,
    pub last_persisted_at: Option<UtcTimestamp>,
    pub persist_failures: u64,
    pub admits_up_to: Option<Priority>,
}

#[derive(Debug)]
pub struct CreditBudget {
    provider: ProviderId,
    policy: BudgetPolicy,
    inner: Mutex<Inner>,
}

impl CreditBudget {
    #[must_use]
    pub fn new(provider: ProviderId, policy: BudgetPolicy, now: OffsetDateTime) -> Self {
        Self {
            provider,
            policy,
            inner: Mutex::new(Inner {
                cycle: BillingCycle::containing(now, policy.cycle_start_day),
                used: 0,
                pending: Vec::new(),
                loaded: false,
                stream_bytes: 0,
                stream_credits: 0,
                denied: [0; 6],
                last_persisted_at: None,
                persist_failures: 0,
            }),
        }
    }

    #[must_use]
    pub const fn policy(&self) -> BudgetPolicy {
        self.policy
    }

    #[must_use]
    pub const fn provider(&self) -> ProviderId {
        self.provider
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock only means another thread panicked mid-update of
        // plain counters; the data is still the best available.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn state_of(&self, inner: &Inner, used: u64) -> BudgetState {
        if !inner.loaded {
            // Unknown history: conservative, but never above the hard limit.
            if used >= self.policy.monthly_credits {
                BudgetState::HardLimitReached
            } else {
                BudgetState::AccountUsageUnknown
            }
        } else {
            self.policy.classify(used)
        }
    }

    fn roll(&self, inner: &mut Inner, now: OffsetDateTime) {
        if now >= inner.cycle.end || now < inner.cycle.start {
            let next = BillingCycle::containing(now, self.policy.cycle_start_day);
            tracing::info!(
                provider = %self.provider,
                previous_used = inner.used,
                "billing cycle rolled over"
            );
            inner.cycle = next;
            inner.used = 0;
            inner.stream_bytes = 0;
            inner.stream_credits = 0;
            inner.denied = [0; 6];
            // The new cycle's history must be loaded before it is trusted.
            inner.loaded = false;
        }
    }

    #[must_use]
    pub fn state(&self, now: OffsetDateTime) -> BudgetState {
        let mut inner = self.lock();
        self.roll(&mut inner, now);
        self.state_of(&inner, inner.used)
    }

    #[must_use]
    pub fn cycle(&self, now: OffsetDateTime) -> BillingCycle {
        let mut inner = self.lock();
        self.roll(&mut inner, now);
        inner.cycle
    }

    /// Charge `credits` for a request at `priority`, or refuse it.
    pub fn try_reserve(
        &self,
        priority: Priority,
        credits: u64,
        now: OffsetDateTime,
    ) -> Result<Reservation, GatewayError> {
        let mut inner = self.lock();
        self.roll(&mut inner, now);
        let projected = inner.used.saturating_add(credits);
        let current = self.state_of(&inner, inner.used);
        // State while the last credit of this reservation is consumed.
        let after = self.state_of(&inner, projected.saturating_sub(1));
        if !current.admits(priority)
            || !after.admits(priority)
            || projected > self.policy.monthly_credits
        {
            let idx = usize::from(priority.rank());
            inner.denied[idx] = inner.denied[idx].saturating_add(1);
            let state = if projected > self.policy.monthly_credits {
                BudgetState::HardLimitReached
            } else if current.admits(priority) {
                after
            } else {
                current
            };
            return Err(GatewayError::BudgetDenied { state, priority });
        }
        inner.used = projected;
        let cycle = inner.cycle;
        add_pending(&mut inner.pending, cycle, credits);
        Ok(Reservation {
            credits,
            cycle_start: cycle.start,
        })
    }

    /// Return a reservation for a request that was never dispatched.
    pub fn refund(&self, reservation: Reservation) {
        let mut inner = self.lock();
        if reservation.cycle_start != inner.cycle.start {
            return; // cycle rolled; keep the conservative charge
        }
        inner.used = inner.used.saturating_sub(reservation.credits);
        if let Some(p) = inner
            .pending
            .iter_mut()
            .find(|p| p.cycle.start == reservation.cycle_start)
        {
            p.credits = p.credits.saturating_sub(reservation.credits);
        }
    }

    /// Meter received stream bytes. Data already received cannot be
    /// refused, so this always charges and returns the credits added.
    pub fn charge_stream_bytes(&self, bytes: u64, now: OffsetDateTime) -> u64 {
        let mut inner = self.lock();
        self.roll(&mut inner, now);
        inner.stream_bytes = inner.stream_bytes.saturating_add(bytes);
        let due = stream_credits_for_bytes(inner.stream_bytes);
        let delta = due.saturating_sub(inner.stream_credits);
        if delta > 0 {
            inner.stream_credits = due;
            inner.used = inner.used.saturating_add(delta);
            let cycle = inner.cycle;
            add_pending(&mut inner.pending, cycle, delta);
        }
        delta
    }

    /// Install the persisted total for the current cycle (startup or
    /// rollover). Credits charged before loading are added on top.
    pub fn load(&self, cycle: BillingCycle, persisted_used: u64) {
        let mut inner = self.lock();
        if inner.cycle != cycle {
            return;
        }
        let pending_here: u64 = inner
            .pending
            .iter()
            .filter(|p| p.cycle == cycle)
            .map(|p| p.credits)
            .sum();
        inner.used = persisted_used.saturating_add(pending_here);
        inner.loaded = true;
    }

    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.lock().loaded
    }

    /// Take everything awaiting persistence. Call [`Self::flush_failed`] with
    /// the same value if the write fails.
    pub fn take_pending(&self) -> Vec<PendingFlush> {
        let mut inner = self.lock();
        std::mem::take(&mut inner.pending)
            .into_iter()
            .filter(|p| p.credits > 0)
            .collect()
    }

    /// The write succeeded; `persisted_total` is the database's cumulative
    /// value for `cycle`. If another writer charged the same cycle, the
    /// larger total wins (never under-count).
    pub fn flush_succeeded(&self, cycle: BillingCycle, persisted_total: u64, at: UtcTimestamp) {
        let mut inner = self.lock();
        inner.last_persisted_at = Some(at);
        if inner.cycle == cycle && inner.loaded {
            let pending_here: u64 = inner
                .pending
                .iter()
                .filter(|p| p.cycle == cycle)
                .map(|p| p.credits)
                .sum();
            inner.used = inner.used.max(persisted_total.saturating_add(pending_here));
        }
    }

    pub fn flush_failed(&self, flushes: Vec<PendingFlush>) {
        let mut inner = self.lock();
        inner.persist_failures = inner.persist_failures.saturating_add(1);
        for f in flushes {
            add_pending(&mut inner.pending, f.cycle, f.credits);
        }
    }

    #[must_use]
    pub fn snapshot(&self, now: OffsetDateTime) -> BudgetSnapshot {
        let mut inner = self.lock();
        self.roll(&mut inner, now);
        let state = self.state_of(&inner, inner.used);
        let elapsed = (now - inner.cycle.start).whole_seconds();
        let total = (inner.cycle.end - inner.cycle.start).whole_seconds();
        let projected = match (u64::try_from(elapsed), u64::try_from(total)) {
            (Ok(e), Ok(t)) if e >= 3_600 && t > 0 => Some(mul_div(inner.used, t, e)),
            _ => None,
        };
        BudgetSnapshot {
            provider: self.provider,
            state,
            cycle_start: inner.cycle.start_ts(),
            cycle_end: inner.cycle.end_ts(),
            credit_limit: self.policy.monthly_credits,
            reserve_credits: self.policy.reserve_credits(),
            normal_credits: self.policy.normal_credits(),
            credits_used: inner.used,
            credits_pending_persist: inner.pending.iter().map(|p| p.credits).sum(),
            credits_remaining: self.policy.monthly_credits.saturating_sub(inner.used),
            projected_cycle_credits: projected,
            stream_bytes: inner.stream_bytes,
            stream_credits: inner.stream_credits,
            denied_by_priority: inner.denied,
            ledger_loaded: inner.loaded,
            last_persisted_at: inner.last_persisted_at,
            persist_failures: inner.persist_failures,
            admits_up_to: state.admits_up_to(),
        }
    }
}

fn add_pending(pending: &mut Vec<PendingFlush>, cycle: BillingCycle, credits: u64) {
    if let Some(p) = pending.iter_mut().find(|p| p.cycle == cycle) {
        p.credits = p.credits.saturating_add(credits);
    } else {
        pending.push(PendingFlush { cycle, credits });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use time::macros::datetime;

    use super::*;

    const POLICY: BudgetPolicy = BudgetPolicy {
        monthly_credits: 1_000_000,
        reserve_percent: 20,
        cycle_start_day: 1,
    };

    fn loaded(now: OffsetDateTime, used: u64) -> CreditBudget {
        let b = CreditBudget::new(ProviderId::Helius, POLICY, now);
        b.load(b.cycle(now), used);
        b
    }

    #[test]
    fn thresholds_follow_the_documented_table() {
        assert_eq!(POLICY.normal_credits(), 800_000);
        assert_eq!(POLICY.reserve_credits(), 200_000);
        assert_eq!(POLICY.classify(0), BudgetState::Normal);
        assert_eq!(POLICY.classify(559_999), BudgetState::Normal);
        assert_eq!(POLICY.classify(560_000), BudgetState::Conservation);
        assert_eq!(POLICY.classify(720_000), BudgetState::Critical);
        assert_eq!(POLICY.classify(800_000), BudgetState::NormalBudgetExhausted);
        assert_eq!(POLICY.classify(1_000_000), BudgetState::HardLimitReached);
    }

    #[test]
    fn admission_sheds_low_priorities_first() {
        let now = datetime!(2026-11-26 12:00 UTC);
        let b = loaded(now, 560_000);
        assert!(b.try_reserve(Priority::P3, 1, now).is_ok());
        assert!(matches!(
            b.try_reserve(Priority::P4, 1, now),
            Err(GatewayError::BudgetDenied {
                state: BudgetState::Conservation,
                ..
            })
        ));
        let b = loaded(now, 799_999);
        // Two credits would cross into the reserve; the last normal one is fine.
        assert!(b.try_reserve(Priority::P2, 2, now).is_err());
        assert!(b.try_reserve(Priority::P2, 1, now).is_ok());
        assert!(b.try_reserve(Priority::P2, 1, now).is_err());
        assert!(b.try_reserve(Priority::P1, 1, now).is_ok());
        let b = loaded(now, 999_999);
        assert!(b.try_reserve(Priority::P0, 1, now).is_ok());
        assert!(matches!(
            b.try_reserve(Priority::P0, 1, now),
            Err(GatewayError::BudgetDenied {
                state: BudgetState::HardLimitReached,
                ..
            })
        ));
        let snap = b.snapshot(now);
        assert_eq!(snap.credits_used, 1_000_000);
        assert_eq!(snap.denied_by_priority[0], 1);
    }

    #[test]
    fn unloaded_ledger_is_conservative() {
        let now = datetime!(2026-11-26 12:00 UTC);
        let b = CreditBudget::new(ProviderId::Helius, POLICY, now);
        assert_eq!(b.state(now), BudgetState::AccountUsageUnknown);
        assert!(b.try_reserve(Priority::P2, 1, now).is_ok());
        assert!(b.try_reserve(Priority::P3, 1, now).is_err());
        // Loading adds what was charged before the load.
        b.load(b.cycle(now), 100);
        assert_eq!(b.snapshot(now).credits_used, 101);
        assert_eq!(b.state(now), BudgetState::Normal);
    }

    #[test]
    fn refund_and_flush_round_trip() {
        let now = datetime!(2026-11-26 12:00 UTC);
        let b = loaded(now, 10);
        let r = b.try_reserve(Priority::P3, 5, now).unwrap();
        b.refund(r);
        let _ = b.try_reserve(Priority::P3, 3, now).unwrap();
        let pending = b.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].credits, 3);
        b.flush_failed(pending);
        assert_eq!(b.snapshot(now).credits_pending_persist, 3);
        let pending = b.take_pending();
        b.flush_succeeded(pending[0].cycle, 13, UtcTimestamp::now());
        let s = b.snapshot(now);
        assert_eq!(s.credits_used, 13);
        assert_eq!(s.persist_failures, 1);
        // Another writer charged more: the larger value wins.
        b.flush_succeeded(pending[0].cycle, 50, UtcTimestamp::now());
        assert_eq!(b.snapshot(now).credits_used, 50);
    }

    #[test]
    fn stream_bytes_are_metered_with_a_running_ceiling() {
        let now = datetime!(2026-11-26 12:00 UTC);
        let b = loaded(now, 0);
        assert_eq!(stream_credits_for_bytes(0), 0);
        assert_eq!(stream_credits_for_bytes(1), 1);
        assert_eq!(stream_credits_for_bytes(100_000), 2);
        assert_eq!(stream_credits_for_bytes(1_000_000), 20);
        assert_eq!(b.charge_stream_bytes(1, now), 1);
        assert_eq!(b.charge_stream_bytes(49_999, now), 0);
        assert_eq!(b.charge_stream_bytes(1, now), 1);
        assert_eq!(b.charge_stream_bytes(949_999, now), 18);
        assert_eq!(b.snapshot(now).credits_used, 20);
    }

    #[test]
    fn billing_cycles_roll_on_the_configured_day() {
        let c = BillingCycle::containing(datetime!(2026-11-26 12:00 UTC), 1);
        assert_eq!(c.start, datetime!(2026-11-01 0:00 UTC));
        assert_eq!(c.end, datetime!(2026-12-01 0:00 UTC));
        let c = BillingCycle::containing(datetime!(2026-12-03 0:00 UTC), 15);
        assert_eq!(c.start, datetime!(2026-11-15 0:00 UTC));
        assert_eq!(c.end, datetime!(2026-12-15 0:00 UTC));
        let c = BillingCycle::containing(datetime!(2027-01-10 0:00 UTC), 28);
        assert_eq!(c.start, datetime!(2026-12-28 0:00 UTC));
        assert_eq!(c.end, datetime!(2027-01-28 0:00 UTC));

        let b = loaded(datetime!(2026-11-30 23:59 UTC), 900_000);
        assert_eq!(
            b.state(datetime!(2026-11-30 23:59 UTC)),
            BudgetState::NormalBudgetExhausted
        );
        // New cycle: unknown until its history is loaded, then normal.
        let next = datetime!(2026-12-01 0:00 UTC);
        assert_eq!(b.state(next), BudgetState::AccountUsageUnknown);
        b.load(b.cycle(next), 0);
        assert_eq!(b.state(next), BudgetState::Normal);
    }
}
