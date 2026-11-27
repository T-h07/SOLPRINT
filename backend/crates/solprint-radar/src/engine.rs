//! The RADAR engine: discovery cycles, the budget-aware job scheduler, DATA
//! ENGINE event consumption, wallet evaluation, monitoring rotation and
//! maintenance. Every step is bounded, restart-safe (state lives in
//! PostgreSQL) and idempotent.
//!
//! RADAR never fetches transactions itself: sampling and history requests are
//! DATA ENGINE backfills, and RADAR learns about wallets only from the swaps
//! the DATA ENGINE verified and published through its outbox.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;
use sha2::{Digest, Sha256};
use solprint_config::{DataEngineConfig, RadarConfig};
use solprint_core::UtcTimestamp;
use solprint_data_engine::mapping::events as de_events;
use solprint_data_engine::programs;
use solprint_storage::data_engine::NewBackfill;
use solprint_storage::radar::{
    self as st, CycleFinish, EvaluationWrite, EvidenceOutcome, EvidenceWrite, JobFilter, JobRow,
    MarketSelection, MarketUpsert, NewCycle, NewJob, ObservationWrite, PoolUpsert, SourceRunWrite,
    SwapForRadar, TransitionWrite, WalletRow,
};
use solprint_storage::{Database, StorageError};
use time::OffsetDateTime;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::budget::{self, Admission, BudgetView, Mode};
use crate::classify::{self, TokenEvidence};
use crate::events as ev;
use crate::feed::{Listing, MarketFeed};
use crate::lifecycle::{self, LifecycleInputs, Need};
use crate::market::{self, MarketCandidate, PoolView, Thresholds};
use crate::model::{
    Bucket, LIFECYCLE_POLICY, PRIORITY_POLICY, Stage, coverage, eligibility as el, is_quote_like,
    lifecycle as lc, monitoring as mon, source, work,
};
use crate::monitoring::{self as rot, RotationPolicy};
use crate::scoring::{
    self, AccountRole, Coverage, EvidenceSummary, PriorityInputs, QualificationInputs,
    QualificationPolicy, ScreeningFacts,
};
use crate::screening::{self, PreBalances, Side, Trade};

/// Approximate slots per day (400 ms slots). Used only to bound history
/// walks by slot; a documented assumption, never a correctness input.
pub const SLOTS_PER_DAY_ESTIMATE: u64 = 216_000;
const LEASE: Duration = Duration::from_secs(300);
const EVENT_BATCH: i64 = 200;
const EVAL_BATCH: i64 = 25;
const SWAP_LIMIT: i64 = 5_000;
/// A job whose backfill finished but whose transactions are still being
/// ingested is closed with `UNRESOLVED_GAPS` after this long.
const INGEST_GRACE: Duration = Duration::from_secs(1_800);
const DEFER_SHORT: Duration = Duration::from_secs(300);
const DEFER_LONG: Duration = Duration::from_secs(3_600);
/// Built-in consumer of RADAR's own outbox (structured log; gates retention).
pub const LOG_CONSUMER: &str = "radar_log";
pub const MONITORING_REFRESH_SIGNATURES: i32 = 25;
pub const DORMANCY_RECHECK_SIGNATURES: i32 = 25;

#[derive(Debug, thiserror::Error)]
pub enum RadarError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("invalid input: {0}")]
    Invalid(&'static str),
}

/// Process counters (since start). Persistent counts come from PostgreSQL.
#[derive(Debug, Default)]
pub struct Metrics {
    pub cycles: AtomicU64,
    pub cycles_deferred: AtomicU64,
    pub source_failures: AtomicU64,
    pub provider_requests: AtomicU64,
    pub events_processed: AtomicU64,
    pub event_failures: AtomicU64,
    pub evidence_recorded: AtomicU64,
    pub evidence_invalidated: AtomicU64,
    pub wallets_discovered: AtomicU64,
    pub evaluations: AtomicU64,
    pub evaluation_conflicts: AtomicU64,
    pub jobs_started: AtomicU64,
    pub jobs_completed: AtomicU64,
    pub jobs_deferred: AtomicU64,
    pub monitoring_changes: AtomicU64,
    pub errors: AtomicU64,
    last: Mutex<LastTimes>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LastTimes {
    pub cycle_at: Option<UtcTimestamp>,
    pub event_at: Option<UtcTimestamp>,
    pub evaluation_at: Option<UtcTimestamp>,
    pub error_code: Option<String>,
    pub error_at: Option<UtcTimestamp>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub cycles: u64,
    pub cycles_deferred: u64,
    pub source_failures: u64,
    pub provider_requests: u64,
    pub events_processed: u64,
    pub event_failures: u64,
    pub evidence_recorded: u64,
    pub evidence_invalidated: u64,
    pub wallets_discovered: u64,
    pub evaluations: u64,
    pub evaluation_conflicts: u64,
    pub jobs_started: u64,
    pub jobs_completed: u64,
    pub jobs_deferred: u64,
    pub monitoring_changes: u64,
    pub errors: u64,
    pub last: LastTimes,
}

fn inc(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}

impl Metrics {
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        let g = |c: &AtomicU64| c.load(Ordering::Relaxed);
        MetricsSnapshot {
            cycles: g(&self.cycles),
            cycles_deferred: g(&self.cycles_deferred),
            source_failures: g(&self.source_failures),
            provider_requests: g(&self.provider_requests),
            events_processed: g(&self.events_processed),
            event_failures: g(&self.event_failures),
            evidence_recorded: g(&self.evidence_recorded),
            evidence_invalidated: g(&self.evidence_invalidated),
            wallets_discovered: g(&self.wallets_discovered),
            evaluations: g(&self.evaluations),
            evaluation_conflicts: g(&self.evaluation_conflicts),
            jobs_started: g(&self.jobs_started),
            jobs_completed: g(&self.jobs_completed),
            jobs_deferred: g(&self.jobs_deferred),
            monitoring_changes: g(&self.monitoring_changes),
            errors: g(&self.errors),
            last: self
                .last
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    }

