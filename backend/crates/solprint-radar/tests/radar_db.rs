//! RADAR end to end against PostgreSQL with the real DATA ENGINE, an
//! in-memory chain source and a fixture market feed.
//!
//! No provider is contacted and no credits are consumed. Ignored by default
//! because they need a disposable PostgreSQL server; run with
//! `scripts/test.ps1`, which starts one and passes `--include-ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rust_decimal::Decimal;
use solprint_config::{Config, DataEngineConfig, RadarConfig};
use solprint_core::{CommitmentLevel, TransactionSignature, UtcTimestamp};
use solprint_data_engine::engine::Engine;
use solprint_data_engine::fixtures::addr;
use solprint_data_engine::programs::RAYDIUM_AMM_V4;
use solprint_data_engine::source::{
    ChainSource, FetchedTx, PageResult, SourceError, StatusesResult,
};
use solprint_gateway::Priority;
use solprint_gateway::models::{SignatureInfo, SignaturePage, SignatureQuery, SignatureStatus};
use solprint_radar::engine::DataEngineLimits;
use solprint_radar::feed::{FeedError, ListedPool, ListedToken, Listing};
use solprint_radar::fixtures::{SwapSpec, round_trips};
use solprint_radar::model::{eligibility, lifecycle as lc, work};
use solprint_radar::{FixtureFeed, Radar};
use solprint_storage::Database;
use solprint_storage::radar::{JobFilter, RADAR_CONSUMER, WalletFilter};
use solprint_storage::test_support::TestDatabase;
use tokio_util::sync::CancellationToken;

// ---- in-memory chain source ---------------------------------------------------------------

#[derive(Default)]
struct Chain {
    txs: Mutex<HashMap<String, (String, u64)>>,
    /// Address → signatures (newest first).
    history: Mutex<HashMap<String, Vec<(String, u64)>>>,
    page_calls: AtomicUsize,
    tx_calls: AtomicUsize,
}

impl Chain {
    fn add(&self, s: &SwapSpec, addresses: &[&str]) {
        let sig = s.signature();
        self.txs
            .lock()
            .unwrap()
            .insert(sig.clone(), (s.build(), s.slot));
        let mut h = self.history.lock().unwrap();
        for a in addresses {
            let v = h.entry((*a).to_owned()).or_default();
            if !v.iter().any(|(x, _)| *x == sig) {
                v.push((sig.clone(), s.slot));
                v.sort_by_key(|a| std::cmp::Reverse(a.1));
            }
        }
    }
}

impl ChainSource for Chain {
    async fn transaction(
        &self,
        signature: &TransactionSignature,
        _commitment: CommitmentLevel,
        _priority: Priority,
        _refresh: bool,
        _cancel: &CancellationToken,
    ) -> Result<Option<FetchedTx>, SourceError> {
        self.tx_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .txs
            .lock()
            .unwrap()
            .get(&signature.to_string())
            .map(|(raw, slot)| FetchedTx {
                raw_json: raw.clone(),
                slot: *slot,
                provider: "FIXTURE",
                fetched_at: UtcTimestamp::now(),
                credits: 1,
                cached: false,
            }))
    }

    async fn signature_statuses(
        &self,
        signatures: &[TransactionSignature],
        _cancel: &CancellationToken,
    ) -> Result<StatusesResult, SourceError> {
        let txs = self.txs.lock().unwrap();
        Ok(StatusesResult {
            statuses: signatures
                .iter()
                .map(|s| {
                    txs.get(&s.to_string()).map(|(_, slot)| SignatureStatus {
                        slot: *slot,
                        confirmations: None,
                        failed: false,
                        err_json: None,
                        confirmation_status: Some(CommitmentLevel::Finalized),
                    })
                })
                .collect(),
            credits: 1,
        })
    }

    async fn signatures_for_address(
        &self,
        q: &SignatureQuery,
        _commitment: CommitmentLevel,
        _cancel: &CancellationToken,
    ) -> Result<PageResult, SourceError> {
        self.page_calls.fetch_add(1, Ordering::SeqCst);
        let all = self
            .history
            .lock()
            .unwrap()
            .get(q.address.as_str())
            .cloned()
            .unwrap_or_default();
        let before = q.before.as_ref().map(ToString::to_string);
        let until = q.until.as_ref().map(ToString::to_string);
        let mut started = before.is_none();
        let mut items = Vec::new();
        for (s, slot) in all {
            if !started {
                started = Some(&s) == before.as_ref();
                continue;
            }
            if Some(&s) == until.as_ref() {
                break;
            }
            items.push(SignatureInfo {
                signature: TransactionSignature::parse(&s).unwrap(),
                slot,
                block_time: None,
                failed: false,
                err_json: None,
                memo: None,
                confirmation_status: Some(CommitmentLevel::Finalized),
            });
        }
        let limit = usize::from(q.page_limit);
        let complete = items.len() <= limit;
        items.truncate(limit);
        let next_before = if complete {
            None
        } else {
            items.last().map(|i| i.signature.clone())
        };
        Ok(PageResult {
            page: SignaturePage {
                items,
                complete,
                next_before,
                pages_fetched: 1,
            },
            credits: 1,
        })
    }

    fn admits(&self, _priority: Priority) -> bool {
        true
    }
}

