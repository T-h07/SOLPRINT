//! The Gateway facade: construction, lifecycle and persistence.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use solprint_config::Secret;
use solprint_core::UtcTimestamp;
use time::OffsetDateTime;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::breaker::CircuitBreaker;
use crate::budget::CreditBudget;
use crate::cache::ResponseCache;
use crate::context::ProviderId;
use crate::engine::{Engine, ProviderRuntime};
use crate::error::GatewayError;
use crate::health::HealthTracker;
use crate::jsonrpc::{IdSource, RpcEndpoint};
use crate::ratelimit::{Rate, RateLimiter};
use crate::scheduler::{LaneLimits, Scheduler};
use crate::settings::GatewaySettings;
use crate::store::GatewayStore;
use crate::stream::{StreamConfig, StreamHandle, StreamManager};
use crate::transport::{HttpTransport, TransportSettings};

/// Public RPC fallback concurrency (documented: 1 in flight).
const FALLBACK_IN_FLIGHT: usize = 1;
const RETENTION_EVERY: Duration = Duration::from_secs(3_600);

#[derive(Debug)]
pub(crate) struct Limiters {
    pub helius: RateLimiter,
    pub jupiter: RateLimiter,
    pub dex_pairs: RateLimiter,
    pub dex_discovery: RateLimiter,
    pub solana: RateLimiter,
}

impl Limiters {
    pub fn all(&self) -> [&RateLimiter; 5] {
        [
            &self.helius,
            &self.jupiter,
            &self.dex_pairs,
            &self.dex_discovery,
            &self.solana,
        ]
    }
}

/// Persistence health, shown on the monitoring API.
#[derive(Debug, Clone, Default)]
pub struct PersistenceStatus {
    pub last_flush_at: Option<UtcTimestamp>,
    pub last_flush_ok: Option<bool>,
    pub flush_failures: u64,
    pub last_prune_at: Option<UtcTimestamp>,
    pub ledger_loaded: bool,
}

pub(crate) struct Inner {
    pub settings: GatewaySettings,
    pub transport: HttpTransport,
    pub engine: Engine,
    pub limiters: Limiters,
    pub helius: Option<RpcEndpoint>,
    pub fallback: Option<RpcEndpoint>,
    pub ids: IdSource,
    pub store: Arc<dyn GatewayStore>,
    pub streams: OnceLock<StreamHandle>,
    pub started_at: UtcTimestamp,
    pub persistence: Mutex<PersistenceStatus>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gateway")
            .field("enabled", &self.settings.enabled)
            .finish_non_exhaustive()
    }
}

/// Cheaply clonable handle to GATEWAY.
#[derive(Debug, Clone)]
pub struct Gateway {
    pub(crate) inner: Arc<Inner>,
}

fn helius_url(base: &Url, key: &Secret<String>) -> Url {
    let mut url = base.clone();
    url.query_pairs_mut()
        .clear()
        .append_pair("api-key", key.expose());
    url
}

