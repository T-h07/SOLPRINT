//! Response cache with explicit freshness.
//!
//! Entries are type-erased (`Arc<dyn Any>`) so one bounded cache serves every
//! operation. Each entry carries its own *fresh* and *keep* windows; moka
//! evicts an entry once its keep window ends. Negative entries record
//! "the provider said this does not exist" for a short time, so repeated
//! lookups of a missing account cost nothing.
//!
//! Freshness on read (age measured on the monotonic clock):
//! `age ≤ fresh_for` → `FRESH`; `age ≤ keep_for` → `STALE`; otherwise
//! `EXPIRED` (never served).

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use moka::Expiry;
use moka::future::Cache;
use solprint_core::UtcTimestamp;
use tokio::time::Instant;

use crate::context::{Freshness, ProviderId};

#[derive(Debug, Clone, Copy)]
pub struct CacheTtl {
    pub fresh_for: Duration,
    pub keep_for: Duration,
    /// How long a "not found" answer is remembered (zero disables).
    pub negative_for: Duration,
}

impl CacheTtl {
    #[must_use]
    pub const fn new(fresh_for: Duration, keep_for: Duration, negative_for: Duration) -> Self {
        Self {
            fresh_for,
            keep_for,
            negative_for,
        }
    }

    /// Never cached (quotes, health checks).
    pub const NONE: Self = Self::new(Duration::ZERO, Duration::ZERO, Duration::ZERO);
}

#[derive(Clone)]
pub struct CacheEntry {
    /// `None` for a negative entry.
    pub value: Option<Arc<dyn Any + Send + Sync>>,
    pub provider: ProviderId,
    pub stored_at: Instant,
    pub observed_at: UtcTimestamp,
    pub source_timestamp: Option<UtcTimestamp>,
    pub context_slot: Option<u64>,
    pub fresh_for: Duration,
    pub keep_for: Duration,
}

impl std::fmt::Debug for CacheEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheEntry")
            .field("negative", &self.value.is_none())
            .field("provider", &self.provider)
            .field("fresh_for", &self.fresh_for)
            .field("keep_for", &self.keep_for)
            .finish_non_exhaustive()
    }
}

impl CacheEntry {
    #[must_use]
    pub fn age(&self) -> Duration {
        self.stored_at.elapsed()
    }

    #[must_use]
    pub fn freshness(&self) -> Freshness {
        let age = self.age();
        if age <= self.fresh_for {
            Freshness::Fresh
        } else if age <= self.keep_for {
            Freshness::Stale
        } else {
            Freshness::Expired
        }
    }

    /// Fresh *for this caller*: within the entry's window and the caller's
    /// `max_age` (if any).
    #[must_use]
    pub fn satisfies(&self, max_age: Option<Duration>) -> bool {
        let age = self.age();
        age <= self.fresh_for && max_age.is_none_or(|m| age <= m)
    }
}

struct PerEntryExpiry;

impl Expiry<String, Arc<CacheEntry>> for PerEntryExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &Arc<CacheEntry>,
        _created_at: std::time::Instant,
    ) -> Option<Duration> {
        Some(value.keep_for.max(Duration::from_millis(1)))
    }
}

#[derive(Debug, Default)]
struct Counters {
    hits: AtomicU64,
    stale_hits: AtomicU64,
    negative_hits: AtomicU64,
    misses: AtomicU64,
    inserts: AtomicU64,
}

/// Counters for one namespace (operation).
#[derive(Debug, Clone, Copy, Default)]
pub struct NamespaceStats {
    pub hits: u64,
    pub stale_hits: u64,
    pub negative_hits: u64,
    pub misses: u64,
    pub inserts: u64,
}

#[derive(Debug, Clone)]
pub struct CacheSnapshot {
    pub max_entries: u64,
    pub entries: u64,
    pub namespaces: Vec<(&'static str, NamespaceStats)>,
}

/// What a lookup found.
#[derive(Debug, Clone)]
pub enum Lookup {
    Miss,
    Hit(Arc<CacheEntry>),
}

#[derive(Clone)]
pub struct ResponseCache {
    inner: Option<Cache<String, Arc<CacheEntry>>>,
    max_entries: u64,
    counters: Arc<Mutex<HashMap<&'static str, Arc<Counters>>>>,
}

impl std::fmt::Debug for ResponseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseCache")
            .field("max_entries", &self.max_entries)
            .finish_non_exhaustive()
    }
}

