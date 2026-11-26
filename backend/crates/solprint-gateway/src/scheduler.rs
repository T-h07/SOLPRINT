//! Priority scheduler with bounded queues and per-provider concurrency.
//!
//! Each provider has a *lane*: at most `max_in_flight` requests run at once,
//! at most `queue_limit` wait, and all lanes together hold at most
//! `global_limit` waiters. Waiters are served by priority (P0 first), then
//! FIFO. A full queue is refused immediately (`QUEUE_FULL`) instead of
//! growing memory without bound.
//!
//! Permits are values: a permit handed to a waiter that gave up in the same
//! instant is dropped with the channel, which releases the slot, so a slot
//! can never leak.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::context::{Priority, ProviderId};
use crate::error::GatewayError;

#[derive(Debug)]
struct Waiter {
    rank: u8,
    seq: u64,
    tx: oneshot::Sender<Permit>,
}

impl PartialEq for Waiter {
    fn eq(&self, other: &Self) -> bool {
        self.rank == other.rank && self.seq == other.seq
    }
}
impl Eq for Waiter {}
impl PartialOrd for Waiter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Waiter {
    // BinaryHeap is a max-heap: "greater" = served first = lower rank, then
    // lower sequence number.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .rank
            .cmp(&self.rank)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

#[derive(Debug, Default)]
struct LaneState {
    in_flight: usize,
    waiters: BinaryHeap<Waiter>,
    seq: u64,
}

#[derive(Debug)]
struct Lane {
    provider: ProviderId,
    max_in_flight: usize,
    queue_limit: usize,
    state: Mutex<LaneState>,
    global: Arc<GlobalQueue>,
    admitted: AtomicU64,
    queued_total: AtomicU64,
    rejected: AtomicU64,
    timed_out: AtomicU64,
    max_in_flight_seen: AtomicUsize,
}

#[derive(Debug)]
struct GlobalQueue {
    limit: usize,
    waiting: AtomicUsize,
}

/// Snapshot for monitoring.
#[derive(Debug, Clone, Copy)]
pub struct LaneSnapshot {
    pub provider: ProviderId,
    pub max_in_flight: usize,
    pub queue_limit: usize,
    pub in_flight: usize,
    pub queued: usize,
    pub admitted: u64,
    pub queued_total: u64,
    pub rejected: u64,
    pub timed_out: u64,
    pub peak_in_flight: usize,
}

/// Holding a permit means one request may be in flight for its provider.
#[derive(Debug)]
pub struct Permit {
    lane: Option<Arc<Lane>>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Some(lane) = self.lane.take() {
            Lane::release(&lane);
        }
    }
}

impl Lane {
    fn lock(&self) -> std::sync::MutexGuard<'_, LaneState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn release(lane: &Arc<Self>) {
        let mut st = lane.lock();
        while let Some(w) = st.waiters.pop() {
            let permit = Permit {
                lane: Some(Arc::clone(lane)),
            };
            match w.tx.send(permit) {
                Ok(()) => return, // slot handed over; in_flight unchanged
                Err(mut returned) => {
                    // Receiver gone: defuse and try the next waiter.
                    returned.lane = None;
                }
            }
        }
        st.in_flight = st.in_flight.saturating_sub(1);
    }

    fn note_in_flight(&self, n: usize) {
        self.max_in_flight_seen
            .fetch_max(n, AtomicOrdering::Relaxed);
    }
}

/// Decrements the global waiting count however the wait ends.
struct WaitGuard<'a>(&'a GlobalQueue);
impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, AtomicOrdering::AcqRel);
    }
}

#[derive(Debug)]
pub struct Scheduler {
    lanes: [Arc<Lane>; 4],
    global: Arc<GlobalQueue>,
}

/// Lane limits for one provider.
#[derive(Debug, Clone, Copy)]
pub struct LaneLimits {
    pub max_in_flight: usize,
    pub queue_limit: usize,
}