impl Gateway {
    /// Build GATEWAY. Nothing is contacted until a caller makes a request
    /// (and streams only start if enabled, in [`Gateway::start`]).
    pub fn new(
        settings: GatewaySettings,
        store: Arc<dyn GatewayStore>,
    ) -> Result<Self, GatewayError> {
        let transport = HttpTransport::new(&TransportSettings {
            connect_timeout: settings.connect_timeout,
            max_response_bytes: settings.max_response_bytes,
            allowed_origins: settings.allowed_origins(),
            allow_insecure_loopback: settings.allow_insecure_loopback,
        })?;

        let gate = |ok: bool, reason: &'static str| if ok { Ok(()) } else { Err(reason) };
        let on = settings.enabled;
        let helius_enabled = gate(on, "GATEWAY_ENABLED=false").and(gate(
            settings.helius.api_key.is_some(),
            "HELIUS_API_KEY not configured",
        ));
        let runtime = |id, enabled, budget| ProviderRuntime {
            id,
            enabled,
            breaker: CircuitBreaker::new(id),
            health: HealthTracker::default(),
            budget,
        };
        let now = OffsetDateTime::now_utc();
        let providers = [
            runtime(
                ProviderId::Helius,
                helius_enabled,
                Some(CreditBudget::new(
                    ProviderId::Helius,
                    settings.helius.budget,
                    now,
                )),
            ),
            runtime(ProviderId::Jupiter, gate(on, "GATEWAY_ENABLED=false"), None),
            runtime(
                ProviderId::DexScreener,
                gate(on, "GATEWAY_ENABLED=false"),
                None,
            ),
            runtime(
                ProviderId::SolanaRpc,
                gate(on, "GATEWAY_ENABLED=false").and(gate(
                    settings.fallback.enabled,
                    "SOLANA_FALLBACK_ENABLED=false",
                )),
                None,
            ),
        ];
        let q = settings.provider_queue_limit;
        let lane = |max_in_flight| LaneLimits {
            max_in_flight,
            queue_limit: q,
        };
        let scheduler = Scheduler::new(
            settings.global_queue_limit,
            [
                lane(settings.helius.max_in_flight),
                lane(settings.jupiter.max_in_flight),
                lane(settings.dexscreener.max_in_flight),
                lane(FALLBACK_IN_FLIGHT),
            ],
        );
        let engine = Engine::new(
            providers,
            scheduler,
            ResponseCache::new(settings.cache_max_entries),
        );
        let limiters = Limiters {
            helius: RateLimiter::new(
                "helius.rpc",
                ProviderId::Helius,
                Rate::per_second(settings.helius.rps),
            ),
            jupiter: RateLimiter::new(
                "jupiter.general",
                ProviderId::Jupiter,
                Rate::per_minute_even(settings.jupiter.rpm),
            ),
            dex_pairs: RateLimiter::new(
                "dexscreener.pairs",
                ProviderId::DexScreener,
                Rate {
                    count: settings.dexscreener.rpm,
                    period: Duration::from_secs(60),
                    burst: 4,
                },
            ),
            dex_discovery: RateLimiter::new(
                "dexscreener.discovery",
                ProviderId::DexScreener,
                // Documented 60/min; GATEWAY uses half.
                Rate::per_minute_even(30),
            ),
            solana: RateLimiter::new(
                "solana_rpc.public",
                ProviderId::SolanaRpc,
                Rate::per_second(settings.fallback.rps),
            ),
        };
        let helius = settings.helius.api_key.as_ref().map(|k| RpcEndpoint {
            provider: ProviderId::Helius,
            url: Secret::new(helius_url(&settings.helius.rpc_url, k)),
        });
        let fallback = settings.fallback.enabled.then(|| RpcEndpoint {
            provider: ProviderId::SolanaRpc,
            url: settings.fallback.url.clone(),
        });
        Ok(Self {
            inner: Arc::new(Inner {
                settings,
                transport,
                engine,
                limiters,
                helius,
                fallback,
                ids: IdSource::default(),
                store,
                streams: OnceLock::new(),
                started_at: UtcTimestamp::now(),
                persistence: Mutex::new(PersistenceStatus::default()),
            }),
        })
    }

    #[must_use]
    pub fn settings(&self) -> &GatewaySettings {
        &self.inner.settings
    }

    #[must_use]
    pub fn streams(&self) -> Option<&StreamHandle> {
        self.inner.streams.get()
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.inner.engine
    }

    pub(crate) fn budget(&self) -> Option<&CreditBudget> {
        self.inner
            .engine
            .runtime(ProviderId::Helius)
            .budget
            .as_ref()
    }

    pub(crate) fn persistence(&self) -> PersistenceStatus {
        self.inner
            .persistence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn update_persistence(&self, f: impl FnOnce(&mut PersistenceStatus)) {
        let mut p = self
            .inner
            .persistence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut p);
    }