impl ResponseCache {
    /// `max_entries == 0` disables caching entirely.
    #[must_use]
    pub fn new(max_entries: u64) -> Self {
        let inner = (max_entries > 0).then(|| {
            Cache::builder()
                .max_capacity(max_entries)
                .expire_after(PerEntryExpiry)
                .build()
        });
        Self {
            inner,
            max_entries,
            counters: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn counters(&self, ns: &'static str) -> Arc<Counters> {
        let mut map = self
            .counters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(map.entry(ns).or_default())
    }

    pub async fn get(&self, ns: &'static str, key: &str) -> Lookup {
        let c = self.counters(ns);
        let Some(cache) = &self.inner else {
            c.misses.fetch_add(1, Ordering::Relaxed);
            return Lookup::Miss;
        };
        match cache.get(key).await {
            Some(e) if e.freshness() != Freshness::Expired => Lookup::Hit(e),
            _ => {
                c.misses.fetch_add(1, Ordering::Relaxed);
                Lookup::Miss
            }
        }
    }

    /// Record how a hit was used (fresh / stale / negative).
    pub fn note_hit(&self, ns: &'static str, entry: &CacheEntry) {
        let c = self.counters(ns);
        if entry.value.is_none() {
            c.negative_hits.fetch_add(1, Ordering::Relaxed);
        } else if entry.freshness() == Freshness::Fresh {
            c.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            c.stale_hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn note_miss(&self, ns: &'static str) {
        self.counters(ns).misses.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn put(&self, ns: &'static str, key: String, entry: CacheEntry) {
        let Some(cache) = &self.inner else { return };
        if entry.keep_for.is_zero() {
            return;
        }
        self.counters(ns).inserts.fetch_add(1, Ordering::Relaxed);
        cache.insert(key, Arc::new(entry)).await;
    }

    pub async fn invalidate(&self, key: &str) {
        if let Some(cache) = &self.inner {
            cache.invalidate(key).await;
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> CacheSnapshot {
        let map = self
            .counters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut namespaces: Vec<_> = map
            .iter()
            .map(|(ns, c)| {
                (
                    *ns,
                    NamespaceStats {
                        hits: c.hits.load(Ordering::Relaxed),
                        stale_hits: c.stale_hits.load(Ordering::Relaxed),
                        negative_hits: c.negative_hits.load(Ordering::Relaxed),
                        misses: c.misses.load(Ordering::Relaxed),
                        inserts: c.inserts.load(Ordering::Relaxed),
                    },
                )
            })
            .collect();
        namespaces.sort_by_key(|(ns, _)| *ns);
        CacheSnapshot {
            max_entries: self.max_entries,
            entries: self.inner.as_ref().map_or(0, Cache::entry_count),
            namespaces,
        }
    }

    /// Run pending maintenance so `entry_count` is accurate (tests, snapshots).
    pub async fn sync(&self) {
        if let Some(c) = &self.inner {
            c.run_pending_tasks().await;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn entry(value: Option<u64>, fresh: u64, keep: u64) -> CacheEntry {
        CacheEntry {
            value: value.map(|v| Arc::new(v) as Arc<dyn Any + Send + Sync>),
            provider: ProviderId::Helius,
            stored_at: Instant::now(),
            observed_at: UtcTimestamp::now(),
            source_timestamp: None,
            context_slot: Some(1),
            fresh_for: Duration::from_millis(fresh),
            keep_for: Duration::from_millis(keep),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn freshness_moves_from_fresh_to_stale_to_expired() {
        let e = entry(Some(1), 100, 300);
        assert_eq!(e.freshness(), Freshness::Fresh);
        assert!(e.satisfies(None));
        assert!(!e.satisfies(Some(Duration::ZERO)) || e.age().is_zero());
        tokio::time::advance(Duration::from_millis(150)).await;
        assert_eq!(e.freshness(), Freshness::Stale);
        assert!(!e.satisfies(None));
        tokio::time::advance(Duration::from_millis(200)).await;
        assert_eq!(e.freshness(), Freshness::Expired);
    }

    #[tokio::test]
    async fn stores_typed_and_negative_entries() {
        let c = ResponseCache::new(100);
        c.put("balance", "a".into(), entry(Some(42), 60_000, 60_000))
            .await;
        c.put("account", "b".into(), entry(None, 60_000, 60_000))
            .await;
        let Lookup::Hit(hit) = c.get("balance", "a").await else {
            panic!("expected hit")
        };
        let v = hit.value.clone().unwrap().downcast::<u64>().unwrap();
        assert_eq!(*v, 42);
        c.note_hit("balance", &hit);
        let Lookup::Hit(neg) = c.get("account", "b").await else {
            panic!("expected negative hit")
        };
        assert!(neg.value.is_none());
        c.note_hit("account", &neg);
        assert!(matches!(c.get("balance", "zzz").await, Lookup::Miss));
        c.sync().await;
        let s = c.snapshot();
        assert_eq!(s.entries, 2);
        let bal = s
            .namespaces
            .iter()
            .find(|(n, _)| *n == "balance")
            .unwrap()
            .1;
        assert_eq!((bal.hits, bal.misses, bal.inserts), (1, 1, 1));
    }

    #[tokio::test]
    async fn zero_capacity_disables_the_cache() {
        let c = ResponseCache::new(0);
        c.put("x", "k".into(), entry(Some(1), 1_000, 1_000)).await;
        assert!(matches!(c.get("x", "k").await, Lookup::Miss));
    }
}
