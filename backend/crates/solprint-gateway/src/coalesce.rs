//! Single-flight request coalescing.
//!
//! Concurrent identical requests (same operation, same parameters, same
//! commitment) share one upstream call: the first caller (the *leader*) runs
//! it, everyone else waits for the leader's result. The leader's context
//! governs the shared call; a follower whose own deadline passes stops
//! waiting without affecting the others. If the leader is cancelled, one
//! follower takes over, so a cancelled caller never fails the others.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::error::GatewayError;

pub type Shared = Arc<dyn Any + Send + Sync>;
type Outcome = Option<Result<Shared, GatewayError>>;

#[derive(Default)]
pub struct SingleFlight {
    in_flight: Mutex<HashMap<String, watch::Receiver<Outcome>>>,
    coalesced: AtomicU64,
}

impl std::fmt::Debug for SingleFlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleFlight")
            .field("coalesced", &self.coalesced.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// Removes the leader's entry however the leader finishes.
struct LeaderGuard<'a> {
    owner: &'a SingleFlight,
    key: &'a str,
}

impl Drop for LeaderGuard<'_> {
    fn drop(&mut self) {
        self.owner.lock().remove(self.key);
    }
}

enum Role {
    Leader(watch::Sender<Outcome>),
    Follower(watch::Receiver<Outcome>),
}

impl SingleFlight {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, watch::Receiver<Outcome>>> {
        self.in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Number of requests that were answered by another caller's call.
    #[must_use]
    pub fn coalesced(&self) -> u64 {
        self.coalesced.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.lock().len()
    }

    /// Run `make()` once per key at a time. The returned flag is `true` when
    /// this caller received another caller's result.
    pub async fn run<F, Fut>(&self, key: &str, make: F) -> (Result<Shared, GatewayError>, bool)
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<Shared, GatewayError>>,
    {
        loop {
            let role = {
                let mut map = self.lock();
                match map.get(key) {
                    Some(rx) => Role::Follower(rx.clone()),
                    None => {
                        let (tx, rx) = watch::channel(None);
                        map.insert(key.to_owned(), rx);
                        Role::Leader(tx)
                    }
                }
            };
            match role {
                Role::Leader(tx) => {
                    let _guard = LeaderGuard { owner: self, key };
                    let result = make().await;
                    let _ = tx.send(Some(result.clone()));
                    return (result, false);
                }
                Role::Follower(mut rx) => {
                    let done = rx.wait_for(Option::is_some).await;
                    if let Ok(value) = done
                        && let Some(result) = value.clone()
                    {
                        self.coalesced.fetch_add(1, Ordering::Relaxed);
                        return (result, true);
                    }
                    // Leader dropped without a result: compete to lead.
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn concurrent_identical_requests_share_one_call() {
        let sf = Arc::new(SingleFlight::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let sf = Arc::clone(&sf);
            let calls = Arc::clone(&calls);
            tasks.push(tokio::spawn(async move {
                let (r, _) = sf
                    .run("k", || {
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            Ok(Arc::new(7_u32) as Shared)
                        }
                    })
                    .await;
                *r.unwrap().downcast::<u32>().unwrap()
            }));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap(), 7);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(sf.coalesced(), 9);
        assert_eq!(sf.in_flight(), 0);
    }

    #[tokio::test]
    async fn errors_are_shared_and_a_cancelled_leader_hands_over() {
        let sf = Arc::new(SingleFlight::default());
        let (r, _) = sf.run("e", || async { Err(GatewayError::NoRoute) }).await;
        assert_eq!(r.unwrap_err(), GatewayError::NoRoute);

        let sf2 = Arc::clone(&sf);
        let leader = tokio::spawn(async move {
            sf2.run("c", || async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(Arc::new(1_u8) as Shared)
            })
            .await
        });
        while sf.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        let sf3 = Arc::clone(&sf);
        let follower = tokio::spawn(async move {
            sf3.run("c", || async { Ok(Arc::new(2_u8) as Shared) })
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        leader.abort();
        let (r, _) = follower.await.unwrap();
        assert_eq!(*r.unwrap().downcast::<u8>().unwrap(), 2);
    }
}