    /// Start background maintenance (ledger load, periodic flush, retention)
    /// and, if enabled, the stream manager. Returns when `shutdown` fires and
    /// the final flush has run.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let this = self.clone();
        if self.inner.settings.streams_enabled
            && self.inner.settings.enabled
            && let Some(key) = &self.inner.settings.helius.api_key
        {
            let handle = StreamManager::spawn(
                StreamConfig::from_settings(&self.inner.settings, key),
                self.clone(),
                shutdown.child_token(),
            );
            let _ = self.inner.streams.set(handle);
        }
        tokio::spawn(async move { this.maintenance(shutdown).await })
    }

    async fn maintenance(&self, shutdown: CancellationToken) {
        self.load_ledger().await;
        let mut flush = tokio::time::interval(self.inner.settings.flush_interval);
        flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        flush.tick().await;
        let mut prune = tokio::time::interval(RETENTION_EVERY);
        prune.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = flush.tick() => {
                    if !self.budget().is_some_and(CreditBudget::is_loaded) {
                        self.load_ledger().await;
                    }
                    self.flush().await;
                }
                _ = prune.tick() => self.prune().await,
            }
        }
        self.flush().await;
        tracing::info!("gateway persistence flushed on shutdown");
    }

    /// Load the persisted credit total for the current cycle.
    pub async fn load_ledger(&self) {
        let Some(budget) = self.budget() else { return };
        let cycle = budget.cycle(OffsetDateTime::now_utc());
        let start = cycle.start_ts();
        match self
            .inner
            .store
            .billing_cycle(ProviderId::Helius.as_str(), &start)
            .await
        {
            Ok(row) => {
                let used = row.map_or(0, |r| r.credits_used);
                budget.load(cycle, used);
                self.update_persistence(|p| p.ledger_loaded = true);
                tracing::info!(
                    provider = "helius",
                    credits_used = used,
                    "credit ledger loaded"
                );
            }
            Err(e) => {
                self.update_persistence(|p| p.ledger_loaded = false);
                tracing::warn!(reason = %e, "credit ledger unavailable; budget state is ACCOUNT_USAGE_UNKNOWN");
            }
        }
    }

    /// Persist pending credits, usage and metrics. Failed writes are kept
    /// and retried on the next flush.
    pub async fn flush(&self) {
        let mut ok = true;
        if let Some(budget) = self.budget() {
            let policy = budget.policy();
            let pending = budget.take_pending();
            let mut failed = Vec::new();
            for p in pending {
                match self
                    .inner
                    .store
                    .add_cycle_credits(
                        ProviderId::Helius.as_str(),
                        &p.cycle.start_ts(),
                        &p.cycle.end_ts(),
                        policy.monthly_credits,
                        policy.reserve_credits(),
                        p.credits,
                    )
                    .await
                {
                    Ok(total) => budget.flush_succeeded(p.cycle, total, UtcTimestamp::now()),
                    Err(_) => failed.push(p),
                }
            }
            if !failed.is_empty() {
                ok = false;
                budget.flush_failed(failed);
            }
        }
        let (usage, metrics) = self.engine().metrics.drain();
        let usage_ok = self.inner.store.record_usage(&usage).await.is_ok();
        let metrics_ok = self.inner.store.record_metrics(&metrics).await.is_ok();
        if !usage_ok || !metrics_ok {
            ok = false;
            self.engine().metrics.restore(
                if usage_ok { Vec::new() } else { usage },
                if metrics_ok { Vec::new() } else { metrics },
            );
        }
        if let Some(streams) = self.streams() {
            for cp in streams.take_checkpoints() {
                if self.inner.store.upsert_checkpoint(&cp).await.is_err() {
                    ok = false;
                    streams.restore_checkpoint(cp);
                }
            }
        }
        self.update_persistence(|p| {
            p.last_flush_at = Some(UtcTimestamp::now());
            p.last_flush_ok = Some(ok);
            if !ok {
                p.flush_failures = p.flush_failures.saturating_add(1);
            }
        });
        if !ok {
            tracing::warn!("gateway persistence flush failed; will retry");
        }
    }

    /// Apply retention to usage and metric buckets.
    pub async fn prune(&self) {
        let now = OffsetDateTime::now_utc();
        let before = |d: Duration| {
            UtcTimestamp::from_offset_datetime(
                now - time::Duration::seconds(i64::try_from(d.as_secs()).unwrap_or(i64::MAX / 2)),
            )
        };
        let usage_before = before(self.inner.settings.usage_retention);
        let metrics_before = before(self.inner.settings.metrics_retention);
        match self.inner.store.prune(&usage_before, &metrics_before).await {
            Ok(out) => {
                self.update_persistence(|p| p.last_prune_at = Some(UtcTimestamp::now()));
                if out.usage_rows + out.metric_rows > 0 {
                    tracing::info!(
                        usage_rows = out.usage_rows,
                        metric_rows = out.metric_rows,
                        "gateway retention applied"
                    );
                }
            }
            Err(e) => tracing::warn!(reason = %e, "gateway retention failed"),
        }
    }

    /// Read stored aggregates (monitoring API). Never contacts a provider.
    pub async fn usage_history(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<solprint_storage::UsageHistoryRow>, solprint_storage::StorageError> {
        self.inner.store.usage_history(since).await
    }

    /// Read stored metric aggregates (monitoring API). Never contacts a provider.
    pub async fn metric_window(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<solprint_storage::MetricDelta>, solprint_storage::StorageError> {
        self.inner.store.metric_window(since).await
    }

    /// Stored stream checkpoints (monitoring API).
    pub async fn stored_checkpoints(
        &self,
    ) -> Result<Vec<solprint_storage::StreamCheckpointRow>, solprint_storage::StorageError> {
        self.inner.store.checkpoints().await
    }
}