// ---- harness ------------------------------------------------------------------------------

fn configs() -> (DataEngineConfig, RadarConfig) {
    let mut map: HashMap<String, String> = HashMap::new();
    map.insert(
        "DATABASE_URL".into(),
        "postgres://solprint@127.0.0.1:5432/solprint".into(),
    );
    map.insert("RADAR_AUTO_DISCOVERY_ENABLED".into(), "true".into());
    let c = Config::from_lookup(|k| map.get(k).cloned()).unwrap();
    let mut de = c.data_engine;
    de.finality_interval = Duration::ZERO;
    let mut r = c.radar;
    // Re-evaluate immediately so the test can step the pipeline.
    r.candidate_reevaluation_interval = Duration::ZERO;
    r.monitoring_min_duration = Duration::ZERO;
    r.monitoring_cooldown = Duration::ZERO;
    (de, r)
}

struct World {
    _tdb: TestDatabase,
    db: Database,
    chain: Arc<Chain>,
    de: Arc<Engine<Chain>>,
    feed: FixtureFeed,
    radar: Radar<FixtureFeed>,
}

async fn world() -> World {
    world_with(|_| {}).await
}

async fn world_with(f: impl FnOnce(&mut RadarConfig)) -> World {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    let chain = Arc::new(Chain::default());
    let (de_cfg, mut r_cfg) = configs();
    f(&mut r_cfg);
    let de = Arc::new(Engine::new(db.clone(), Arc::clone(&chain), de_cfg.clone()));
    de.register_consumer(RADAR_CONSUMER);
    let feed = FixtureFeed::with_budget(1_000_000);
    let radar = Radar::new(
        db.clone(),
        Arc::new(feed.clone()),
        r_cfg,
        DataEngineLimits::of(&de_cfg),
    );
    World {
        _tdb: tdb,
        db,
        chain,
        de,
        feed,
        radar,
    }
}

const NOW_SLOT: u64 = 310_000_000;

fn now_unix() -> i64 {
    UtcTimestamp::now().as_offset_datetime().unix_timestamp()
}

/// One meme market with a supported Raydium pool and an active trader.
fn seed_market(w: &World, mint_label: &str, pool_label: &str) -> String {
    let mint = addr(mint_label);
    w.feed.set_listing(
        Listing::JupiterTopTraded,
        vec![ListedToken {
            mint: mint.clone(),
            symbol: Some("FIX".into()),
            launchpad: Some("fixture-launchpad".into()),
            ..ListedToken::default()
        }],
    );
    w.feed.add_pool(
        ListedPool {
            pool: addr(pool_label),
            base_mint: mint.clone(),
            quote_mint: Some(solprint_radar::model::WSOL_MINT.into()),
            dex_id: Some("raydium".into()),
            labels: Vec::new(),
            liquidity_usd: Some(Decimal::from(80_000)),
            volume_24h_usd: Some(Decimal::from(400_000)),
            txns_1h: Some(300),
            txns_24h: Some(5_000),
            pair_created_at: None,
        },
        RAYDIUM_AMM_V4,
    );
    mint
}