    fn touch(&self, f: impl FnOnce(&mut LastTimes)) {
        f(&mut self.last.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn error(&self, code: &str) {
        inc(&self.errors);
        let code = code.to_owned();
        self.touch(|l| {
            l.error_code = Some(code);
            l.error_at = Some(UtcTimestamp::now());
        });
    }
}

/// What the API and CLI may read about a running RADAR (local state only).
pub trait RadarInfo: Send + Sync + std::fmt::Debug {
    fn config(&self) -> &RadarConfig;
    fn metrics(&self) -> MetricsSnapshot;
    /// GATEWAY budget and RADAR's admission (local state, no provider call).
    fn admission(&self, outstanding: u64) -> (Option<BudgetView>, Admission);
    fn live_monitoring_available(&self) -> bool;
}

/// Limits RADAR takes from the DATA ENGINE configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataEngineLimits {
    pub enabled: bool,
    pub backfill_enabled: bool,
    pub auto_ingest: bool,
    pub max_active_targets: usize,
    pub page_size: u16,
}

impl DataEngineLimits {
    #[must_use]
    pub fn of(c: &DataEngineConfig) -> Self {
        Self {
            enabled: c.enabled,
            backfill_enabled: c.backfill_enabled,
            auto_ingest: c.auto_ingest_enabled,
            max_active_targets: c.max_active_targets,
            page_size: c.backfill_page_size.clamp(1, 1000),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleReport {
    pub cycle_id: Option<Uuid>,
    pub status: String,
    pub deferred_reason: Option<String>,
    pub observations: i32,
    pub markets_new: i32,
    pub markets_selected: i32,
    pub samples_scheduled: i32,
    pub credits_estimated: i32,
}

pub struct Radar<F: MarketFeed> {
    db: Database,
    feed: Arc<F>,
    cfg: RadarConfig,
    de: DataEngineLimits,
    owner: Uuid,
    metrics: Arc<Metrics>,
}

impl<F: MarketFeed> std::fmt::Debug for Radar<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Radar")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

impl<F: MarketFeed> RadarInfo for Radar<F> {
    fn config(&self) -> &RadarConfig {
        &self.cfg
    }

    fn metrics(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    fn admission(&self, outstanding: u64) -> (Option<BudgetView>, Admission) {
        let view = self.feed.budget();
        let a = budget::admission(view.as_ref(), self.cfg.normal_budget_share_bps, outstanding);
        (view, a)
    }

    fn live_monitoring_available(&self) -> bool {
        self.de.auto_ingest && self.feed.streams_available()
    }
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn ts(t: OffsetDateTime) -> UtcTimestamp {
    UtcTimestamp::from_offset_datetime(t)
}

fn odt(t: &UtcTimestamp) -> OffsetDateTime {
    *t.as_offset_datetime()
}

fn i32_of(v: impl TryInto<i32>) -> i32 {
    v.try_into().unwrap_or(i32::MAX)
}

fn dec_str(d: Option<Decimal>) -> Option<String> {
    d.map(|v| v.normalize().to_string())
}

fn u32_of(v: Option<u64>) -> Option<i32> {
    v.map(|x| i32::try_from(x).unwrap_or(i32::MAX))
}

/// Addresses that are programs SOLPRINT knows (never traders).
#[must_use]
pub fn is_known_program(a: &str) -> bool {
    programs::protocol_label(a).is_some()
        || programs::is_infrastructure(a)
        || programs::is_token_program(a)
        || matches!(
            a,
            programs::SYSTEM_PROGRAM | programs::ASSOCIATED_TOKEN_PROGRAM | programs::WSOL_MINT
        )
}

/// The protocol of a pool from its on-chain owner program, when the DATA
/// ENGINE decodes that program's swaps. Jupiter is an aggregator, not a
/// pool owner.
#[must_use]
pub fn pool_protocol(owner: &str) -> Option<&'static str> {
    match programs::protocol_label(owner) {
        Some("JUPITER") | None => None,
        Some(p) => Some(p),
    }
}

impl<F: MarketFeed> Radar<F> {
    pub fn new(db: Database, feed: Arc<F>, cfg: RadarConfig, de: DataEngineLimits) -> Self {
        Self {
            db,
            feed,
            cfg,
            de,
            owner: Uuid::new_v4(),
            metrics: Arc::new(Metrics::default()),
        }
    }

    #[must_use]
    pub const fn db(&self) -> &Database {
        &self.db
    }

    #[must_use]
    pub fn metrics_handle(&self) -> Arc<Metrics> {
        Arc::clone(&self.metrics)
    }

    fn thresholds(&self) -> Thresholds {
        Thresholds {
            min_liquidity_usd: Decimal::from(self.cfg.min_pool_liquidity_usd),
            min_volume_24h_usd: Decimal::from(self.cfg.min_volume_24h_usd),
            min_txns_1h: self.cfg.min_txns_1h,
            max_observation_age: self.cfg.max_market_observation_age,
            resample_after: self.cfg.market_discovery_interval.saturating_mul(6),
        }
    }

    fn shares(&self) -> [u32; 5] {
        [
            self.cfg.share_established_bps,
            self.cfg.share_emerging_bps,
            self.cfg.share_local_activity_bps,
            self.cfg.share_historical_bps,
            self.cfg.share_exploration_bps,
        ]
    }

    async fn admission_now(&self) -> Result<Admission, RadarError> {
        let outstanding = u64::try_from(self.db.radar_open_estimates().await?).unwrap_or(0);
        Ok(self.admission(outstanding).1)
    }

    // ---- discovery cycle ---------------------------------------------------------------------

    /// Run one discovery cycle. `trigger`: `SCHEDULED`, `OPERATOR` or `FIXTURE`.
    pub async fn discovery_cycle(&self, trigger: &str) -> Result<CycleReport, RadarError> {
        let started = now();
        let adm = self.admission_now().await?;
        let budget_state = self
            .feed
            .budget()
            .map_or_else(|| "UNKNOWN".to_owned(), |b| b.state);
        let seed_bytes = Uuid::new_v4();
        let mut seed8 = [0u8; 8];
        seed8.copy_from_slice(&seed_bytes.as_bytes()[..8]);
        let seed = i64::from_le_bytes(seed8);
        let cycle_id = Uuid::new_v4();
        self.db
            .radar_start_cycle(&NewCycle {
                cycle_id,
                trigger: trigger.to_owned(),
                seed,
                budget_state: budget_state.clone(),
            })
            .await?;
        inc(&self.metrics.cycles);
        self.metrics
            .touch(|l| l.cycle_at = Some(UtcTimestamp::now()));
        let mut rep = CycleReport {
            cycle_id: Some(cycle_id),
            ..CycleReport::default()
        };
        if let Some(reason) = adm.reason() {
            inc(&self.metrics.cycles_deferred);
            rep.status = "DEFERRED".into();
            rep.deferred_reason = Some(reason.to_owned());
            self.db
                .radar_finish_cycle(
                    cycle_id,
                    &CycleFinish {
                        status: rep.status.clone(),
                        deferred_reason: rep.deferred_reason.clone(),
                        ..CycleFinish::default()
                    },
                )
                .await?;
            return Ok(rep);
        }
        let origin = if trigger == "SCHEDULED" {
            "AUTONOMOUS"
        } else {
            "OPERATOR"
        };
        let remote = adm.mode == Mode::Full;
        let mut partial = false;
        let mut observations: Vec<ObservationWrite> = Vec::new();
        let mut universe = self.db.radar_market_count().await?;
        let ceiling = i64::from(self.cfg.max_market_targets);
        let mut listed: Vec<String> = Vec::new();
        let mut evidence: HashMap<String, ListedEvidence> = HashMap::new();

        // 1. Provider listings (organic first; promotion is exploration-only).
        let listings = [
            Listing::JupiterTopTraded,
            Listing::JupiterTopOrganic,
            Listing::JupiterRecent,
            Listing::DexBoosts,
        ];
        for l in listings {
            let run_start = UtcTimestamp::now();
            if !remote {
                self.record_run(
                    cycle_id,
                    l.source(),
                    "BUDGET_DEFERRED",
                    Some("BUDGET_CONSERVATION"),
                    0,
                    0,
                    0,
                    &run_start,
                )
                .await?;
                continue;
            }
            match self.feed.listing(l).await {
                Err(e) => {
                    if !matches!(e, crate::feed::FeedError::Disabled(_)) {
                        partial = true;
                        inc(&self.metrics.source_failures);
                    }
                    self.record_run(
                        cycle_id,
                        l.source(),
                        e.run_status(),
                        Some(e.code()),
                        0,
                        0,
                        1,
                        &run_start,
                    )
                    .await?;
                }
                Ok(f) => {
                    self.metrics
                        .provider_requests
                        .fetch_add(u64::from(f.requests), Ordering::Relaxed);
                    let mut new = 0;
                    let observed = odt(&f.observed_at);
                    for t in &f.value {
                        if is_quote_like(&t.mint) {
                            continue;
                        }
                        let age_days = t
                            .first_pool_created_at
                            .as_ref()
                            .or(t.created_at.as_ref())
                            .map(|c| (observed - odt(c)).whole_days());
                        let young = t
                            .first_pool_created_at
                            .as_ref()
                            .is_some_and(|c| observed - odt(c) < time::Duration::hours(24));
                        let bucket = match l {
                            Listing::JupiterRecent => Bucket::Emerging,
                            Listing::DexBoosts => Bucket::Exploration,
                            _ if young => Bucket::Emerging,
                            _ => Bucket::Established,
                        };
                        let c = classify::classify(&TokenEvidence {
                            mint: &t.mint,
                            tags: &t.tags,
                            launchpad: t.launchpad.as_deref(),
                            verified: t.verified,
                            age_days,
                            pool_protocols: &[],
                        });
                        let known = self.db.radar_market(&t.mint).await?.is_some();
                        if !known && universe >= ceiling {
                            observations.push(obs(
                                cycle_id,
                                &t.mint,
                                l.source(),
                                l.promotional(),
                                t.rank,
                                "OBSERVED",
                                "UNIVERSE_FULL",
                                &f.observed_at,
                            ));
                            continue;
                        }
                        let inserted = self
                            .db
                            .radar_upsert_market(&MarketUpsert {
                                mint: t.mint.clone(),
                                token_program: t.token_program.clone(),
                                display_symbol: t
                                    .symbol
                                    .as_ref()
                                    .map(|s| s.chars().take(32).collect()),
                                display_name: t.name.as_ref().map(|s| s.chars().take(64).collect()),
                                classification: c.label.to_owned(),
                                classification_reasons: c.reasons.clone(),
                                ruleset: crate::model::CLASSIFICATION_RULESET.to_owned(),
                                bucket: bucket.as_str().to_owned(),
                                source: l.source().to_owned(),
                                promotional: l.promotional(),
                                observed_at: f.observed_at,
                            })
                            .await?;
                        if inserted {
                            new += 1;
                            universe += 1;
                        }
                        evidence.insert(
                            t.mint.clone(),
                            (t.tags.clone(), t.launchpad.clone(), t.verified, age_days),
                        );
                        listed.push(t.mint.clone());
                        let mut o = obs(
                            cycle_id,
                            &t.mint,
                            l.source(),
                            l.promotional(),
                            t.rank,
                            "OBSERVED",
                            "LISTED",
                            &f.observed_at,
                        );
                        o.organic_score = dec_str(t.organic_score);
                        o.txns_1h = u32_of(t.txns_1h);
                        observations.push(o);
                    }
                    rep.markets_new += new;
                    let status = if f.value.is_empty() { "EMPTY" } else { "OK" };
                    self.record_run(
                        cycle_id,
                        l.source(),
                        status,
                        None,
                        i32_of(f.value.len()),
                        new,
                        i32_of(f.requests),
                        &run_start,
                    )
                    .await?;
                }
            }
        }

        // 2. Local activity: mints the DATA ENGINE already decoded (no provider).
        let run_start = UtcTimestamp::now();
        let since = ts(started - time::Duration::hours(24));
        let local = self.db.radar_local_activity_mints(&since, 200).await?;
        let mut local_counts: HashMap<String, u32> = HashMap::new();
        let mut local_new = 0;
        for (mint, n) in &local {
            if is_quote_like(mint) || is_known_program(mint) {
                continue;
            }
            let pools = self.db.radar_local_pools_for_mint(mint, 5).await?;
            let protocols: Vec<String> = pools.iter().map(|(_, p, _)| p.clone()).collect();
            let c = classify::classify(&TokenEvidence {
                mint,
                pool_protocols: &protocols,
                ..TokenEvidence::default()
            });
            let known = self.db.radar_market(mint).await?.is_some();
            if !known && universe >= ceiling {
                continue;
            }
            if self
                .db
                .radar_upsert_market(&MarketUpsert {
                    mint: mint.clone(),
                    token_program: None,
                    display_symbol: None,
                    display_name: None,
                    classification: c.label.to_owned(),
                    classification_reasons: c.reasons,
                    ruleset: crate::model::CLASSIFICATION_RULESET.to_owned(),
                    bucket: Bucket::LocalActivity.as_str().to_owned(),
                    source: source::LOCAL_ACTIVITY.to_owned(),
                    promotional: false,
                    observed_at: ts(started),
                })
                .await?
            {
                local_new += 1;
                universe += 1;
            }
            local_counts.insert(mint.clone(), u32::try_from(*n).unwrap_or(u32::MAX));
            for (pool, protocol, _) in &pools {
                // A pool the DATA ENGINE decoded swaps for is a supported
                // venue by evidence; no account lookup is needed.
                self.db
                    .radar_upsert_pool(&PoolUpsert {
                        pool: pool.clone(),
                        mint: mint.clone(),
                        quote_mint: None,
                        dex_id: None,
                        labels: Vec::new(),
                        liquidity_usd: None,
                        volume_24h_usd: None,
                        txns_1h: None,
                        txns_24h: None,
                        pair_created_at: None,
                        observed_at: ts(started),
                    })
                    .await?;
                if let Some(program) = program_of_protocol(protocol) {
                    self.db
                        .radar_set_pool_validation(pool, "SUPPORTED", Some(program), Some(protocol))
                        .await?;
                }
            }
            observations.push(obs(
                cycle_id,
                mint,
                source::LOCAL_ACTIVITY,
                false,
                0,
                "OBSERVED",
                "LOCAL_VERIFIED_SWAPS",
                &ts(started),
            ));
        }
        rep.markets_new += local_new;
        let local_status = if local.is_empty() { "EMPTY" } else { "OK" };
        self.record_run(
            cycle_id,
            source::LOCAL_ACTIVITY,
            local_status,
            None,
            i32_of(local.len()),
            local_new,
            0,
            &run_start,
        )
        .await?;

        // 3. Pools of listed markets (market data; rate-limited, no credits).
        let mut wanted: Vec<String> = listed.clone();
        wanted.sort();
        wanted.dedup();
        wanted.truncate(usize::try_from(self.cfg.max_active_market_targets).unwrap_or(0));
        if remote && !wanted.is_empty() {
            match self.feed.pools(&wanted).await {
                Ok(f) => {
                    self.metrics
                        .provider_requests
                        .fetch_add(u64::from(f.requests), Ordering::Relaxed);
                    let set: HashSet<&str> = wanted.iter().map(String::as_str).collect();
                    for p in &f.value {
                        let mint =
                            if set.contains(p.base_mint.as_str()) && !is_quote_like(&p.base_mint) {
                                p.base_mint.clone()
                            } else if let Some(q) = p
                                .quote_mint
                                .as_ref()
                                .filter(|q| set.contains(q.as_str()) && !is_quote_like(q))
                            {
                                q.clone()
                            } else {
                                continue;
                            };
                        let quote = if mint == p.base_mint {
                            p.quote_mint.clone()
                        } else {
                            Some(p.base_mint.clone())
                        };
                        self.db
                            .radar_upsert_pool(&PoolUpsert {
                                pool: p.pool.clone(),
                                mint: mint.clone(),
                                quote_mint: quote,
                                dex_id: p.dex_id.clone().filter(|d| {
                                    d.len() <= 48
                                        && d.chars().all(|c| {
                                            c.is_ascii_lowercase()
                                                || c.is_ascii_digit()
                                                || "_.-".contains(c)
                                        })
                                }),
                                labels: p.labels.iter().take(8).cloned().collect(),
                                liquidity_usd: dec_str(p.liquidity_usd),
                                volume_24h_usd: dec_str(p.volume_24h_usd),
                                txns_1h: u32_of(p.txns_1h),
                                txns_24h: u32_of(p.txns_24h),
                                pair_created_at: p.pair_created_at,
                                observed_at: f.observed_at,
                            })
                            .await?;
                        let mut o = obs(
                            cycle_id,
                            &mint,
                            "DEXSCREENER_PAIRS",
                            false,
                            0,
                            "OBSERVED",
                            "POOL_OBSERVED",
                            &f.observed_at,
                        );
                        o.pool = Some(p.pool.clone());
                        o.liquidity_usd = dec_str(p.liquidity_usd);
                        o.volume_24h_usd = dec_str(p.volume_24h_usd);
                        o.txns_1h = u32_of(p.txns_1h);
                        o.txns_24h = u32_of(p.txns_24h);
                        o.pair_created_at = p.pair_created_at;
                        observations.push(o);
                    }
                }
                Err(e) => {
                    partial = true;
                    inc(&self.metrics.source_failures);
                    tracing::warn!(target: "solprint::radar", code = e.code(), "pool discovery failed");
                }
            }
        }

        // 4. Validate pool program owners (getMultipleAccounts, budgeted).
        let pools = self.db.radar_pools_for_mints(&wanted).await?;
        let unverified: Vec<String> = pools
            .iter()
            .filter(|p| p.validation == "UNVERIFIED")
            .map(|p| p.pool.clone())
            .collect();
        let validation_estimate =
            u64::try_from(unverified.len().div_ceil(crate::feed::ACCOUNTS_CHUNK))
                .unwrap_or(u64::MAX);
        if remote && !unverified.is_empty() && adm.admits(validation_estimate) {
            match self.feed.owners(&unverified).await {
                Ok(f) => {
                    self.metrics
                        .provider_requests
                        .fetch_add(u64::from(f.requests), Ordering::Relaxed);
                    rep.credits_estimated += i32_of(f.credits);
                    for (pool, owner) in unverified.iter().zip(f.value) {
                        let (state, proto) = match owner.as_deref() {
                            None => ("ACCOUNT_NOT_FOUND", None),
                            Some(o) => pool_protocol(o)
                                .map_or(("UNSUPPORTED_PROGRAM", None), |p| ("SUPPORTED", Some(p))),
                        };
                        self.db
                            .radar_set_pool_validation(pool, state, owner.as_deref(), proto)
                            .await?;
                    }
                }
                Err(e) => {
                    partial = true;
                    inc(&self.metrics.source_failures);
                    tracing::warn!(target: "solprint::radar", code = e.code(), "pool validation failed");
                }
            }
        }
        // Re-classify listed markets with their validated pool programs.
        let pools = self.db.radar_pools_for_mints(&wanted).await?;
        for mint in &wanted {
            let protocols: Vec<String> = pools
                .iter()
                .filter(|p| &p.mint == mint && p.validation == "SUPPORTED")
                .filter_map(|p| p.protocol.clone())
                .collect();
            let Some((tags, launchpad, verified, age_days)) = evidence.get(mint) else {
                continue;
            };
            let c = classify::classify(&TokenEvidence {
                mint,
                tags,
                launchpad: launchpad.as_deref(),
                verified: *verified,
                age_days: *age_days,
                pool_protocols: &protocols,
            });
            if let Some(m) = self.db.radar_market(mint).await?
                && m.classification != c.label
            {
                // `promotional: true` keeps the stored bucket and source
                // flags unchanged; only the classification is refreshed.
                self.db
                    .radar_upsert_market(&MarketUpsert {
                        mint: mint.clone(),
                        token_program: None,
                        display_symbol: None,
                        display_name: None,
                        classification: c.label.to_owned(),
                        classification_reasons: c.reasons,
                        ruleset: crate::model::CLASSIFICATION_RULESET.to_owned(),
                        bucket: m.bucket.clone(),
                        source: m
                            .sources
                            .first()
                            .cloned()
                            .unwrap_or_else(|| source::LOCAL_ACTIVITY.to_owned()),
                        promotional: true,
                        observed_at: ts(now()),
                    })
                    .await?;
            }
        }

        // 5. Multi-bucket selection and sampling jobs.
        let markets = self.db.radar_markets_for_selection(ceiling).await?;
        let mints: Vec<String> = markets.iter().map(|m| m.mint.clone()).collect();
        let all_pools = self.db.radar_pools_for_mints(&mints).await?;
        let mut cands: Vec<MarketCandidate> = Vec::with_capacity(markets.len());
        for m in &markets {
            let best = all_pools
                .iter()
                .filter(|p| p.mint == m.mint && p.validation == "SUPPORTED")
                .max_by(|a, b| {
                    let la = a
                        .liquidity_usd
                        .as_deref()
                        .and_then(|s| s.parse::<Decimal>().ok());
                    let lb = b
                        .liquidity_usd
                        .as_deref()
                        .and_then(|s| s.parse::<Decimal>().ok());
                    la.cmp(&lb).then(b.pool.cmp(&a.pool))
                });
            let bucket = Bucket::parse(&m.bucket).unwrap_or(Bucket::Exploration);
            if adm.mode == Mode::Conservation && bucket != Bucket::LocalActivity {
                continue;
            }
            let probe = classify::Classification {
                label: classify_label(&m.classification),
                reasons: Vec::new(),
            };
            cands.push(MarketCandidate {
                mint: m.mint.clone(),
                bucket,
                excluded_class: probe
                    .excluded_from_sampling()
                    .then(|| m.classification.clone()),
                promotional_only: m.promotional_only,
                pool: best.map(|p| PoolView {
                    pool: p.pool.clone(),
                    supported: true,
                    liquidity_usd: p.liquidity_usd.as_deref().and_then(|s| s.parse().ok()),
                    volume_24h_usd: p.volume_24h_usd.as_deref().and_then(|s| s.parse().ok()),
                    txns_1h: p.txns_1h.and_then(|v| u32::try_from(v).ok()),
                    observed_at: odt(&p.last_observed_at),
                    last_sampled_at: p.last_sampled_at.as_ref().map(odt),
                }),
                samples_completed: u32::try_from(m.samples_completed).unwrap_or(0),
                wallets_found: u32::try_from(m.wallets_found).unwrap_or(0),
                local_swaps: local_counts.get(&m.mint).copied().unwrap_or(0),
            });
        }
        let counts = self.db.radar_job_counts().await?;
        let open_samples = counts
            .open_by_class
            .iter()
            .find(|(c, _)| c == work::TRANSACTION_SAMPLING)
            .map_or(0, |(_, n)| *n);
        let room = i64::from(self.cfg.max_pending_jobs)
            .saturating_sub(counts.open_total)
            .max(0);
        let slots = u32::try_from(
            i64::from(self.cfg.max_market_samples_per_cycle)
                .min(room)
                .min(
                    i64::from(self.cfg.max_active_market_targets)
                        .saturating_sub(open_samples)
                        .max(0),
                ),
        )
        .unwrap_or(0);
        let shares = if adm.mode == Mode::Conservation {
            [0, 0, 10_000, 0, 0]
        } else {
            self.shares()
        };
        let sel = market::select(&cands, slots, shares, &self.thresholds(), seed, now());
        for d in &sel.decisions {
            self.db
                .radar_set_market_selection(&MarketSelection {
                    mint: d.mint.clone(),
                    selection_state: d.state.to_owned(),
                    selection_reasons: d.reasons.clone(),
                    activity_state: if d.state == "SELECTED" {
                        "ACTIVE".into()
                    } else {
                        "UNKNOWN".into()
                    },
                    market_priority: dec_str(d.priority),
                    coverage_state: None,
                })
                .await?;
        }
        let mut spent: u64 = 0;
        let mut exploration_picks = 0;
        let mut historical_picks = 0;
        for p in &sel.picks {
            let m = markets.iter().find(|m| m.mint == p.mint);
            let pool = all_pools.iter().find(|x| x.pool == p.pool);
            let limit = if m.is_some_and(|m| m.wallets_found > 0 && m.samples_completed > 0) {
                self.cfg.expanded_sample_signatures
            } else {
                self.cfg.initial_sample_signatures
            };
            let estimate = 1 + u64::from(limit);
            if !adm.admits(spent + estimate) {
                observations.push(obs(
                    cycle_id,
                    &p.mint,
                    p.bucket.as_str(),
                    false,
                    0,
                    "DEFERRED",
                    "BUDGET_ALLOWANCE",
                    &ts(now()),
                ));
                continue;
            }
            let priority = 400
                + (p.priority * Decimal::from(500))
                    .trunc()
                    .to_i32()
                    .unwrap_or(0)
                    .clamp(0, 500);
            let created = self
                .db
                .radar_create_job(&NewJob {
                    job_id: Uuid::new_v4(),
                    work_class: work::TRANSACTION_SAMPLING.into(),
                    subject_kind: "POOL".into(),
                    subject: p.pool.clone(),
                    origin: origin.into(),
                    bucket: Some(p.bucket.as_str().into()),
                    cycle_id: Some(cycle_id),
                    priority,
                    signature_limit: i32_of(limit),
                    lookback_days: None,
                    start_before_signature: None,
                    until_signature: pool.and_then(|x| x.sample_until_signature.clone()),
                    min_slot: None,
                    commitment: "confirmed".into(),
                    estimated_credits: i32_of(estimate),
                    next_run_at: None,
                })
                .await?;
            if created.is_some() {
                spent += estimate;
                rep.samples_scheduled += 1;
                rep.markets_selected += 1;
                match p.bucket {
                    Bucket::Exploration => exploration_picks += 1,
                    Bucket::Historical => historical_picks += 1,
                    _ => {}
                }
                let mut o = obs(
                    cycle_id,
                    &p.mint,
                    p.bucket.as_str(),
                    false,
                    0,
                    "SELECTED",
                    &format!("BUCKET_{}", p.bucket.as_str()),
                    &ts(now()),
                );
                o.pool = Some(p.pool.clone());
                observations.push(o);
            }
        }
        for (src, n) in [
            (source::HISTORICAL, historical_picks),
            (source::EXPLORATION, exploration_picks),
        ] {
            let status = if n > 0 { "OK" } else { "EMPTY" };
            self.record_run(cycle_id, src, status, None, n, 0, 0, &ts(started))
                .await?;
        }
        rep.credits_estimated += i32_of(spent);
        rep.observations = i32_of(observations.len());
        self.db.radar_insert_observations(&observations).await?;
        rep.status = if partial {
            "PARTIAL".into()
        } else {
            "COMPLETED".into()
        };
        self.db
            .radar_finish_cycle(
                cycle_id,
                &CycleFinish {
                    status: rep.status.clone(),
                    deferred_reason: None,
                    observations: rep.observations,
                    markets_new: rep.markets_new,
                    markets_selected: rep.markets_selected,
                    samples_scheduled: rep.samples_scheduled,
                    credits_estimated: rep.credits_estimated,
                },
            )
            .await?;
        tracing::info!(target: "solprint::radar", status = %rep.status, new = rep.markets_new, scheduled = rep.samples_scheduled, "discovery cycle");
        Ok(rep)
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_run(
        &self,
        cycle_id: Uuid,
        src: &str,
        status: &str,
        error: Option<&str>,
        items: i32,
        new: i32,
        requests: i32,
        started: &UtcTimestamp,
    ) -> Result<(), RadarError> {
        self.db
            .radar_record_source_run(&SourceRunWrite {
                run_id: Uuid::new_v4(),
                cycle_id,
                source: src.to_owned(),
                status: status.to_owned(),
                error_code: error.map(str::to_owned),
                items,
                markets_new: new,
                provider_requests: requests,
                credits_estimated: 0,
                started_at: *started,
                finished_at: UtcTimestamp::now(),
            })
            .await?;
        Ok(())
    }

    // ---- jobs --------------------------------------------------------------------------------

    /// Start runnable jobs (as DATA ENGINE backfills) and close finished ones.
    pub async fn jobs_tick(&self) -> Result<usize, RadarError> {
        self.db.radar_recover_leases().await?;
        let mut progressed = self.close_waiting_jobs().await?;
        let control = self.db.radar_control().await?;
        if control.paused {
            return Ok(progressed);
        }
        let mut adm = self.admission_now().await?;
        let sampling = vec![work::TRANSACTION_SAMPLING.to_owned()];
        let history: Vec<String> = work::HISTORY.iter().map(|s| (*s).to_owned()).collect();
        let mut claimed = self
            .db
            .radar_claim_jobs(
                self.owner,
                &sampling,
                i64::from(self.cfg.max_concurrent_sample_jobs),
                LEASE,
                8,
            )
            .await?;
        claimed.extend(
            self.db
                .radar_claim_jobs(
                    self.owner,
                    &history,
                    i64::from(self.cfg.max_concurrent_history_jobs),
                    LEASE,
                    8,
                )
                .await?,
        );
        for j in claimed {
            if j.cancel_requested {
                self.db
                    .radar_finish_job(j.job_id, "CANCELLED", "CANCELLED_BY_OPERATOR", 0, 0)
                    .await?;
                continue;
            }
            let defer = if !self.de.enabled || !self.de.backfill_enabled {
                Some(("DATA_ENGINE_BACKFILL_DISABLED", DEFER_LONG))
            } else if !self.cfg.auto_discovery_enabled && j.origin != "OPERATOR" {
                Some(("AUTO_DISCOVERY_DISABLED", DEFER_LONG))
            } else if let Some(r) = adm.reason() {
                Some((r, DEFER_SHORT))
            } else if adm.mode == Mode::Conservation
                && !(j.work_class == work::INITIAL_HISTORY
                    || (j.work_class == work::TRANSACTION_SAMPLING
                        && j.bucket.as_deref() == Some(Bucket::LocalActivity.as_str())))
            {
                Some(("BUDGET_CONSERVATION", DEFER_SHORT))
            } else if !adm.admits(u64::try_from(j.estimated_credits).unwrap_or(u64::MAX)) {
                Some(("BUDGET_ALLOWANCE", DEFER_SHORT))
            } else {
                None
            };
            if let Some((reason, retry)) = defer {
                inc(&self.metrics.jobs_deferred);
                self.db.radar_defer_job(j.job_id, reason, retry).await?;
                continue;
            }
            let limit = j.signature_limit.max(1);
            let page = i32::from(self.de.page_size).min(limit).max(1);
            let pages = (limit + page - 1) / page;
            let backfill_id = Uuid::new_v4();
            self.db
                .de_create_backfill(&NewBackfill {
                    backfill_id,
                    target_id: None,
                    address: j.subject.clone(),
                    commitment: j.commitment.clone(),
                    start_before_signature: j.start_before_signature.clone(),
                    until_signature: j.until_signature.clone(),
                    min_slot: j.min_slot,
                    page_size: page,
                    max_pages: pages.clamp(1, 100),
                    max_signatures: limit.min(10_000),
                    credits_estimated: j.estimated_credits,
                })
                .await?;
            let reused = if j.subject_kind == "WALLET" {
                i32_of(self.db.radar_local_signature_count(&j.subject).await?)
            } else {
                0
            };
            if self
                .db
                .radar_job_waiting(j.job_id, self.owner, backfill_id, reused)
                .await?
            {
                inc(&self.metrics.jobs_started);
                progressed += 1;
                adm.available = adm
                    .available
                    .saturating_sub(u64::try_from(j.estimated_credits).unwrap_or(0));
            }
        }
        Ok(progressed)
    }

    async fn close_waiting_jobs(&self) -> Result<usize, RadarError> {
        let mut closed = 0;
        for j in self.db.radar_waiting_jobs(100).await? {
            let Some(bf) = j.backfill_id else {
                self.db
                    .radar_finish_job(j.job_id, "FAILED", "BACKFILL_MISSING", 0, 0)
                    .await?;
                continue;
            };
            let Some(p) = self.db.radar_backfill_progress(bf).await? else {
                self.db
                    .radar_finish_job(j.job_id, "FAILED", "BACKFILL_MISSING", 0, 0)
                    .await?;
                continue;
            };
            let terminal = matches!(
                p.status.as_str(),
                "COMPLETED" | "COMPLETED_WITH_GAPS" | "FAILED" | "CANCELLED"
            );
            if !terminal {
                continue;
            }
            let waited = now() - odt(&j.updated_at);
            if p.transactions_pending > 0 && waited < INGEST_GRACE {
                continue;
            }
            let credits = p.credits_spent.saturating_add(p.queued);
            let code = job_result(&p, &j);
            let state = if p.status == "FAILED" {
                "FAILED"
            } else if p.status == "CANCELLED" {
                "CANCELLED"
            } else {
                "COMPLETED"
            };
            if !self
                .db
                .radar_finish_job(j.job_id, state, code, credits, p.discovered)
                .await?
            {
                continue;
            }
            inc(&self.metrics.jobs_completed);
            closed += 1;
            if j.work_class == work::TRANSACTION_SAMPLING {
                if let Some(pool) = self.db.radar_pool(&j.subject).await? {
                    let found = self.db.radar_backfill_wallet_yield(bf).await?;
                    self.db
                        .radar_mark_market_sampled(
                            &pool.mint,
                            &pool.pool,
                            p.newest_signature.as_deref(),
                            i32_of(found),
                        )
                        .await?;
                }
            } else {
                self.db.radar_mark_dirty(&j.subject).await?;
            }
        }
        Ok(closed)
    }

    // ---- DATA ENGINE events ------------------------------------------------------------------

    /// Consume DATA ENGINE outbox events for the `radar` consumer.
    pub async fn events_tick(&self) -> Result<usize, RadarError> {
        let events = self.db.radar_pending_events(EVENT_BATCH).await?;
        let n = events.len();
        for e in events {
            let payload: serde_json::Value =
                serde_json::from_str(&e.payload_json).unwrap_or_default();
            let text = |k: &str| payload.get(k).and_then(|v| v.as_str()).map(str::to_owned);
            let r: Result<(), RadarError> = async {
                match e.event_type.as_str() {
                    t if t == de_events::SWAP_RECORDED => {
                        let Some(id) = text("swapId") else {
                            self.db.radar_ack_event(e.event_id).await?;
                            return Ok(());
                        };
                        match self.db.radar_swap(&id).await? {
                            Some(s) => self.apply_swap(Some(e.event_id), &s).await,
                            None => Ok(self.db.radar_ack_event(e.event_id).await?),
                        }
                    }
                    t if t == de_events::SWAP_RETRACTED => {
                        let id = text("swapId").unwrap_or_default();
                        let reason = text("reason")
                            .filter(|r| r.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
                            .unwrap_or_else(|| "UNSPECIFIED".into());
                        let w = self
                            .db
                            .radar_invalidate_swap(
                                Some(e.event_id),
                                &id,
                                &format!("RETRACTED_{reason}"),
                            )
                            .await?;
                        self.metrics
                            .evidence_invalidated
                            .fetch_add(u64::try_from(w.len()).unwrap_or(0), Ordering::Relaxed);
                        Ok(())
                    }
                    t if t == de_events::TRANSACTION_FINALIZED => {
                        let sig = text("signature").unwrap_or_default();
                        self.db
                            .radar_finalize_signature(Some(e.event_id), &sig)
                            .await?;
                        Ok(())
                    }
                    _ => Ok(self.db.radar_ack_event(e.event_id).await?),
                }
            }
            .await;
            match r {
                Ok(()) => inc(&self.metrics.events_processed),
                Err(err) => {
                    inc(&self.metrics.event_failures);
                    self.metrics.error("EVENT_FAILED");
                    tracing::warn!(target: "solprint::radar", error = %err, event = %e.event_type, "event processing failed");
                    self.db
                        .de_outbox_mark(
                            st::RADAR_CONSUMER,
                            e.event_id,
                            false,
                            Some("RADAR_EVENT_FAILED"),
                            Duration::from_secs(30),
                        )
                        .await?;
                }
            }
        }
        if n > 0 {
            self.metrics
                .touch(|l| l.event_at = Some(UtcTimestamp::now()));
        }
        Ok(n)
    }

    /// Record evidence from one DATA ENGINE swap (current stored state; a
    /// corrected swap is re-read, so revisions and retractions are honored).
    pub async fn apply_swap(
        &self,
        event_id: Option<Uuid>,
        s: &SwapForRadar,
    ) -> Result<(), RadarError> {
        let Some(wallet) = s.wallet.clone() else {
            // No verified owner: the fee payer is never a substitute.
            if let Some(id) = event_id {
                self.db.radar_ack_event(id).await?;
            }
            return Ok(());
        };
        let trade = trade_of(s, &wallet);
        let (side, _, token) = screening::side(&trade);
        if side == Side::QuoteOnly {
            if let Some(id) = event_id {
                self.db.radar_ack_event(id).await?;
            }
            return Ok(());
        }
        let (role, token_mint, quote_mint) = match side {
            Side::Buy => (
                "BUYER",
                token.map(str::to_owned),
                quote_of(s.input_mint.as_deref(), &s.input_asset_kind),
            ),
            Side::Sell => (
                "SELLER",
                token.map(str::to_owned),
                quote_of(s.output_mint.as_deref(), &s.output_asset_kind),
            ),
            _ => ("SWAPPER", s.output_mint.clone(), None),
        };
        let verified_attr = matches!(
            s.attribution.as_str(),
            "VERIFIED_OWNER" | "VERIFIED_AUTHORITY"
        );
        let invalid = if !s.canonical {
            Some("SWAP_NOT_CANONICAL")
        } else if s.classification_status != "VERIFIED" {
            Some("SWAP_NOT_VERIFIED")
        } else if !verified_attr {
            Some("ATTRIBUTION_LOST")
        } else if s.finality == "ORPHANED" {
            Some("ORPHANED")
        } else {
            None
        };
        let origin = self.db.radar_signature_origin(&s.signature).await?;
        let discovery_source = match origin.work_class.as_deref() {
            Some(work::TRANSACTION_SAMPLING) => match origin.job_id {
                Some(id) => self
                    .db
                    .radar_job(id)
                    .await?
                    .and_then(|j| j.bucket)
                    .unwrap_or_else(|| source::EXPLORATION.to_owned()),
                None => source::DATA_ENGINE.to_owned(),
            },
            Some(work::ACTIVE_MONITORING) => source::MONITORING.to_owned(),
            Some(_) => source::WALLET_HISTORY.to_owned(),
            None if origin.source == "LIVE_STREAM" => source::MONITORING.to_owned(),
            None => source::DATA_ENGINE.to_owned(),
        };
        let infrastructure =
            is_known_program(&wallet) || self.db.radar_is_known_pool(&wallet).await?;
        let w = EvidenceWrite {
            address: wallet.clone(),
            swap_id: s.swap_id.clone(),
            signature: s.signature.clone(),
            slot: s.slot,
            block_time: s.block_time,
            token_mint,
            quote_mint,
            pool: s.pool.clone(),
            protocol: s.protocol.clone(),
            economic_role: role.to_owned(),
            attribution: s.attribution.clone(),
            finality: s.finality.clone(),
            discovery_source: discovery_source.clone(),
            job_id: origin.job_id,
            backfill_id: origin.backfill_id,
            swap_revision: s.revision.max(1),
            valid: invalid.is_none(),
            invalidation_reason: invalid.map(str::to_owned),
            evidence_available_at: s.first_observed_at,
        };
        let new_id = Uuid::new_v4();
        let discovered = ev::wallet_event(
            ev::WALLET_CANDIDATE_DISCOVERED,
            &wallet,
            1,
            &discovery_source,
            event_id,
            json!({"walletId": new_id, "discoverySource": discovery_source, "swapId": s.swap_id, "signature": s.signature}),
        );
        let new_wallet = (!infrastructure).then_some((new_id, &discovered));
        match self
            .db
            .radar_apply_evidence(event_id, &w, new_wallet)
            .await?
        {
            EvidenceOutcome::NewWallet => {
                inc(&self.metrics.wallets_discovered);
                inc(&self.metrics.evidence_recorded);
            }
            EvidenceOutcome::NewEvidence => inc(&self.metrics.evidence_recorded),
            EvidenceOutcome::Updated if invalid.is_some() => {
                inc(&self.metrics.evidence_invalidated)
            }
            EvidenceOutcome::Updated | EvidenceOutcome::Unchanged => {}
        }
        Ok(())
    }

    // ---- evaluation --------------------------------------------------------------------------

    /// Re-evaluate dirty and due wallets (bounded batch).
    pub async fn evaluate_tick(&self) -> Result<usize, RadarError> {
        let due = self
            .db
            .radar_wallets_to_evaluate(self.cfg.candidate_reevaluation_interval, EVAL_BATCH)
            .await?;
        let mut n = 0;
        for w in &due {
            match self.evaluate_wallet(w).await {
                Ok(true) => n += 1,
                Ok(false) => inc(&self.metrics.evaluation_conflicts),
                Err(e) => {
                    self.metrics.error("EVALUATION_FAILED");
                    tracing::warn!(target: "solprint::radar", error = %e, "wallet evaluation failed");
                }
            }
        }
        if n > 0 {
            self.metrics
                .touch(|l| l.evaluation_at = Some(UtcTimestamp::now()));
        }
        Ok(n)
    }

    /// Evaluate one wallet: eligibility, screening, priority, qualification
    /// and lifecycle, written atomically. `Ok(false)`: concurrent change.
    #[allow(clippy::too_many_lines)]
    pub async fn evaluate_wallet(&self, w: &WalletRow) -> Result<bool, RadarError> {
        let at = now();
        let addr = w.address.as_str();
        let swaps = self.db.radar_wallet_swaps(addr, SWAP_LIMIT).await?;
        let sigs: Vec<String> = swaps
            .iter()
            .map(|s| s.signature.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut pre: PreBalances = HashMap::new();
        for b in self.db.radar_wallet_token_balances(addr, &sigs).await? {
            pre.insert(
                (b.signature, b.mint),
                b.pre.and_then(|v| v.parse::<u64>().ok()),
            );
        }
        let trades: Vec<Trade> = swaps.iter().map(|s| trade_of(s, addr)).collect();
        let screened = screening::screen(&trades, &pre);
        let windows = screening::activity(&trades, at);
        let w30 = windows
            .iter()
            .find(|x| x.window == "30d")
            .cloned()
            .unwrap_or_default();
        let last_trade = trades.iter().filter_map(|t| t.block_time).max();
        let days_since = last_trade.map(|t| (at - t).whole_days().max(0));

        let evidence = self.db.radar_wallet_evidence(addr, SWAP_LIMIT).await?;
        let tokens: BTreeSet<&str> = evidence
            .iter()
            .filter(|e| e.valid)
            .filter_map(|e| e.token_mint.as_deref())
            .collect();
        let summary = EvidenceSummary {
            total: u32::try_from(evidence.len()).unwrap_or(u32::MAX),
            valid: count(evidence.iter().filter(|e| e.valid)),
            valid_finalized: count(
                evidence
                    .iter()
                    .filter(|e| e.valid && e.finality == "FINALIZED"),
            ),
            valid_quote_denominated: count(
                evidence
                    .iter()
                    .filter(|e| e.valid && e.economic_role != "SWAPPER"),
            ),
            invalid_attribution: count(
                evidence
                    .iter()
                    .filter(|e| e.invalidation_reason.as_deref() == Some("ATTRIBUTION_LOST")),
            ),
            distinct_tokens: u32::try_from(tokens.len()).unwrap_or(u32::MAX),
        };
        let new_evidence = evidence.iter().any(|e| {
            e.valid
                && w.last_evaluated_at
                    .as_ref()
                    .is_none_or(|t| e.discovered_at > *t)
        });
        let role = AccountRole {
            is_program: is_known_program(addr),
            is_known_pool: self.db.radar_is_known_pool(addr).await?,
            is_token_account: self.db.radar_is_foreign_token_account(addr).await?,
            operator_excluded: w.operator_excluded,
        };
        let (eligibility, eligibility_reasons) = scoring::eligibility(&role, &summary);

        let jobs = self
            .db
            .radar_jobs(&JobFilter {
                subject: Some(w.address.clone()),
                limit: 100,
                ..JobFilter::default()
            })
            .await?;
        let open = |j: &JobRow| {
            matches!(
                j.state.as_str(),
                "PENDING" | "RUNNING" | "WAITING_DATA" | "DEFERRED"
            )
        };
        let investigation_jobs: Vec<&JobRow> = jobs
            .iter()
            .filter(|j| {
                Stage::of_work_class(&j.work_class).is_some()
                    || j.work_class == work::DORMANT_RECHECK
            })
            .collect();
        let job_open = investigation_jobs.iter().any(|j| open(j));
        let job_running = investigation_jobs
            .iter()
            .filter(|j| open(j))
            .any(|j| matches!(j.state.as_str(), "RUNNING" | "WAITING_DATA"));
        let stage_done = jobs
            .iter()
            .filter(|j| j.state == "COMPLETED")
            .filter_map(|j| Stage::of_work_class(&j.work_class))
            .max()
            .max(Stage::parse_completed(&w.history_stage));
        let last_history = jobs
            .iter()
            .filter(|j| j.state != "CANCELLED" && j.completed_at.is_some())
            .filter(|j| work::HISTORY.contains(&j.work_class.as_str()))
            .max_by_key(|j| j.completed_at);
        let investigated_at = last_history
            .and_then(|j| j.completed_at)
            .max(w.last_investigated_at);
        let investigated = investigated_at != w.last_investigated_at;
        let coverage_state = if job_open {
            coverage::INVESTIGATION_PENDING.to_owned()
        } else {
            last_history
                .and_then(|j| j.result_code.clone())
                .filter(|c| coverage::ALL.contains(&c.as_str()))
                .unwrap_or_else(|| {
                    if stage_done.is_some() {
                        coverage::UNKNOWN.to_owned()
                    } else {
                        coverage::NOT_INVESTIGATED.to_owned()
                    }
                })
        };
        let retrieval = self.db.radar_wallet_retrieval(addr).await?;
        let cov = Coverage {
            signatures_discovered: u64::try_from(retrieval.discovered.max(0)).unwrap_or(0),
            transactions_retrieved: u64::try_from(retrieval.transactions_retrieved).unwrap_or(0),
            transactions_unavailable: u64::try_from(retrieval.transactions_unavailable)
                .unwrap_or(0),
            transactions_verified: u64::try_from(retrieval.transactions_verified).unwrap_or(0),
            transactions_unsupported: u64::try_from(retrieval.transactions_unsupported)
                .unwrap_or(0),
        };
        // Before any investigation, the evidence itself is the coverage.
        let cov = if cov.signatures_discovered == 0 {
            Coverage {
                signatures_discovered: u64::from(summary.total),
                transactions_retrieved: u64::from(summary.total),
                transactions_verified: u64::from(summary.valid),
                ..cov
            }
        } else {
            cov
        };
        let facts = ScreeningFacts::of(&screened);
        let primary = screened
            .primary()
            .map(|(d, s)| (d, s.outcomes, s.return_ratio()));
        let prio = scoring::priority(&PriorityInputs {
            swaps_30d: w30.swaps,
            days_since_last_trade: days_since,
            half_life_days: self.cfg.activity_half_life_days,
            coverage: cov.clone(),
            evidence: summary.clone(),
            screening_coverage_bps: screened.matched_coverage_bps(),
            primary,
        });
        let (qualification, qualification_reasons) = scoring::qualify(
            &QualificationPolicy {
                min_swaps: self.cfg.min_qualifying_swaps,
                min_trading_days: self.cfg.min_distinct_trading_days,
                min_outcomes: self.cfg.min_matched_outcomes,
                min_matched_coverage_bps: self.cfg.min_matched_coverage_bps,
                max_inactive_days: self.cfg.qualification_max_inactive_days,
            },
            &QualificationInputs {
                swaps_30d: w30.swaps,
                trading_days_30d: w30.trading_days,
                days_since_last_trade: days_since,
                stage_b_done: stage_done >= Some(Stage::B),
                stage_c_done: stage_done >= Some(Stage::C),
                screening: Some(facts.clone()),
            },
        );
        let investigated_after = match (investigated_at, last_trade) {
            (Some(i), Some(t)) => Some((odt(&i) - t).whole_days()),
            _ => None,
        };
        let mut decision = lifecycle::decide(&LifecycleInputs {
            current: &w.lifecycle,
            eligibility,
            qualification,
            stage_done,
            job_open,
            job_running,
            valid_swaps_in_history: u32::try_from(trades.len()).unwrap_or(u32::MAX),
            days_since_last_trade: days_since,
            investigated_days_after_last_trade: investigated_after,
            new_evidence,
            dormant_after_days: self.cfg.dormant_after_days,
            archive_after_days: self.cfg.archive_after_days,
            min_swaps_for_stage_b: 3,
        });
        let mut next_investigation_at = None;
        match decision.need {
            Need::Nothing => {}
            Need::History(stage) => {
                match self
                    .request_history(w, stage, prio.score, eligibility)
                    .await?
                {
                    Some(()) => {
                        if decision.lifecycle != lc::PRELIMINARY_QUALIFIED {
                            decision.lifecycle = lc::HISTORY_QUEUED;
                        }
                        decision
                            .reasons
                            .push(format!("STAGE_{}_QUEUED", stage.as_str()));
                    }
                    None => {
                        if decision.lifecycle == lc::DISCOVERED {
                            decision.lifecycle = lc::SCREENING;
                        }
                        decision
                            .reasons
                            .push("INVESTIGATION_CAPACITY_WAIT".to_owned());
                        next_investigation_at =
                            Some(ts(at + self.cfg.candidate_reevaluation_interval));
                    }
                }
            }
            Need::DormancyRecheck => {
                if self.request_recheck(w).await? {
                    decision.reasons.push("DORMANCY_RECHECK_QUEUED".to_owned());
                } else {
                    next_investigation_at = Some(ts(at + self.cfg.candidate_reevaluation_interval));
                }
            }
        }

        // Point-in-time inputs: the same inputs give the same hash.
        let mut h = Sha256::new();
        h.update(PRIORITY_POLICY.as_bytes());
        for s in &swaps {
            h.update(s.swap_id.as_bytes());
            h.update(s.revision.to_le_bytes());
            h.update(s.finality.as_bytes());
        }
        for e in &evidence {
            h.update(e.swap_id.as_bytes());
            h.update([u8::from(e.valid)]);
        }
        h.update(coverage_state.as_bytes());
        h.update(format!("{stage_done:?}{eligibility}{qualification}").as_bytes());
        let input_hash: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();

        let denominations: Vec<serde_json::Value> = screened
            .by_denomination
            .iter()
            .map(|(d, s)| {
                json!({
                    "denomination": d.as_str(),
                    "outcomes": s.outcomes,
                    "wins": s.wins,
                    "cost": s.cost.to_string(),
                    "proceeds": s.proceeds.to_string(),
                    "returnRatio": s.return_ratio().map(|r| r.to_string()),
                    "incompleteFees": s.incomplete_fees,
                })
            })
            .collect();
        let metrics = json!({
            "policy": PRIORITY_POLICY,
            "computedAt": ts(at).to_rfc3339(),
            "priority": {
                "score": prio.score.to_string(),
                "activity": prio.activity.to_string(),
                "recency": prio.recency.to_string(),
                "evidenceQuality": prio.evidence_quality.to_string(),
                "evidenceParts": prio.evidence_parts.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "performance": prio.performance.to_string(),
                "performanceStatus": prio.performance_status,
                "diversity": prio.diversity.to_string(),
            },
            "activity": windows.iter().map(|x| json!({
                "window": x.window, "swaps": x.swaps, "buys": x.buys, "sells": x.sells,
                "distinctTokens": x.distinct_tokens, "tradingDays": x.trading_days, "undated": x.undated,
            })).collect::<Vec<_>>(),
            "screening": {
                "outcomes": screened.outcomes.len(),
                "disposals": screened.disposals,
                "disposalsMatched": screened.disposals_matched,
                "matchedCoverageBps": screened.matched_coverage_bps(),
                "openPositions": screened.open_positions,
                "unevaluatedCycles": screened.unevaluated_cycles,
                "ambiguousTokens": screened.ambiguous_tokens.len(),
                "unknownCostTokens": screened.unknown_cost_tokens.len(),
                "byDenomination": denominations,
            },
            "coverage": {
                "state": coverage_state,
                "signaturesDiscovered": cov.signatures_discovered,
                "transactionsRetrieved": cov.transactions_retrieved,
                "transactionsUnavailable": cov.transactions_unavailable,
                "transactionsVerified": cov.transactions_verified,
                "transactionsUnsupported": cov.transactions_unsupported,
                "stageCompleted": stage_done.map(Stage::as_str),
                "swapsAnalysed": trades.len(),
                "swapLimit": SWAP_LIMIT,
            },
            "evidence": {
                "total": summary.total, "valid": summary.valid, "validFinalized": summary.valid_finalized,
                "distinctTokens": summary.distinct_tokens,
            },
        });
        let mut transitions = Vec::new();
        let mut events = Vec::new();
        let next_rev = w.revision + 1;
        if decision.lifecycle != w.lifecycle {
            transitions.push(TransitionWrite {
                from_state: Some(w.lifecycle.clone()),
                to_state: decision.lifecycle.to_owned(),
                trigger: decision.trigger.to_owned(),
                reasons: decision.reasons.clone(),
                metrics_json: json!({"priorityScore": prio.score.to_string(), "qualification": qualification, "eligibility": eligibility}).to_string(),
            });
            let event = if decision.trigger == "REACTIVATED" {
                ev::WALLET_REACTIVATED
            } else {
                ev::WALLET_LIFECYCLE_CHANGED
            };
            events.push(ev::wallet_event(
                event,
                addr,
                next_rev,
                "RADAR",
                None,
                json!({"from": w.lifecycle, "to": decision.lifecycle, "trigger": decision.trigger, "reasons": decision.reasons}),
            ));
        }
        if w.qualification.as_deref() != Some(qualification) {
            events.push(ev::wallet_event(
                ev::WALLET_QUALIFICATION_CHANGED,
                addr,
                next_rev,
                "RADAR",
                None,
                json!({"from": w.qualification, "to": qualification, "reasons": qualification_reasons}),
            ));
        }
        let old_score = w
            .priority_score
            .as_deref()
            .and_then(|s| s.parse::<Decimal>().ok());
        if old_score.is_none_or(|o| (o - prio.score).abs() >= Decimal::ONE) {
            events.push(ev::wallet_event(
                ev::WALLET_PRIORITY_CHANGED,
                addr,
                next_rev,
                "RADAR",
                None,
                json!({"from": old_score.map(|d| d.to_string()), "to": prio.score.to_string(), "policy": PRIORITY_POLICY}),
            ));
        }
        let saved = self
            .db
            .radar_save_evaluation(&EvaluationWrite {
                address: w.address.clone(),
                expected_revision: w.revision,
                lifecycle: decision.lifecycle.to_owned(),
                eligibility: eligibility.to_owned(),
                eligibility_reasons,
                qualification: Some(qualification.to_owned()),
                qualification_reasons,
                priority_score: prio.score.to_string(),
                policy_version: PRIORITY_POLICY.to_owned(),
                history_stage: stage_done.map_or("NONE", Stage::as_str).to_owned(),
                history_coverage_state: coverage_state,
                history_coverage_start: trades.iter().filter_map(|t| t.block_time).min().map(ts),
                history_coverage_end: last_trade.map(ts),
                last_verified_trade_at: last_trade.map(ts),
                next_investigation_at,
                valid_evidence_count: i32_of(summary.valid),
                verified_swap_count: i32_of(trades.len()),
                distinct_token_count: i32_of(summary.distinct_tokens),
                investigated,
                transitions,
                input_hash,
                metrics_json: metrics.to_string(),
                events,
            })
            .await?;
        if saved {
            inc(&self.metrics.evaluations);
        }
        let _ = LIFECYCLE_POLICY;
        Ok(saved)
    }

    async fn request_history(
        &self,
        w: &WalletRow,
        stage: Stage,
        score: Decimal,
        eligibility: &str,
    ) -> Result<Option<()>, RadarError> {
        if !self.cfg.auto_discovery_enabled && w.first_discovery_source != source::OPERATOR {
            return Ok(None);
        }
        let counts = self.db.radar_job_counts().await?;
        if counts.open_total >= i64::from(self.cfg.max_pending_jobs) {
            return Ok(None);
        }
        let investigating = self
            .db
            .radar_count_in(&[lc::HISTORY_QUEUED.to_owned(), lc::INVESTIGATING.to_owned()])
            .await?;
        let already = matches!(w.lifecycle.as_str(), lc::HISTORY_QUEUED | lc::INVESTIGATING);
        if !already && investigating >= i64::from(self.cfg.max_investigating_candidates) {
            return Ok(None);
        }
        let (days, sigs) = match stage {
            Stage::A => (
                self.cfg.history_stage_a_days,
                self.cfg.history_stage_a_signatures,
            ),
            Stage::B => (
                self.cfg.history_stage_b_days,
                self.cfg.history_stage_b_signatures,
            ),
            Stage::C => (
                self.cfg.history_stage_c_days,
                self.cfg.history_stage_c_signatures,
            ),
        };
        let start_before = if stage == Stage::A {
            None
        } else {
            self.db.radar_wallet_signature_bounds(&w.address).await?.1
        };
        let min_slot = self.min_slot_for(days).await?;
        let page = u32::from(self.de.page_size).max(1);
        let estimate = sigs.div_ceil(page) + sigs;
        let mut priority = score.trunc().to_i32().unwrap_or(0).clamp(0, 100) * 8;
        if eligibility == el::NEEDS_MORE_EVIDENCE {
            priority /= 2;
        }
        let origin = if w.first_discovery_source == source::OPERATOR {
            "OPERATOR"
        } else {
            "AUTONOMOUS"
        };
        self.db
            .radar_create_job(&NewJob {
                job_id: Uuid::new_v4(),
                work_class: stage.work_class().to_owned(),
                subject_kind: "WALLET".into(),
                subject: w.address.clone(),
                origin: origin.into(),
                bucket: None,
                cycle_id: None,
                priority: priority + 100,
                signature_limit: i32_of(sigs),
                lookback_days: Some(i32_of(days)),
                start_before_signature: start_before,
                until_signature: None,
                min_slot,
                commitment: "finalized".into(),
                estimated_credits: i32_of(estimate),
                next_run_at: None,
            })
            .await?;
        Ok(Some(()))
    }

    async fn request_recheck(&self, w: &WalletRow) -> Result<bool, RadarError> {
        if !self.cfg.auto_discovery_enabled && w.first_discovery_source != source::OPERATOR {
            return Ok(false);
        }
        let newest = self.db.radar_wallet_signature_bounds(&w.address).await?.0;
        Ok(self
            .db
            .radar_create_job(&NewJob {
                job_id: Uuid::new_v4(),
                work_class: work::DORMANT_RECHECK.into(),
                subject_kind: "WALLET".into(),
                subject: w.address.clone(),
                origin: "AUTONOMOUS".into(),
                bucket: None,
                cycle_id: None,
                priority: 50,
                signature_limit: DORMANCY_RECHECK_SIGNATURES,
                lookback_days: None,
                start_before_signature: None,
                until_signature: newest,
                min_slot: None,
                commitment: "finalized".into(),
                estimated_credits: 1 + DORMANCY_RECHECK_SIGNATURES,
                next_run_at: None,
            })
            .await?
            .is_some())
    }

    /// Lower slot bound for a lookback, from the newest local slot with a
    /// block time (only when that reference is recent). `None` when unknown.
    async fn min_slot_for(&self, days: u32) -> Result<Option<u64>, RadarError> {
        let Some((slot, bt)) = self.db.radar_reference_slot().await? else {
            return Ok(None);
        };
        let age = now() - odt(&bt);
        if age > time::Duration::days(1) || age < time::Duration::ZERO {
            return Ok(None);
        }
        Ok(Some(slot.saturating_sub(
            u64::from(days).saturating_mul(SLOTS_PER_DAY_ESTIMATE),
        )))
    }

    // ---- monitoring --------------------------------------------------------------------------

    /// Rotate monitoring assignments and schedule periodic refreshes.
    pub async fn monitoring_tick(&self) -> Result<usize, RadarError> {
        if !self.cfg.auto_discovery_enabled {
            return Ok(0);
        }
        let at = now();
        let live = self.live_monitoring_available();
        let adm = self.admission_now().await?;
        let capacity = if live {
            let foreign = usize::try_from(self.db.radar_foreign_enabled_targets().await?)
                .unwrap_or(usize::MAX);
            usize::try_from(self.cfg.max_live_monitored_wallets)
                .unwrap_or(0)
                .min(self.de.max_active_targets.saturating_sub(foreign))
        } else {
            let per = u64::try_from(1 + MONITORING_REFRESH_SIGNATURES).unwrap_or(26);
            usize::try_from(self.cfg.max_live_monitored_wallets)
                .unwrap_or(0)
                .min(usize::try_from(adm.available / per).unwrap_or(usize::MAX))
        };
        let capacity = if adm.reason().is_some() { 0 } else { capacity };
        let states = [lc::PRIORITIZED.to_owned(), lc::MONITORED.to_owned()];
        let wallets = self.db.radar_wallets_in(&states, 500).await?;
        let cooldown_since = ts(at - self.cfg.monitoring_cooldown);
        let ended: HashMap<String, UtcTimestamp> = self
            .db
            .radar_monitoring_last_ended(&cooldown_since)
            .await?
            .into_iter()
            .collect();
        let cands: Vec<rot::Candidate> = wallets
            .iter()
            .map(|w| rot::Candidate {
                address: w.address.clone(),
                priority: w
                    .priority_score
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(Decimal::ZERO),
                last_ended_at: ended.get(&w.address).map(odt),
            })
            .collect();
        let current_rows = self.db.radar_monitoring(true, 1000).await?;
        let current: Vec<rot::Assignment> = current_rows
            .iter()
            .map(|m| rot::Assignment {
                address: m.address.clone(),
                priority: m
                    .priority_at_start
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(Decimal::ZERO),
                started_at: odt(&m.started_at),
            })
            .collect();
        let plan = rot::rotate(
            &current,
            &cands,
            &RotationPolicy {
                capacity,
                min_duration: self.cfg.monitoring_min_duration,
                cooldown: self.cfg.monitoring_cooldown,
                margin: Decimal::from(5),
            },
            at,
        );
        let mode = if live {
            mon::LIVE_SUBSCRIPTION
        } else {
            mon::PERIODIC_HISTORY_REFRESH
        };
        let mut changes = 0;
        for (addr, reason) in &plan.stop {
            let rev = self
                .db
                .radar_wallet(addr)
                .await?
                .map_or(1, |w| w.revision + 1);
            let e = ev::wallet_event(
                ev::WALLET_MONITORING_CHANGED,
                addr,
                rev,
                "RADAR",
                None,
                json!({"monitoring": "INACTIVE", "reason": reason}),
            );
            if self
                .db
                .radar_stop_monitoring(addr, reason, Some(&e))
                .await?
            {
                changes += 1;
            }
        }
        for addr in &plan.start {
            let w = wallets.iter().find(|w| &w.address == addr);
            let rev = w.map_or(1, |w| w.revision + 1);
            let e = ev::wallet_event(
                ev::WALLET_MONITORING_CHANGED,
                addr,
                rev,
                "RADAR",
                None,
                json!({"monitoring": mode}),
            );
            if self
                .db
                .radar_start_monitoring(
                    addr,
                    mode,
                    w.and_then(|w| w.priority_score.as_deref()),
                    Uuid::new_v4(),
                    Some(&e),
                )
                .await?
            {
                changes += 1;
            }
        }
        // Periodic refresh: one bounded history page since the newest
        // signature already covered.
        for m in self.db.radar_monitoring(true, 1000).await? {
            if m.mode != mon::PERIODIC_HISTORY_REFRESH {
                continue;
            }
            let last = self
                .db
                .radar_jobs(&JobFilter {
                    work_class: Some(work::ACTIVE_MONITORING.into()),
                    subject: Some(m.address.clone()),
                    limit: 1,
                    ..JobFilter::default()
                })
                .await?;
            if last
                .first()
                .is_some_and(|j| at - odt(&j.created_at) < self.cfg.candidate_reevaluation_interval)
            {
                continue;
            }
            let newest = self.db.radar_wallet_signature_bounds(&m.address).await?.0;
            self.db
                .radar_create_job(&NewJob {
                    job_id: Uuid::new_v4(),
                    work_class: work::ACTIVE_MONITORING.into(),
                    subject_kind: "WALLET".into(),
                    subject: m.address.clone(),
                    origin: "AUTONOMOUS".into(),
                    bucket: None,
                    cycle_id: None,
                    priority: 300,
                    signature_limit: MONITORING_REFRESH_SIGNATURES,
                    lookback_days: None,
                    start_before_signature: None,
                    until_signature: newest,
                    min_slot: None,
                    commitment: "confirmed".into(),
                    estimated_credits: 1 + MONITORING_REFRESH_SIGNATURES,
                    next_run_at: None,
                })
                .await?;
        }
        self.metrics
            .monitoring_changes
            .fetch_add(u64::try_from(changes).unwrap_or(0), Ordering::Relaxed);
        Ok(changes)
    }

    // ---- maintenance -------------------------------------------------------------------------

    /// Deliver RADAR events to the built-in log consumer and apply retention.
    pub async fn maintenance_tick(&self, retention: bool) -> Result<(), RadarError> {
        for e in self.db.radar_events_pending(LOG_CONSUMER, 500).await? {
            tracing::debug!(target: "solprint::radar::events", event = %e.event_type, aggregate = %e.aggregate_id, revision = e.aggregate_revision, "radar event");
            self.db.radar_events_mark(LOG_CONSUMER, e.event_id).await?;
        }
        if retention {
            let r = self
                .db
                .radar_retention(
                    self.cfg.market_observation_retention_days,
                    self.cfg.snapshot_retention_days,
                    self.cfg.job_retention_days,
                    &[LOG_CONSUMER.to_owned()],
                )
                .await?;
            tracing::info!(target: "solprint::radar", observations = r.observations_deleted, snapshots = r.snapshots_deleted, jobs = r.jobs_deleted, events = r.events_deleted, "radar retention");
        }
        Ok(())
    }

    /// One full local pass (events, jobs, evaluation, monitoring). Used by
    /// the background loop, the fixture CLI and tests.
    pub async fn step(&self) -> Result<usize, RadarError> {
        let mut n = self.events_tick().await?;
        n += self.jobs_tick().await?;
        n += self.evaluate_tick().await?;
        n += self.monitoring_tick().await?;
        Ok(n)
    }
}

/// Tags, launchpad, verified flag and age (days) of a listed token.
type ListedEvidence = (Vec<String>, Option<String>, Option<bool>, Option<i64>);

fn count<'a, T: 'a>(it: impl Iterator<Item = &'a T>) -> u32 {
    u32::try_from(it.count()).unwrap_or(u32::MAX)
}

fn classify_label(s: &str) -> &'static str {
    [
        classify::MEME_CANDIDATE,
        classify::GENERAL_SPECULATIVE_TOKEN,
        classify::ESTABLISHED_ASSET,
        classify::STABLECOIN,
        classify::WRAPPED_NATIVE_ASSET,
        classify::LIQUID_STAKING_ASSET,
        classify::LIQUIDITY_POSITION_TOKEN,
        classify::KNOWN_INFRASTRUCTURE_ASSET,
    ]
    .into_iter()
    .find(|x| *x == s)
    .unwrap_or(classify::UNKNOWN)
}

/// Program address for a DATA ENGINE protocol label.
#[must_use]
pub fn program_of_protocol(p: &str) -> Option<&'static str> {
    Some(match p {
        "PUMP_FUN" => programs::PUMP_FUN,
        "PUMPSWAP" => programs::PUMPSWAP,
        "RAYDIUM_AMM_V4" => programs::RAYDIUM_AMM_V4,
        "RAYDIUM_CPMM" => programs::RAYDIUM_CPMM,
        "RAYDIUM_CLMM" => programs::RAYDIUM_CLMM,
        "ORCA_WHIRLPOOL" => programs::ORCA_WHIRLPOOL,
        _ => return None,
    })
}

fn quote_of(mint: Option<&str>, kind: &str) -> Option<String> {
    match crate::model::Denomination::of(mint, kind)? {
        crate::model::Denomination::Sol => Some(crate::model::WSOL_MINT.to_owned()),
        _ => mint.map(str::to_owned),
    }
}

/// Map a DATA ENGINE swap to a screening trade for `wallet`.
#[must_use]
pub fn trade_of(s: &SwapForRadar, wallet: &str) -> Trade {
    Trade {
        swap_id: s.swap_id.clone(),
        signature: s.signature.clone(),
        slot: s.slot,
        order: (
            s.slot,
            s.transaction_index.map_or(i64::MAX, i64::from),
            format!("{:0>12}", s.instruction_ref),
        ),
        block_time: s.block_time.as_ref().map(odt),
        input_mint: s.input_mint.clone(),
        input_kind: s.input_asset_kind.clone(),
        input_amount: s.input_amount.as_deref().and_then(|v| v.parse().ok()),
        output_mint: s.output_mint.clone(),
        output_kind: s.output_asset_kind.clone(),
        output_amount: s.output_amount.as_deref().and_then(|v| v.parse().ok()),
        wallet_paid_fee: s.fee_payer == wallet,
        fee_lamports: s.fee_lamports.parse().unwrap_or(0),
        finalized: s.finality == "FINALIZED",
    }
}

#[allow(clippy::too_many_arguments)]
fn obs(
    cycle: Uuid,
    mint: &str,
    src: &str,
    promotional: bool,
    rank: u32,
    decision: &str,
    reason: &str,
    at: &UtcTimestamp,
) -> ObservationWrite {
    ObservationWrite {
        cycle_id: Some(cycle),
        mint: mint.to_owned(),
        pool: None,
        source: src.to_owned(),
        promotional,
        source_rank: i32::try_from(rank).ok(),
        liquidity_usd: None,
        volume_24h_usd: None,
        txns_1h: None,
        txns_24h: None,
        pair_created_at: None,
        organic_score: None,
        decision: Some(decision.to_owned()),
        decision_reasons: vec![reason.to_owned()],
        observed_at: *at,
    }
}

/// The coverage/result code a finished job records.
fn job_result(p: &st::BackfillProgress, j: &JobRow) -> &'static str {
    if p.status == "FAILED" {
        return if j.work_class == work::TRANSACTION_SAMPLING {
            "SAMPLE_FAILED"
        } else {
            coverage::PROVIDER_HISTORY_UNAVAILABLE
        };
    }
    if p.status == "CANCELLED" {
        return "CANCELLED";
    }
    if j.work_class == work::TRANSACTION_SAMPLING {
        return if p.transactions_pending > 0 || p.transactions_unavailable > 0 {
            "SAMPLE_PARTIAL"
        } else {
            "SAMPLE_COMPLETE"
        };
    }
    if p.transactions_pending > 0 || p.transactions_unavailable > 0 {
        return coverage::UNRESOLVED_GAPS;
    }
    if p.transactions_retrieved > 0 && p.transactions_unsupported * 2 > p.transactions_retrieved {
        return coverage::DECODER_COVERAGE_LIMITED;
    }
    match p.stop_reason.as_deref() {
        Some("HISTORY_EXHAUSTED" | "MIN_SLOT_REACHED" | "UNTIL_REACHED") => {
            coverage::REQUESTED_SCOPE_COMPLETE
        }
        Some("LIMIT_REACHED") => coverage::BOUNDED_SAMPLE_COMPLETE,
        _ => coverage::PARTIAL_HISTORY,
    }
}

// ---- background loops --------------------------------------------------------------------------

/// Running RADAR loops.
#[derive(Debug)]
pub struct RadarHandle {
    tasks: JoinSet<()>,
    cancel: CancellationToken,
}

impl RadarHandle {
    /// Stop the loops and return leased jobs to the queue.
    pub async fn shutdown<F: MarketFeed>(mut self, radar: &Radar<F>) {
        self.cancel.cancel();
        while self.tasks.join_next().await.is_some() {}
        match radar.db.radar_release_owner(radar.owner).await {
            Ok(n) if n > 0 => {
                tracing::info!(target: "solprint::radar", released = n, "released leased jobs")
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(target: "solprint::radar", error = %e, "could not release leases")
            }
        }
    }
}

async fn sleep_or_cancel(d: Duration, c: &CancellationToken) -> bool {
    tokio::select! {
        () = c.cancelled() => false,
        () = tokio::time::sleep(d) => true,
    }
}

const STEP_INTERVAL: Duration = Duration::from_secs(5);
const RETENTION_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Start RADAR's loops. Nothing starts when RADAR is disabled. Discovery
/// cycles run on schedule only with `RADAR_AUTO_DISCOVERY_ENABLED=true`;
/// an operator-requested cycle (`solprint radar cycle`) runs once either way.
pub fn start<F: MarketFeed>(
    radar: &Arc<Radar<F>>,
    parent: &CancellationToken,
) -> Option<RadarHandle> {
    if !radar.cfg.enabled {
        return None;
    }
    let cancel = parent.child_token();
    let mut tasks = JoinSet::new();
    {
        let (r, c) = (Arc::clone(radar), cancel.clone());
        tasks.spawn(async move {
            while sleep_or_cancel(STEP_INTERVAL, &c).await {
                if let Err(e) = r.step().await {
                    r.metrics.error("STEP_FAILED");
                    tracing::warn!(target: "solprint::radar", error = %e, "radar step failed");
                }
            }
        });
    }
    {
        let (r, c) = (Arc::clone(radar), cancel.clone());
        tasks.spawn(async move {
            let tick = Duration::from_secs(15);
            while sleep_or_cancel(tick, &c).await {
                let control = match r.db.radar_control().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(target: "solprint::radar", error = %e, "control read failed");
                        continue;
                    }
                };
                if control.paused {
                    continue;
                }
                let due = control
                    .last_cycle_at
                    .is_none_or(|t| now() - odt(&t) >= r.cfg.market_discovery_interval);
                let trigger = if control.cycle_requested {
                    Some("OPERATOR")
                } else if r.cfg.auto_discovery_enabled && due {
                    Some("SCHEDULED")
                } else {
                    None
                };
                if let Some(t) = trigger
                    && let Err(e) = r.discovery_cycle(t).await
                {
                        r.metrics.error("CYCLE_FAILED");
                        tracing::warn!(target: "solprint::radar", error = %e, "discovery cycle failed");
                        let _ = r.db.radar_request_cycle(false).await;
                }
            }
        });
    }
    {
        let (r, c) = (Arc::clone(radar), cancel.clone());
        tasks.spawn(async move {
            let tick = r.cfg.registry_maintenance_interval.max(Duration::from_secs(10));
            let mut since = RETENTION_EVERY;
            while sleep_or_cancel(tick, &c).await {
                since = since.saturating_add(tick);
                let retention = since >= RETENTION_EVERY;
                if retention {
                    since = Duration::ZERO;
                }
                if let Err(e) = r.maintenance_tick(retention).await {
                    tracing::warn!(target: "solprint::radar", error = %e, "radar maintenance failed");
                }
            }
        });
    }
    Some(RadarHandle { tasks, cancel })
}