impl Scheduler {
    /// `limits` is indexed by [`ProviderId::index`].
    #[must_use]
    pub fn new(global_limit: usize, limits: [LaneLimits; 4]) -> Self {
        let global = Arc::new(GlobalQueue {
            limit: global_limit,
            waiting: AtomicUsize::new(0),
        });
        let lanes = ProviderId::ALL.map(|p| {
            let l = limits[p.index()];
            Arc::new(Lane {
                provider: p,
                max_in_flight: l.max_in_flight.max(1),
                queue_limit: l.queue_limit,
                state: Mutex::new(LaneState::default()),
                global: Arc::clone(&global),
                admitted: AtomicU64::new(0),
                queued_total: AtomicU64::new(0),
                rejected: AtomicU64::new(0),
                timed_out: AtomicU64::new(0),
                max_in_flight_seen: AtomicUsize::new(0),
            })
        });
        Self { lanes, global }
    }

    /// Wait for a permit for `provider` at `priority`.
    pub async fn acquire(
        &self,
        provider: ProviderId,
        priority: Priority,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Permit, GatewayError> {
        let lane = &self.lanes[provider.index()];
        let rx = {
            let mut st = lane.lock();
            if st.in_flight < lane.max_in_flight && st.waiters.is_empty() {
                st.in_flight += 1;
                lane.note_in_flight(st.in_flight);
                lane.admitted.fetch_add(1, AtomicOrdering::Relaxed);
                return Ok(Permit {
                    lane: Some(Arc::clone(lane)),
                });
            }
            if st.waiters.len() >= lane.queue_limit {
                // Drop entries whose callers already gave up before refusing.
                st.waiters.retain(|w| !w.tx.is_closed());
            }
            let global_full =
                lane.global.waiting.load(AtomicOrdering::Acquire) >= lane.global.limit;
            if st.waiters.len() >= lane.queue_limit || global_full {
                lane.rejected.fetch_add(1, AtomicOrdering::Relaxed);
                return Err(GatewayError::QueueFull { provider });
            }
            let (tx, rx) = oneshot::channel();
            st.seq = st.seq.wrapping_add(1);
            let seq = st.seq;
            st.waiters.push(Waiter {
                rank: priority.rank(),
                seq,
                tx,
            });
            lane.global.waiting.fetch_add(1, AtomicOrdering::AcqRel);
            lane.queued_total.fetch_add(1, AtomicOrdering::Relaxed);
            rx
        };
        let _guard = WaitGuard(&lane.global);
        tokio::select! {
            biased;
            got = rx => match got {
                Ok(permit) => {
                    lane.admitted.fetch_add(1, AtomicOrdering::Relaxed);
                    lane.note_in_flight(lane.lock().in_flight);
                    Ok(permit)
                }
                Err(_) => Err(GatewayError::Cancelled),
            },
            () = cancel.cancelled() => Err(GatewayError::Cancelled),
            () = tokio::time::sleep_until(deadline) => {
                lane.timed_out.fetch_add(1, AtomicOrdering::Relaxed);
                Err(GatewayError::DeadlineExceeded)
            }
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<LaneSnapshot> {
        self.lanes
            .iter()
            .map(|l| {
                let st = l.lock();
                LaneSnapshot {
                    provider: l.provider,
                    max_in_flight: l.max_in_flight,
                    queue_limit: l.queue_limit,
                    in_flight: st.in_flight,
                    queued: st.waiters.iter().filter(|w| !w.tx.is_closed()).count(),
                    admitted: l.admitted.load(AtomicOrdering::Relaxed),
                    queued_total: l.queued_total.load(AtomicOrdering::Relaxed),
                    rejected: l.rejected.load(AtomicOrdering::Relaxed),
                    timed_out: l.timed_out.load(AtomicOrdering::Relaxed),
                    peak_in_flight: l.max_in_flight_seen.load(AtomicOrdering::Relaxed),
                }
            })
            .collect()
    }

    #[must_use]
    pub fn global_waiting(&self) -> usize {
        self.global.waiting.load(AtomicOrdering::Acquire)
    }

    #[must_use]
    pub fn global_limit(&self) -> usize {
        self.global.limit
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn sched(in_flight: usize, queue: usize, global: usize) -> Arc<Scheduler> {
        let l = LaneLimits {
            max_in_flight: in_flight,
            queue_limit: queue,
        };
        Arc::new(Scheduler::new(global, [l; 4]))
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[tokio::test]
    async fn bounds_concurrency_and_serves_by_priority() {
        let s = sched(1, 8, 64);
        let c = CancellationToken::new();
        let first = s
            .acquire(ProviderId::Helius, Priority::P3, far(), &c)
            .await
            .unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut tasks = Vec::new();
        for p in [Priority::P5, Priority::P1, Priority::P3, Priority::P0] {
            let s = Arc::clone(&s);
            let order = Arc::clone(&order);
            let c = c.clone();
            tasks.push(tokio::spawn(async move {
                let _permit = s.acquire(ProviderId::Helius, p, far(), &c).await.unwrap();
                order.lock().unwrap().push(p);
            }));
            tokio::task::yield_now().await;
        }
        while s.snapshot()[0].queued < 4 {
            tokio::task::yield_now().await;
        }
        drop(first);
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(
            *order.lock().unwrap(),
            vec![Priority::P0, Priority::P1, Priority::P3, Priority::P5]
        );
        let snap = s.snapshot()[0];
        assert_eq!(snap.in_flight, 0);
        assert_eq!(snap.peak_in_flight, 1);
    }

    #[tokio::test]
    async fn full_queues_are_refused_immediately() {
        let s = sched(1, 1, 64);
        let c = CancellationToken::new();
        let _held = s
            .acquire(ProviderId::Jupiter, Priority::P3, far(), &c)
            .await
            .unwrap();
        let s2 = Arc::clone(&s);
        let c2 = c.clone();
        let waiter = tokio::spawn(async move {
            s2.acquire(ProviderId::Jupiter, Priority::P3, far(), &c2)
                .await
                .map(|_| ())
        });
        while s.snapshot()[1].queued < 1 {
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            s.acquire(ProviderId::Jupiter, Priority::P0, far(), &c)
                .await,
            Err(GatewayError::QueueFull { .. })
        ));
        c.cancel();
        assert!(matches!(
            waiter.await.unwrap(),
            Err(GatewayError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn global_limit_spans_providers() {
        let s = sched(1, 8, 1);
        let c = CancellationToken::new();
        let _a = s
            .acquire(ProviderId::Helius, Priority::P3, far(), &c)
            .await
            .unwrap();
        let _b = s
            .acquire(ProviderId::Jupiter, Priority::P3, far(), &c)
            .await
            .unwrap();
        let s2 = Arc::clone(&s);
        let c2 = c.clone();
        let t = tokio::spawn(async move {
            s2.acquire(ProviderId::Helius, Priority::P3, far(), &c2)
                .await
                .map(|_| ())
        });
        while s.global_waiting() < 1 {
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            s.acquire(ProviderId::Jupiter, Priority::P3, far(), &c)
                .await,
            Err(GatewayError::QueueFull { .. })
        ));
        c.cancel();
        let _ = t.await;
        assert_eq!(s.global_waiting(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_and_abandoned_waiters_never_leak_slots() {
        let s = sched(1, 8, 64);
        let c = CancellationToken::new();
        let held = s
            .acquire(ProviderId::Helius, Priority::P3, far(), &c)
            .await
            .unwrap();
        let soon = Instant::now() + Duration::from_millis(50);
        assert!(matches!(
            s.acquire(ProviderId::Helius, Priority::P3, soon, &c).await,
            Err(GatewayError::DeadlineExceeded)
        ));
        drop(held);
        // The timed-out waiter's entry is skipped; the slot is free again.
        let again = s
            .acquire(ProviderId::Helius, Priority::P3, far(), &c)
            .await
            .unwrap();
        drop(again);
        assert_eq!(s.snapshot()[0].in_flight, 0);
    }
}