/// Step RADAR and the DATA ENGINE until `done` or `rounds` passes.
async fn run(w: &World, rounds: usize, done: impl AsyncFn(&World) -> bool) -> bool {
    let c = CancellationToken::new();
    for _ in 0..rounds {
        w.radar.step().await.unwrap();
        for _ in 0..5 {
            if w.de.backfill_tick(&c).await.unwrap().is_none() {
                break;
            }
        }
        for _ in 0..10 {
            if w.de.run_batch(&c).await.unwrap() == 0 {
                break;
            }
        }
        w.de.finality_tick(&c).await.unwrap();
        w.radar.step().await.unwrap();
        if done(w).await {
            return true;
        }
    }
    false
}

async fn lifecycle_of(w: &World, wallet: &str) -> Option<String> {
    w.db.radar_wallet(wallet)
        .await
        .unwrap()
        .map(|x| x.lifecycle)
}

// ---- tests --------------------------------------------------------------------------------

/// The headline test: from an empty registry and no configured wallets or
/// targets, RADAR discovers a market, samples it through the DATA ENGINE,
/// finds the trader from verified swaps, investigates it in stages,
/// qualifies it from matched outcomes and assigns monitoring.
#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn autonomous_discovery_from_an_empty_registry() {
    let w = world().await;
    assert_eq!(w.db.radar_counts().await.unwrap().wallets_total, 0);
    assert!(w.db.de_targets().await.unwrap().is_empty());
    let mint = seed_market(&w, "mint:radar-e2e", "pool:radar-e2e");
    let pool = addr("pool:radar-e2e");
    let trader = addr("wallet:trader");
    let tourist = addr("wallet:tourist");
    // The trader: 6 profitable round trips over 5 days, last one 2 hours ago.
    let trips = round_trips(
        "wallet:trader",
        "pool:radar-e2e",
        &mint,
        6,
        20,
        2,
        now_unix(),
        NOW_SLOT,
    );
    for s in &trips {
        w.chain.add(s, &[&pool, &trader]);
    }
    // A one-off buyer.
    let one = SwapSpec {
        label: "wallet:tourist:buy".into(),
        wallet: tourist.clone(),
        pool_label: "pool:radar-e2e".into(),
        mint: mint.clone(),
        buy: true,
        sol: 500_000_000,
        tokens: 400_000,
        token_pre: None,
        slot: NOW_SLOT + 500,
        block_time: now_unix() - 3600,
    };
    w.chain.add(&one, &[&pool, &tourist]);

    let rep = w.radar.discovery_cycle("FIXTURE").await.unwrap();
    assert_eq!(rep.status, "COMPLETED", "{rep:?}");
    assert_eq!(rep.markets_new, 1);
    assert_eq!(rep.samples_scheduled, 1);
    let m = w.db.radar_market(&mint).await.unwrap().unwrap();
    assert_eq!(m.classification, "MEME_CANDIDATE");
    let p = w.db.radar_pool(&pool).await.unwrap().unwrap();
    assert_eq!(
        (p.validation.as_str(), p.protocol.as_deref()),
        ("SUPPORTED", Some("RAYDIUM_AMM_V4"))
    );

    // Monitoring and finality advance independently: wait for both (the
    // DATA ENGINE's TransactionFinalized events turn PROVISIONAL into
    // ELIGIBLE once RADAR consumes them).
    let ok = run(&w, 60, async |w: &World| {
        w.db.radar_wallet(&addr("wallet:trader"))
            .await
            .unwrap()
            .is_some_and(|t| {
                t.lifecycle == lc::MONITORED && t.eligibility == eligibility::ELIGIBLE_CANDIDATE
            })
    })
    .await;
    let t =
        w.db.radar_wallet(&trader)
            .await
            .unwrap()
            .expect("trader discovered");
    assert!(
        ok,
        "trader ended in {} / {} ({:?})",
        t.lifecycle, t.eligibility, t.qualification
    );
    assert_eq!(t.eligibility, eligibility::ELIGIBLE_CANDIDATE);
    assert_eq!(t.history_stage, "C");
    assert_eq!(
        t.qualification.as_deref(),
        Some("READY_FOR_DEEPER_ANALYSIS")
    );
    assert_eq!(t.monitoring_mode, "PERIODIC_HISTORY_REFRESH");
    assert_eq!(t.verified_swap_count, 12);
    assert!(t.priority_score.is_some());
    // Provenance: discovered by market sampling, then by its own history.
    let sources = w.db.radar_wallet_sources(&trader).await.unwrap();
    assert!(
        sources
            .iter()
            .any(|s| s == "ESTABLISHED" || s == "EXPLORATION"),
        "{sources:?}"
    );
    // Every lifecycle step is recorded.
    let tr = w.db.radar_wallet_transitions(&trader, 50).await.unwrap();
    let states: Vec<&str> = tr.iter().rev().map(|x| x.to_state.as_str()).collect();
    for s in [
        lc::DISCOVERED,
        lc::HISTORY_QUEUED,
        lc::PRELIMINARY_QUALIFIED,
        lc::PRIORITIZED,
        lc::MONITORED,
    ] {
        assert!(states.contains(&s), "missing {s} in {states:?}");
    }
    // Snapshots are versioned and carry the formula components.
    let snaps = w.db.radar_wallet_snapshots(&trader, 10).await.unwrap();
    assert!(!snaps.is_empty());
    assert!(snaps[0].metrics_json.contains("evidenceQuality"));

    // Later phases read the registry through the typed query interface.
    let q = solprint_radar::query::RadarQuery::new(w.db.clone());
    let ready = q.ready_for_analysis(None, 10).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].address, trader);
    assert_eq!(ready[0].coverage_state, t.history_coverage_state);
    assert_eq!(q.monitored(10).await.unwrap().len(), 1);
    assert_eq!(q.evidence(&trader, 100).await.unwrap().len(), 12);

    // The one-off buyer is known but not promoted.
    let tw =
        w.db.radar_wallet(&tourist)
            .await
            .unwrap()
            .expect("tourist discovered");
    assert_eq!(tw.lifecycle, lc::INSUFFICIENT_EVIDENCE);

    // RADAR events were emitted with unique (aggregate, revision, type).
    let events = w.db.radar_events_recent(200).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.event_type == "WalletCandidateDiscovered" && e.aggregate_id == trader)
    );
    assert!(
        events
            .iter()
            .any(|e| e.event_type == "WalletMonitoringChanged")
    );
    // No DATA ENGINE target was created: periodic monitoring does not need one.
    assert!(w.db.de_targets().await.unwrap().is_empty());
    // Every RADAR job finished or is a scheduled refresh.
    let jobs =
        w.db.radar_jobs(&JobFilter {
            limit: 100,
            ..JobFilter::default()
        })
        .await
        .unwrap();
    assert!(
        jobs.iter()
            .all(|j| j.state == "COMPLETED" || j.work_class == work::ACTIVE_MONITORING),
        "{:?}",
        jobs.iter()
            .map(|j| (&j.work_class, &j.state, &j.defer_reason))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn event_redelivery_is_idempotent_and_retraction_invalidates_evidence() {
    let w = world().await;
    let mint = seed_market(&w, "mint:radar-idem", "pool:radar-idem");
    let pool = addr("pool:radar-idem");
    let trader = addr("wallet:idem");
    for s in round_trips(
        "wallet:idem",
        "pool:radar-idem",
        &mint,
        1,
        1,
        1,
        now_unix(),
        NOW_SLOT,
    ) {
        w.chain.add(&s, &[&pool, &trader]);
    }
    w.radar.discovery_cycle("FIXTURE").await.unwrap();
    let found = run(&w, 10, async |w: &World| {
        w.db.radar_wallet(&addr("wallet:idem"))
            .await
            .unwrap()
            .is_some()
    })
    .await;
    assert!(found);
    let ev = w.db.radar_wallet_evidence(&trader, 100).await.unwrap();
    assert_eq!(ev.len(), 2);
    // Re-apply every swap as if the events were delivered again.
    for e in &ev {
        let s = w.db.radar_swap(&e.swap_id).await.unwrap().unwrap();
        w.radar.apply_swap(None, &s).await.unwrap();
    }
    assert_eq!(
        w.db.radar_wallet_evidence(&trader, 100)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(w.db.radar_counts().await.unwrap().wallets_total, 1);

    // Retract one swap: the evidence stays, invalidated with its reason.
    let gone =
        w.db.radar_invalidate_swap(None, &ev[0].swap_id, "RETRACTED_ORPHANED")
            .await
            .unwrap();
    assert_eq!(gone, vec![trader.clone()]);
    let after = w.db.radar_wallet_evidence(&trader, 100).await.unwrap();
    let inv = after.iter().find(|e| e.swap_id == ev[0].swap_id).unwrap();
    assert!(!inv.valid);
    assert_eq!(
        inv.invalidation_reason.as_deref(),
        Some("RETRACTED_ORPHANED")
    );
    let wr = w.db.radar_wallet(&trader).await.unwrap().unwrap();
    assert!(wr.needs_evaluation);
    assert_eq!(wr.valid_evidence_count, 1);
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn degraded_or_unknown_budget_pauses_provider_work() {
    let w = world().await;
    seed_market(&w, "mint:radar-budget", "pool:radar-budget");
    w.feed.set_budget(None);
    let rep = w.radar.discovery_cycle("FIXTURE").await.unwrap();
    assert_eq!(
        (rep.status.as_str(), rep.deferred_reason.as_deref()),
        ("DEFERRED", Some("BUDGET_UNKNOWN"))
    );
    assert_eq!(
        w.feed.calls(),
        0,
        "no provider call while the budget is unknown"
    );
    w.feed.set_budget(Some(solprint_radar::budget::BudgetView {
        state: "CRITICAL".into(),
        normal_credits: 1000,
        credits_used: 10,
        ledger_loaded: true,
    }));
    let rep = w.radar.discovery_cycle("FIXTURE").await.unwrap();
    assert_eq!(rep.deferred_reason.as_deref(), Some("BUDGET_CRITICAL"));
    assert_eq!(w.feed.calls(), 0);
    assert_eq!(w.db.radar_job_counts().await.unwrap().open_total, 0);
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn provider_outage_is_recorded_and_local_sources_still_run() {
    let w = world().await;
    seed_market(&w, "mint:radar-outage", "pool:radar-outage");
    w.feed.fail_with(Some(FeedError::Cooldown("RATE_LIMITED")));
    let rep = w.radar.discovery_cycle("FIXTURE").await.unwrap();
    assert_eq!(rep.status, "PARTIAL");
    let runs =
        w.db.radar_source_metrics(&UtcTimestamp::from_offset_datetime(
            time::OffsetDateTime::UNIX_EPOCH,
        ))
        .await
        .unwrap();
    let jt = runs
        .iter()
        .find(|r| r.source == "JUPITER_TOP_TRADED")
        .unwrap();
    assert_eq!(jt.last_status.as_deref(), Some("PROVIDER_COOLDOWN"));
    assert!(runs.iter().any(|r| r.source == "LOCAL_ACTIVITY"));
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn discovery_off_means_no_provider_jobs_run() {
    let w = world_with(|c| c.auto_discovery_enabled = false).await;
    let mint = seed_market(&w, "mint:radar-off", "pool:radar-off");
    let pool = addr("pool:radar-off");
    let trader = addr("wallet:off");
    for s in round_trips(
        "wallet:off",
        "pool:radar-off",
        &mint,
        1,
        1,
        1,
        now_unix(),
        NOW_SLOT,
    ) {
        w.chain.add(&s, &[&pool, &trader]);
    }
    // An operator-requested cycle still records markets and queues work…
    let rep = w.radar.discovery_cycle("OPERATOR").await.unwrap();
    assert_eq!(rep.samples_scheduled, 1);
    // …and operator-origin work runs, but nothing autonomous is scheduled
    // for discovered wallets while discovery is off.
    run(&w, 6, async |_: &World| false).await;
    let jobs =
        w.db.radar_jobs(&JobFilter {
            limit: 100,
            ..JobFilter::default()
        })
        .await
        .unwrap();
    assert!(
        jobs.iter()
            .all(|j| j.work_class == work::TRANSACTION_SAMPLING),
        "{jobs:?}"
    );
    let t = w.db.radar_wallet(&trader).await.unwrap().unwrap();
    assert_eq!(t.lifecycle, lc::SCREENING);
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn restart_resumes_from_durable_state_without_duplicate_jobs() {
    let w = world().await;
    let mint = seed_market(&w, "mint:radar-restart", "pool:radar-restart");
    let pool = addr("pool:radar-restart");
    let trader = addr("wallet:restart");
    for s in round_trips(
        "wallet:restart",
        "pool:radar-restart",
        &mint,
        3,
        20,
        2,
        now_unix(),
        NOW_SLOT,
    ) {
        w.chain.add(&s, &[&pool, &trader]);
    }
    w.radar.discovery_cycle("FIXTURE").await.unwrap();
    // A second cycle right away does not duplicate the open sample.
    let again = w.radar.discovery_cycle("FIXTURE").await.unwrap();
    assert_eq!(again.samples_scheduled, 0);
    run(&w, 3, async |_: &World| false).await;
    // "Restart": a new RADAR instance over the same database.
    let (de_cfg, mut r_cfg) = configs();
    r_cfg.candidate_reevaluation_interval = Duration::ZERO;
    let radar2 = Radar::new(
        w.db.clone(),
        Arc::new(w.feed.clone()),
        r_cfg,
        DataEngineLimits::of(&de_cfg),
    );
    let c = CancellationToken::new();
    for _ in 0..30 {
        radar2.step().await.unwrap();
        while w.de.backfill_tick(&c).await.unwrap().is_some() {}
        while w.de.run_batch(&c).await.unwrap() > 0 {}
        radar2.step().await.unwrap();
    }
    let t = w.db.radar_wallet(&trader).await.unwrap().unwrap();
    assert!(t.history_stage != "NONE", "{t:?}");
    // At most one job per (class, subject) was ever open at once, and no
    // stage ran twice.
    let jobs =
        w.db.radar_jobs(&JobFilter {
            subject: Some(trader.clone()),
            limit: 100,
            ..JobFilter::default()
        })
        .await
        .unwrap();
    let initial = jobs
        .iter()
        .filter(|j| j.work_class == work::INITIAL_HISTORY)
        .count();
    assert_eq!(initial, 1, "{jobs:?}");
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn operator_controls_and_registry_queries() {
    let w = world().await;
    let a = addr("wallet:operator");
    assert!(
        w.db.radar_register_operator_wallet(uuid::Uuid::new_v4(), &a)
            .await
            .unwrap()
    );
    assert!(
        !w.db
            .radar_register_operator_wallet(uuid::Uuid::new_v4(), &a)
            .await
            .unwrap()
    );
    assert!(w.db.radar_set_operator_excluded(&a, true).await.unwrap());
    w.radar.evaluate_tick().await.unwrap();
    let r = w.db.radar_wallet(&a).await.unwrap().unwrap();
    assert_eq!(
        (r.eligibility.as_str(), r.lifecycle.as_str()),
        (eligibility::EXCLUDED_BY_POLICY, lc::REJECTED)
    );
    assert!(
        w.db.radar_reactivate(&a, "OPERATOR_REQUEST", None)
            .await
            .unwrap()
    );
    let r = w.db.radar_wallet(&a).await.unwrap().unwrap();
    assert_eq!(r.lifecycle, lc::DISCOVERED);
    assert!(!r.operator_excluded);
    // Pause stops job claims; the control row survives restarts.
    w.db.radar_set_paused(true, Some("OPERATOR_PAUSE"))
        .await
        .unwrap();
    assert!(w.db.radar_control().await.unwrap().paused);
    let page =
        w.db.radar_wallets(&WalletFilter {
            limit: 10,
            ..WalletFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
}

/// Inactivity becomes dormancy only once coverage shows the quiet period;
/// new verified activity reactivates the wallet and reuses its history.
#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn dormancy_is_coverage_aware_and_new_activity_reactivates() {
    let w = world().await;
    let mint = seed_market(&w, "mint:radar-dormant", "pool:radar-dormant");
    let pool = addr("pool:radar-dormant");
    let trader = addr("wallet:dormant");
    // Six round trips, the last one 20 days ago (dormancy threshold: 14).
    for s in round_trips(
        "wallet:dormant",
        "pool:radar-dormant",
        &mint,
        6,
        20,
        20 * 24,
        now_unix(),
        NOW_SLOT,
    ) {
        w.chain.add(&s, &[&pool, &trader]);
    }
    w.radar.discovery_cycle("FIXTURE").await.unwrap();
    let ok = run(&w, 30, async |w: &World| {
        lifecycle_of(w, &addr("wallet:dormant")).await.as_deref() == Some(lc::DORMANT)
    })
    .await;
    let t = w.db.radar_wallet(&trader).await.unwrap().unwrap();
    assert!(ok, "ended in {} / stage {}", t.lifecycle, t.history_stage);
    let tr = w.db.radar_wallet_transitions(&trader, 50).await.unwrap();
    assert_eq!(tr[0].to_state, lc::DORMANT);
    assert!(
        tr[0]
            .reasons
            .iter()
            .any(|r| r == "DORMANCY_THRESHOLD_REACHED"),
        "{:?}",
        tr[0].reasons
    );

    // A new verified swap arrives (here through an operator-style backfill
    // of the wallet); the wallet is reopened.
    let fresh = SwapSpec {
        label: "wallet:dormant:fresh".into(),
        wallet: trader.clone(),
        pool_label: "pool:radar-dormant".into(),
        mint: mint.clone(),
        buy: true,
        sol: 1_000_000_000,
        tokens: 1_000_000,
        token_pre: None,
        slot: NOW_SLOT + 1_000,
        block_time: now_unix() - 600,
    };
    w.chain.add(&fresh, &[&pool, &trader]);
    w.db.de_create_backfill(&solprint_storage::data_engine::NewBackfill {
        backfill_id: uuid::Uuid::new_v4(),
        target_id: None,
        address: trader.clone(),
        commitment: "confirmed".into(),
        start_before_signature: None,
        until_signature: None,
        min_slot: Some(NOW_SLOT + 500),
        page_size: 10,
        max_pages: 1,
        max_signatures: 10,
        credits_estimated: 11,
    })
    .await
    .unwrap();
    let ok = run(&w, 10, async |w: &World| {
        w.db.radar_wallet_transitions(&addr("wallet:dormant"), 50)
            .await
            .unwrap()
            .iter()
            .any(|x| x.reasons.iter().any(|r| r == "NEW_VERIFIED_ACTIVITY"))
    })
    .await;
    let t = w.db.radar_wallet(&trader).await.unwrap().unwrap();
    assert!(ok, "after new activity: {}", t.lifecycle);
    assert_ne!(t.lifecycle, lc::DORMANT);
    // History already fetched is reused: stage A is not repeated.
    let jobs =
        w.db.radar_jobs(&JobFilter {
            subject: Some(trader.clone()),
            limit: 100,
            ..JobFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(
        jobs.iter()
            .filter(|j| j.work_class == work::INITIAL_HISTORY)
            .count(),
        1,
        "{jobs:?}"
    );
}
