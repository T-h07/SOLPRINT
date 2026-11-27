//! DATA ENGINE end to end against PostgreSQL, with an in-memory chain source.
//!
//! No provider is contacted and no credits are consumed: the source serves the
//! synthetic fixture catalogue. Ignored by default because they need a
//! disposable PostgreSQL server; run with `scripts/test.ps1`, which starts one
//! and passes `--include-ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use solprint_config::{Config, DataEngineConfig};
use solprint_core::{CommitmentLevel, TransactionSignature, UtcTimestamp};
use solprint_data_engine::engine::Engine;
use solprint_data_engine::fixtures::{self, Fixture};
use solprint_data_engine::source::{
    ChainSource, FetchedTx, PageResult, SourceError, StatusesResult,
};
use solprint_gateway::Priority;
use solprint_gateway::context::Provenance;
use solprint_gateway::models::{SignatureInfo, SignaturePage, SignatureQuery, SignatureStatus};
use solprint_gateway::stream::{StreamEvent, StreamEventKind};
use solprint_storage::Database;
use solprint_storage::data_engine::{EnqueueOutcome, NewBackfill};
use solprint_storage::test_support::TestDatabase;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// ---- in-memory source -------------------------------------------------------------------

#[derive(Default)]
struct MemorySource {
    txs: Mutex<HashMap<String, (String, u64)>>,
    statuses: Mutex<HashMap<String, Option<SignatureStatus>>>,
    pages: Mutex<HashMap<Option<String>, SignaturePage>>,
    budget_denied: AtomicBool,
    /// Deny budget for signature pages from this call number on.
    deny_pages_from: Mutex<Option<usize>>,
    page_calls: AtomicUsize,
    fetches: Mutex<Vec<(String, CommitmentLevel, bool)>>,
}

impl MemorySource {
    fn add(&self, f: &Fixture) -> (String, u64) {
        let v: serde_json::Value = serde_json::from_str(&f.json).unwrap();
        let sig = v["transaction"]["signatures"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let slot = v["slot"].as_u64().unwrap();
        self.txs
            .lock()
            .unwrap()
            .insert(sig.clone(), (f.json.clone(), slot));
        (sig, slot)
    }

    fn status(&self, sig: &str, s: Option<SignatureStatus>) {
        self.statuses.lock().unwrap().insert(sig.to_owned(), s);
    }

    fn fetch_count(&self, sig: &str) -> usize {
        self.fetches
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _, _)| s == sig)
            .count()
    }
}

impl ChainSource for MemorySource {
    async fn transaction(
        &self,
        signature: &TransactionSignature,
        commitment: CommitmentLevel,
        _priority: Priority,
        refresh: bool,
        _cancel: &CancellationToken,
    ) -> Result<Option<FetchedTx>, SourceError> {
        if self.budget_denied.load(Ordering::SeqCst) {
            return Err(SourceError::Budget);
        }
        let key = signature.to_string();
        self.fetches
            .lock()
            .unwrap()
            .push((key.clone(), commitment, refresh));
        Ok(self
            .txs
            .lock()
            .unwrap()
            .get(&key)
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
        let map = self.statuses.lock().unwrap();
        Ok(StatusesResult {
            statuses: signatures
                .iter()
                .map(|s| map.get(&s.to_string()).cloned().flatten())
                .collect(),
            credits: 1,
        })
    }

    async fn signatures_for_address(
        &self,
        query: &SignatureQuery,
        _commitment: CommitmentLevel,
        _cancel: &CancellationToken,
    ) -> Result<PageResult, SourceError> {
        let n = self.page_calls.fetch_add(1, Ordering::SeqCst);
        if self.deny_pages_from.lock().unwrap().is_some_and(|m| n >= m) {
            return Err(SourceError::Budget);
        }
        let key = query.before.as_ref().map(ToString::to_string);
        let page = self
            .pages
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .unwrap_or(SignaturePage {
                items: Vec::new(),
                complete: true,
                next_before: None,
                pages_fetched: 1,
            });
        Ok(PageResult { page, credits: 10 })
    }

    fn admits(&self, _priority: Priority) -> bool {
        !self.budget_denied.load(Ordering::SeqCst)
    }
}

// ---- helpers ------------------------------------------------------------------------------

fn config() -> DataEngineConfig {
    let mut map: HashMap<String, String> = HashMap::new();
    map.insert(
        "DATABASE_URL".into(),
        "postgres://solprint@127.0.0.1:5432/solprint".into(),
    );
    let mut c = Config::from_lookup(|k| map.get(k).cloned())
        .unwrap()
        .data_engine;
    // Checks due immediately so the reconciler can be driven step by step.
    c.finality_interval = Duration::ZERO;
    c
}

async fn engine(src: Arc<MemorySource>) -> (TestDatabase, Arc<Engine<MemorySource>>) {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    (tdb, Arc::new(Engine::new(db, src, config())))
}

fn fixture(name: &str) -> Fixture {
    fixtures::catalog()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("fixture {name}"))
}

async fn observe(e: &Engine<MemorySource>, sig: &str) -> EnqueueOutcome {
    e.observe(
        sig,
        "MANUAL",
        None,
        Some(CommitmentLevel::Confirmed),
        None,
        None,
        3,
    )
    .await
    .unwrap()
}

async fn drain(e: &Arc<Engine<MemorySource>>) {
    let c = CancellationToken::new();
    for _ in 0..10 {
        if e.run_batch(&c).await.unwrap() == 0 {
            return;
        }
    }
}

async fn job_state(e: &Engine<MemorySource>, sig: &str) -> (String, i32) {
    let j = e
        .database()
        .de_jobs(None, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|j| j.signature == sig)
        .unwrap();
    (j.state, j.attempts)
}

fn sig_info(sig: &str, slot: u64) -> SignatureInfo {
    SignatureInfo {
        signature: TransactionSignature::parse(sig).unwrap(),
        slot,
        block_time: None,
        failed: false,
        err_json: None,
        memo: None,
        confirmation_status: Some(CommitmentLevel::Finalized),
    }
}

fn finalized_at(slot: u64) -> Option<SignatureStatus> {
    Some(SignatureStatus {
        slot,
        confirmations: None,
        failed: false,
        err_json: None,
        confirmation_status: Some(CommitmentLevel::Finalized),
    })
}

async fn wait_until_due(db: &Database, n: usize) {
    for _ in 0..50 {
        if db.de_finality_due(100).await.unwrap().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("finality checks never became due");
}

// ---- tests ----------------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn ingest_persists_once_and_reprocessing_is_idempotent() {
    let src = Arc::new(MemorySource::default());
    let f = fixture("raydium_amm_v4_swap");
    let (sig, slot) = src.add(&f);
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();

    assert_eq!(observe(&e, &sig).await, EnqueueOutcome::Created);
    // An identical repeat is recorded once; another source only adds provenance.
    assert_eq!(
        observe(&e, &sig).await,
        EnqueueOutcome::DuplicateObservation
    );
    let again = e
        .observe(
            &sig,
            "LIVE_STREAM",
            None,
            Some(CommitmentLevel::Confirmed),
            None,
            None,
            3,
        )
        .await
        .unwrap();
    assert_eq!(again, EnqueueOutcome::Existing);
    drain(&e).await;
    assert_eq!(job_state(&e, &sig).await.0, "COMPLETED");

    let d = db.de_transaction(&sig).await.unwrap().unwrap();
    assert_eq!(d.summary.slot, slot);
    assert_eq!(d.summary.current_revision, 1);
    assert_eq!(d.summary.finality, "OBSERVED_CONFIRMED");
    assert_eq!(d.evidence.len(), 1);
    let canonical: Vec<_> = d.swaps.iter().filter(|s| s.canonical).collect();
    assert_eq!(canonical.len(), f.expect.swaps);
    let s = canonical[0];
    assert_eq!(
        s.write.input_amount.as_deref(),
        f.expect.input_amount.map(|v| v.to_string()).as_deref()
    );
    assert_eq!(
        s.write.output_amount.as_deref(),
        f.expect.output_amount.map(|v| v.to_string()).as_deref()
    );

    // Outbox: decoded + one SwapRecorded per swap, delivered exactly once.
    let pending = db.de_outbox_pending("diagnostic_log", 100).await.unwrap();
    let types: Vec<&str> = pending.iter().map(|o| o.event_type.as_str()).collect();
    assert!(types.contains(&"TransactionDecoded"), "{types:?}");
    assert_eq!(
        types.iter().filter(|t| **t == "SwapRecorded").count(),
        f.expect.swaps
    );
    assert_eq!(e.outbox_tick().await.unwrap(), pending.len());
    assert_eq!(e.outbox_tick().await.unwrap(), 0);

    // Reprocessing the same evidence with the same decoder rewrites nothing
    // and emits nothing, and never contacts the source.
    let fetched_before = src.fetch_count(&sig);
    assert_eq!(db.de_requeue_for_reprocess(None, 10).await.unwrap(), 1);
    drain(&e).await;
    assert_eq!(
        src.fetch_count(&sig),
        fetched_before,
        "reprocess uses stored evidence"
    );
    let d2 = db.de_transaction(&sig).await.unwrap().unwrap();
    assert_eq!(d2.summary.current_revision, 1);
    assert_eq!(d2.evidence.len(), 1);
    assert!(
        db.de_outbox_pending("diagnostic_log", 100)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(e.metrics().snapshot().unchanged >= 1);
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn budget_denial_releases_without_consuming_an_attempt() {
    let src = Arc::new(MemorySource::default());
    let (sig, _) = src.add(&fixture("pump_fun_buy"));
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    observe(&e, &sig).await;

    // Admission closed: nothing is claimed at all.
    src.budget_denied.store(true, Ordering::SeqCst);
    assert_eq!(e.run_batch(&CancellationToken::new()).await.unwrap(), 0);
    assert_eq!(job_state(&e, &sig).await, ("PENDING".to_owned(), 0));

    // Denied mid-flight (admission races): released, attempt not consumed.
    let job = e
        .database()
        .de_claim_jobs(e.owner(), 1, Duration::from_secs(30))
        .await
        .unwrap()
        .remove(0);
    e.process_job(&job, &CancellationToken::new())
        .await
        .unwrap();
    let (state, attempts) = job_state(&e, &sig).await;
    assert_eq!(state, "PENDING");
    assert_eq!(attempts, 0);
    assert!(e.database().de_transaction(&sig).await.unwrap().is_none());
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn unknown_transaction_is_retried_and_never_invented() {
    let src = Arc::new(MemorySource::default());
    let sig = fixtures::sig("never-seen:0");
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    observe(&e, &sig).await;
    drain(&e).await;
    let (state, attempts) = job_state(&e, &sig).await;
    assert_eq!(state, "FETCH_RETRY");
    assert_eq!(attempts, 1);
    assert!(e.database().de_transaction(&sig).await.unwrap().is_none());
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn finality_advances_and_unknown_status_is_never_orphaned() {
    let src = Arc::new(MemorySource::default());
    let (a, slot_a) = src.add(&fixture("raydium_amm_v4_swap"));
    let (b, _) = src.add(&fixture("pump_fun_buy"));
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    observe(&e, &a).await;
    observe(&e, &b).await;
    drain(&e).await;
    e.outbox_tick().await.unwrap();

    src.status(
        &a,
        Some(SignatureStatus {
            slot: slot_a,
            confirmations: None,
            failed: false,
            err_json: None,
            confirmation_status: Some(CommitmentLevel::Finalized),
        }),
    );
    src.status(&b, None);
    // Due times are written with the host clock and compared with the
    // database clock; wait past any small offset between the two.
    wait_until_due(db, 2).await;
    let c = CancellationToken::new();
    assert_eq!(e.finality_tick(&c).await.unwrap(), 2);

    let ta = db.de_transaction(&a).await.unwrap().unwrap();
    assert_eq!(ta.summary.finality, "FINALIZED");
    let tb = db.de_transaction(&b).await.unwrap().unwrap();
    assert_eq!(tb.summary.finality, "FINALITY_UNKNOWN");
    assert_ne!(tb.summary.finality, "ORPHANED");
    assert!(tb.issues.iter().any(|i| i.code == "FINALITY_UNKNOWN"));

    // The unknown one is re-queued for a finalized refetch.
    let jb = db
        .de_jobs(None, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|j| j.signature == b)
        .unwrap();
    assert_eq!(jb.state, "PENDING");
    assert_eq!(jb.required_commitment, "finalized");
    assert_eq!(jb.source, "FINALITY");

    let events: Vec<String> = db
        .de_outbox_pending("diagnostic_log", 100)
        .await
        .unwrap()
        .into_iter()
        .map(|o| o.event_type)
        .collect();
    assert!(
        events.contains(&"TransactionFinalized".to_owned()),
        "{events:?}"
    );
    assert!(events.contains(&"FinalityUnknown".to_owned()), "{events:?}");

    // A finalized transaction is never checked again and never regresses.
    src.status(&a, None);
    wait_until_due(db, 1).await;
    assert_eq!(e.finality_tick(&c).await.unwrap(), 1);
    let ta = db.de_transaction(&a).await.unwrap().unwrap();
    assert_eq!(ta.summary.finality, "FINALIZED");
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn backfill_is_bounded_resumable_and_deduplicated() {
    let src = Arc::new(MemorySource::default());
    let owner = fixtures::addr("backfill-owner");
    let s: Vec<String> = (0..3).map(|i| fixtures::sig(&format!("bf:{i}"))).collect();
    let info = |sig: &str, slot: u64| SignatureInfo {
        signature: TransactionSignature::parse(sig).unwrap(),
        slot,
        block_time: None,
        failed: false,
        err_json: None,
        memo: None,
        confirmation_status: Some(CommitmentLevel::Finalized),
    };
    {
        let mut pages = src.pages.lock().unwrap();
        pages.insert(
            None,
            SignaturePage {
                items: vec![info(&s[0], 300), info(&s[1], 200)],
                complete: false,
                next_before: Some(TransactionSignature::parse(&s[1]).unwrap()),
                pages_fetched: 1,
            },
        );
        pages.insert(
            Some(s[1].clone()),
            SignaturePage {
                // s[1] repeated: deduplicated, not queued twice.
                items: vec![info(&s[1], 200), info(&s[2], 100)],
                complete: true,
                next_before: None,
                pages_fetched: 1,
            },
        );
    }
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    let id = Uuid::new_v4();
    db.de_create_backfill(&NewBackfill {
        backfill_id: id,
        target_id: None,
        address: owner,
        commitment: "finalized".into(),
        start_before_signature: None,
        until_signature: None,
        min_slot: None,
        page_size: 2,
        max_pages: 5,
        max_signatures: 100,
        credits_estimated: 50,
    })
    .await
    .unwrap();

    let bf = e
        .backfill_tick(&CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    let bf = db.de_backfill(bf.backfill_id).await.unwrap().unwrap();
    assert_eq!(bf.status, "COMPLETED", "{bf:?}");
    assert!(bf.history_exhausted);
    assert_eq!(bf.pages_fetched, 2);
    assert_eq!(bf.queued_count, 3);
    assert_eq!(bf.oldest_slot_seen, Some(100));
    assert_eq!(bf.newest_slot_seen, Some(300));
    let queued: Vec<String> = db
        .de_jobs(None, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|j| j.signature)
        .collect();
    assert_eq!(queued.len(), 3);
    for sig in &s {
        assert!(queued.contains(sig));
    }
    // A completed backfill is not claimed again.
    assert!(
        e.backfill_tick(&CancellationToken::new())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn every_fixture_persists_within_schema_constraints_and_reads_back() {
    let src = Arc::new(MemorySource::default());
    let catalog = fixtures::catalog();
    let sigs: Vec<(String, &Fixture)> = catalog.iter().map(|f| (src.add(f).0, f)).collect();
    let mut distinct: Vec<&String> = sigs.iter().map(|(s, _)| s).collect();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        catalog.len(),
        "fixture signatures are unique"
    );

    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    db.de_add_target(&solprint_storage::data_engine::NewTarget {
        target_id: Uuid::new_v4(),
        target_type: "WALLET_ADDRESS".into(),
        address: fixtures::addr("user"),
        label: Some("fixture".into()),
        priority: "P3".into(),
        commitment: "confirmed".into(),
        discovery_source: "MANUAL".into(),
    })
    .await
    .unwrap();
    for (sig, _) in &sigs {
        observe(&e, sig).await;
    }
    drain(&e).await;

    for (sig, f) in &sigs {
        let (state, _) = job_state(&e, sig).await;
        assert_eq!(state, "COMPLETED", "{}", f.name);
        let d = db.de_transaction(sig).await.unwrap().unwrap();
        assert_eq!(d.summary.decode_status, f.expect.tx_status, "{}", f.name);
        let canonical = d.swaps.iter().filter(|s| s.canonical).count();
        assert_eq!(canonical, f.expect.swaps, "{}", f.name);
        if let Some(w) = &f.expect.verified_wallet {
            assert!(
                d.swaps
                    .iter()
                    .all(|s| s.write.wallet.as_deref() == Some(w.as_str())),
                "{}",
                f.name
            );
        }
    }

    // Every read model used by the API runs against the populated schema.
    let counts = db.de_counts().await.unwrap();
    assert_eq!(usize::try_from(counts.transactions).unwrap(), catalog.len());
    assert_eq!(counts.targets_total, 1);
    let swaps = db
        .de_swaps(
            &solprint_storage::data_engine::SwapFilter {
                limit: 200,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        swaps.len(),
        catalog.iter().map(|f| f.expect.swaps).sum::<usize>()
    );

    // The verified-trade query excludes every swap without verified
    // classification and attribution, and agrees with the verified count.
    let verified = db
        .de_swaps(
            &solprint_storage::data_engine::SwapFilter {
                verified_only: true,
                limit: 200,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        i64::try_from(verified.len()).unwrap(),
        counts.swaps_verified
    );
    assert!(
        verified.len() < swaps.len(),
        "some fixtures are not verified trades"
    );
    assert!(verified.iter().all(|s| {
        s.canonical
            && s.write.classification_status == "VERIFIED"
            && s.write.attribution.starts_with("VERIFIED_")
            && s.write.wallet.is_some()
    }));
    for excluded in [
        "program_controlled_swap",
        "missing_inner_instructions",
        "amount_mismatch",
    ] {
        let sig = serde_json::from_str::<serde_json::Value>(&fixture(excluded).json).unwrap()
            ["transaction"]["signatures"][0]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            verified.iter().all(|s| s.signature != sig),
            "{excluded} must not be a verified trade"
        );
    }
    assert!(
        !db.de_issues(None, None, false, 200)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!db.de_outbox_recent(50).await.unwrap().is_empty());
    assert_eq!(db.de_targets().await.unwrap().len(), 1);
    assert!(db.de_backfills(10).await.unwrap().is_empty());

    // The outbox drains completely, and retention runs without touching
    // anything recent.
    while e.outbox_tick().await.unwrap() > 0 {}
    assert_eq!(db.de_counts().await.unwrap().outbox_undelivered, 0);
    let r = e.retention().await.unwrap();
    assert_eq!(r.evidence_pruned, 0);
    assert_eq!(r.outbox_deleted, 0);
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn invalid_evidence_is_quarantined_and_slot_mismatch_is_not_orphaned() {
    let src = Arc::new(MemorySource::default());
    let bad = fixtures::sig("garbage:0");
    src.txs
        .lock()
        .unwrap()
        .insert(bad.clone(), ("{\"not\":\"a transaction\"}".into(), 1));
    let (good, slot) = src.add(&fixture("pump_fun_buy"));
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    observe(&e, &bad).await;
    observe(&e, &good).await;
    drain(&e).await;

    assert_eq!(job_state(&e, &bad).await.0, "QUARANTINED");
    assert!(db.de_transaction(&bad).await.unwrap().is_none());
    assert!(
        !db.de_issues(Some(&bad), None, false, 10)
            .await
            .unwrap()
            .is_empty()
    );

    src.status(
        &good,
        Some(SignatureStatus {
            slot: slot + 7,
            confirmations: Some(3),
            failed: false,
            err_json: None,
            confirmation_status: Some(CommitmentLevel::Confirmed),
        }),
    );
    wait_until_due(db, 1).await;
    assert_eq!(e.finality_tick(&CancellationToken::new()).await.unwrap(), 1);
    let t = db.de_transaction(&good).await.unwrap().unwrap();
    assert_eq!(t.summary.finality, "POSSIBLY_ORPHANED");
    assert!(t.issues.iter().any(|i| i.code == "SLOT_MISMATCH"));
    // Swaps stay canonical: an orphan is never inferred from one status.
    assert!(t.swaps.iter().all(|s| s.canonical));
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn backfill_resumes_from_its_checkpoint_after_shutdown_and_budget_pause() {
    let src = Arc::new(MemorySource::default());
    let owner = fixtures::addr("resume-owner");
    let s: Vec<String> = (0..3).map(|i| fixtures::sig(&format!("rs:{i}"))).collect();
    {
        let mut pages = src.pages.lock().unwrap();
        pages.insert(
            None,
            SignaturePage {
                items: vec![sig_info(&s[0], 300), sig_info(&s[1], 200)],
                complete: false,
                next_before: Some(TransactionSignature::parse(&s[1]).unwrap()),
                pages_fetched: 1,
            },
        );
        pages.insert(
            Some(s[1].clone()),
            SignaturePage {
                items: vec![sig_info(&s[2], 100)],
                complete: true,
                next_before: None,
                pages_fetched: 1,
            },
        );
    }
    let (_tdb, a) = engine(Arc::clone(&src)).await;
    let db = a.database().clone();

    // s[1] was already seen live: the backfill must not queue it again.
    let live = a
        .observe(&s[1], "LIVE_STREAM", None, None, Some(200), None, 3)
        .await
        .unwrap();
    assert_eq!(live, EnqueueOutcome::Created);

    let id = Uuid::new_v4();
    db.de_create_backfill(&NewBackfill {
        backfill_id: id,
        target_id: None,
        address: owner,
        commitment: "finalized".into(),
        start_before_signature: None,
        until_signature: None,
        min_slot: None,
        page_size: 2,
        max_pages: 5,
        max_signatures: 100,
        credits_estimated: 50,
    })
    .await
    .unwrap();

    // 1. Shutdown before the first page: paused, nothing fetched.
    let stopped = CancellationToken::new();
    stopped.cancel();
    a.backfill_tick(&stopped).await.unwrap().unwrap();
    let bf = db.de_backfill(id).await.unwrap().unwrap();
    assert_eq!(bf.status, "PAUSED");
    assert_eq!(bf.stop_reason.as_deref(), Some("SHUTDOWN"));
    assert_eq!(bf.pages_fetched, 0);

    // 2. Resumed; the second page is denied by the budget.
    *src.deny_pages_from.lock().unwrap() = Some(1);
    a.backfill_tick(&CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    let bf = db.de_backfill(id).await.unwrap().unwrap();
    assert_eq!(bf.status, "PAUSED_BY_BUDGET", "{bf:?}");
    assert_eq!(bf.pages_fetched, 1);
    assert_eq!(bf.cursor_before.as_deref(), Some(s[1].as_str()));
    assert_eq!(bf.queued_count, 1);
    assert_eq!(bf.duplicate_count, 1);
    assert!(!bf.history_exhausted);

    // 3. A restarted process (new engine, new lease owner) continues from the
    //    stored cursor once the budget admits again; page one is not refetched.
    *src.deny_pages_from.lock().unwrap() = None;
    let calls_before = src.page_calls.load(Ordering::SeqCst);
    let b = Arc::new(Engine::new(db.clone(), Arc::clone(&src), config()));
    b.backfill_tick(&CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(src.page_calls.load(Ordering::SeqCst), calls_before + 1);
    let bf = db.de_backfill(id).await.unwrap().unwrap();
    assert_eq!(bf.status, "COMPLETED", "{bf:?}");
    assert_eq!(bf.pages_fetched, 2);
    assert_eq!(bf.queued_count, 2);
    assert_eq!(bf.duplicate_count, 1);
    assert!(bf.history_exhausted);
    assert_eq!(bf.coverage_state, "ADDRESS_HISTORY_EXHAUSTED");
    let jobs = db.de_jobs(None, 100).await.unwrap();
    assert_eq!(jobs.len(), 3, "one job per signature");
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn crashed_leases_and_undelivered_events_survive_a_restart() {
    let src = Arc::new(MemorySource::default());
    let (sig, _) = src.add(&fixture("raydium_cpmm_swap"));
    let (_tdb, a) = engine(Arc::clone(&src)).await;
    let db = a.database().clone();
    observe(&a, &sig).await;

    // A worker that crashed while holding the lease.
    let crashed = Uuid::new_v4();
    let claimed = db
        .de_claim_jobs(crashed, 10, Duration::from_millis(1))
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(db.de_recover_expired_leases().await.unwrap(), 1);
    let j = db.de_jobs(None, 10).await.unwrap().remove(0);
    assert_eq!(j.state, "FETCH_RETRY");
    assert_eq!(j.last_error_code.as_deref(), Some("LEASE_EXPIRED"));

    // The recovered job is processed exactly once.
    drain(&a).await;
    assert_eq!(job_state(&a, &sig).await.0, "COMPLETED");
    assert_eq!(src.fetch_count(&sig), 1);
    let pending = db.de_outbox_pending("diagnostic_log", 100).await.unwrap();
    assert!(!pending.is_empty());

    // The process "restarts" before delivering: a new engine delivers the
    // same events, each once.
    let b = Arc::new(Engine::new(db.clone(), Arc::clone(&src), config()));
    assert_eq!(b.outbox_tick().await.unwrap(), pending.len());
    assert_eq!(b.outbox_tick().await.unwrap(), 0);

    // A duplicate acknowledgement (or a late failure report) never undoes a
    // delivery.
    let first = pending[0].event_id;
    db.de_outbox_mark("diagnostic_log", first, true, None, Duration::ZERO)
        .await
        .unwrap();
    db.de_outbox_mark(
        "diagnostic_log",
        first,
        false,
        Some("CONSUMER_ERROR"),
        Duration::ZERO,
    )
    .await
    .unwrap();
    assert!(
        db.de_outbox_pending("diagnostic_log", 100)
            .await
            .unwrap()
            .is_empty()
    );

    // Delivery is tracked per consumer: a failed delivery to another
    // consumer is offered again.
    db.de_outbox_mark(
        "other",
        first,
        false,
        Some("CONSUMER_ERROR"),
        Duration::ZERO,
    )
    .await
    .unwrap();
    let retry = db.de_outbox_pending("other", 100).await.unwrap();
    assert!(retry.iter().any(|o| o.event_id == first));
    assert_eq!(retry.len(), pending.len());
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn out_of_order_and_late_observations_never_regress_or_refetch() {
    let src = Arc::new(MemorySource::default());
    let (sig, slot) = src.add(&fixture("orca_whirlpool_swap"));
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();

    let low = e
        .observe(&sig, "LIVE_STREAM", None, None, Some(slot), Some(false), 4)
        .await
        .unwrap();
    assert_eq!(low, EnqueueOutcome::Created);
    let high = e
        .observe(&sig, "MANUAL", None, None, None, None, 2)
        .await
        .unwrap();
    assert_eq!(high, EnqueueOutcome::Existing);
    let lower = e
        .observe(&sig, "STREAM_RECOVERY", None, None, Some(slot), None, 5)
        .await
        .unwrap();
    assert_eq!(lower, EnqueueOutcome::Existing);
    let j = db.de_jobs(None, 10).await.unwrap().remove(0);
    assert_eq!(j.priority, 2, "priority only rises");

    drain(&e).await;
    assert_eq!(
        src.fetch_count(&sig),
        1,
        "three observations, one retrieval"
    );

    src.status(&sig, finalized_at(slot));
    wait_until_due(db, 1).await;
    e.finality_tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(
        db.de_transaction(&sig)
            .await
            .unwrap()
            .unwrap()
            .summary
            .finality,
        "FINALIZED"
    );

    // A late observation of the same transaction adds provenance only.
    let late = e
        .observe(&sig, "BACKFILL", None, None, Some(slot), Some(false), 3)
        .await
        .unwrap();
    assert_eq!(late, EnqueueOutcome::Existing);
    drain(&e).await;
    assert_eq!(src.fetch_count(&sig), 1);
    let t = db.de_transaction(&sig).await.unwrap().unwrap();
    assert_eq!(t.summary.finality, "FINALIZED");
    assert_eq!(t.summary.current_revision, 1);
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn stream_gaps_stay_visible_and_live_logs_become_jobs() {
    let src = Arc::new(MemorySource::default());
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    let target = db
        .de_add_target(&solprint_storage::data_engine::NewTarget {
            target_id: Uuid::new_v4(),
            target_type: "WALLET_ADDRESS".into(),
            address: fixtures::addr("stream-target"),
            label: None,
            priority: "P3".into(),
            commitment: "confirmed".into(),
            discovery_source: "MANUAL".into(),
        })
        .await
        .unwrap();
    let tid = Some(target.target_id);
    let ev = |kind: StreamEventKind| StreamEvent {
        subscription: None,
        generation: 2,
        received_at: UtcTimestamp::now(),
        commitment: Some(CommitmentLevel::Confirmed),
        context_slot: Some(500),
        provenance: Provenance::Direct,
        kind,
    };
    let live = fixtures::sig("stream:live");
    e.record_stream_event(
        &ev(StreamEventKind::Logs {
            signature: TransactionSignature::parse(&live).unwrap(),
            failed: false,
            logs: Vec::new(),
        }),
        tid,
        3,
    )
    .await
    .unwrap();
    e.record_stream_event(
        &ev(StreamEventKind::Gap {
            reason: "RECONNECT",
            from_slot: Some(400),
            to_slot: Some(500),
        }),
        tid,
        3,
    )
    .await
    .unwrap();
    e.record_stream_event(
        &ev(StreamEventKind::ContinuityUnknown {
            reason: "BACKPRESSURE",
        }),
        tid,
        3,
    )
    .await
    .unwrap();

    let jobs = db.de_jobs(None, 10).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].signature, live);
    assert_eq!(jobs[0].source, "LIVE_STREAM");

    let issues = db.de_issues(None, None, false, 50).await.unwrap();
    let gap = issues.iter().find(|i| i.code == "STREAM_GAP").unwrap();
    assert_eq!(gap.target_id, tid);
    assert!(gap.resolved_at.is_none());
    assert!(
        gap.signature.is_none(),
        "a gap is not attributed to a transaction"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.code == "CONTINUITY_UNKNOWN" && i.resolved_at.is_none())
    );
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn corrected_evidence_retracts_swaps_under_a_new_revision() {
    let src = Arc::new(MemorySource::default());
    let f = fixture("raydium_amm_v4_swap");
    let (sig, slot) = src.add(&f);
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    observe(&e, &sig).await;
    drain(&e).await;
    while e.outbox_tick().await.unwrap() > 0 {}
    let before = db.de_transaction(&sig).await.unwrap().unwrap();
    assert_eq!(before.swaps.iter().filter(|s| s.canonical).count(), 1);
    let swap_id = before.swaps[0].write.swap_id.clone();

    // The authoritative (finalized) evidence says the transaction failed.
    let mut v: serde_json::Value = serde_json::from_str(&f.json).unwrap();
    v["meta"]["err"] = serde_json::json!({ "InstructionError": [0, { "Custom": 1 }] });
    src.txs
        .lock()
        .unwrap()
        .insert(sig.clone(), (v.to_string(), slot));
    src.status(&sig, None);
    wait_until_due(db, 1).await;
    e.finality_tick(&CancellationToken::new()).await.unwrap();
    drain(&e).await;

    let after = db.de_transaction(&sig).await.unwrap().unwrap();
    assert_eq!(after.summary.current_revision, 2);
    assert_eq!(after.summary.finality, "FINALIZED");
    assert_eq!(after.evidence.len(), 2, "the superseded evidence is kept");
    assert!(
        after.swaps.iter().all(|s| !s.canonical),
        "{:?}",
        after.swaps
    );
    assert!(
        after
            .swaps
            .iter()
            .all(|s| s.retraction_reason.as_deref() == Some("EVIDENCE_REVISION"))
    );
    let events: Vec<_> = db
        .de_outbox_pending("diagnostic_log", 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|o| o.event_type == "SwapRetracted")
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].aggregate_id, swap_id);
    assert_eq!(events[0].aggregate_revision, 2);
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn extreme_amounts_round_trip_exactly_through_the_schema() {
    use solprint_data_engine::fixtures::{TxBuilder, spl_transfer};
    use solprint_data_engine::programs::SPL_TOKEN_PROGRAM;

    let src = Arc::new(MemorySource::default());
    let user = fixtures::addr("user");
    let friend = fixtures::addr("friend");
    let mint = fixtures::addr("mint:max");
    let (from, to) = (fixtures::addr("user:max"), fixtures::addr("friend:max"));
    let json = TxBuilder::new("extremes", &user)
        .ix(
            SPL_TOKEN_PROGRAM,
            &[&from, &to, &user],
            spl_transfer(u64::MAX),
        )
        .token(&from, &mint, &user, 0, Some(u64::MAX), Some(0))
        .token(&to, &mint, &friend, 0, Some(0), Some(u64::MAX))
        .lamports(&user, u64::MAX, u64::MAX - 5_000)
        .build();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let sig = v["transaction"]["signatures"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let slot = v["slot"].as_u64().unwrap();
    src.txs.lock().unwrap().insert(sig.clone(), (json, slot));
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    observe(&e, &sig).await;
    drain(&e).await;
    assert_eq!(job_state(&e, &sig).await.0, "COMPLETED");

    let max = u64::MAX.to_string();
    let d = db.de_transaction(&sig).await.unwrap().unwrap();
    let delta = |acct: &str| {
        d.balances
            .iter()
            .find(|b| b.account == acct && b.asset != "SOL")
            .and_then(|b| b.delta.clone())
    };
    // NUMERIC(21,0) holds both signs of a full u64 swing; nothing is rounded.
    assert_eq!(delta(&from), Some(format!("-{max}")));
    assert_eq!(delta(&to), Some(max.clone()));
    let native = d
        .balances
        .iter()
        .find(|b| b.account == user && b.asset == "SOL")
        .unwrap();
    assert_eq!(native.pre_amount.as_deref(), Some(max.as_str()));
    assert_eq!(native.delta.as_deref(), Some("-5000"));
    assert!(
        d.activities
            .iter()
            .any(|a| a.write.amount.as_deref() == Some(max.as_str())),
        "the transfer amount is stored as an exact u64"
    );
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn a_full_queue_refuses_intake_and_pauses_backfills_visibly() {
    let src = Arc::new(MemorySource::default());
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    let mut cfg = config();
    cfg.queue_capacity = 1;
    let e = Arc::new(Engine::new(db.clone(), Arc::clone(&src), cfg));

    let first = fixtures::sig("full:0");
    observe(&e, &first).await;
    // The second observation is refused, not silently dropped.
    let refused = e
        .observe(
            &fixtures::sig("full:1"),
            "MANUAL",
            None,
            None,
            None,
            None,
            3,
        )
        .await;
    assert!(refused.is_err());
    assert_eq!(db.de_jobs(None, 10).await.unwrap().len(), 1);
    assert!(
        db.de_issues(None, None, false, 10)
            .await
            .unwrap()
            .iter()
            .any(|i| i.code == "QUEUE_SATURATED")
    );

    let id = Uuid::new_v4();
    db.de_create_backfill(&NewBackfill {
        backfill_id: id,
        target_id: None,
        address: fixtures::addr("full-owner"),
        commitment: "finalized".into(),
        start_before_signature: None,
        until_signature: None,
        min_slot: None,
        page_size: 10,
        max_pages: 5,
        max_signatures: 100,
        credits_estimated: 105,
    })
    .await
    .unwrap();
    e.backfill_tick(&CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    let bf = db.de_backfill(id).await.unwrap().unwrap();
    assert_eq!(bf.status, "PAUSED_BY_QUEUE");
    assert_eq!(
        bf.pages_fetched, 0,
        "no credits are spent while the queue is full"
    );
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn a_page_limited_backfill_reports_partial_history() {
    let src = Arc::new(MemorySource::default());
    let s0 = fixtures::sig("partial:0");
    src.pages.lock().unwrap().insert(
        None,
        SignaturePage {
            items: vec![sig_info(&s0, 500)],
            complete: false,
            next_before: Some(TransactionSignature::parse(&s0).unwrap()),
            pages_fetched: 1,
        },
    );
    let (_tdb, e) = engine(Arc::clone(&src)).await;
    let db = e.database();
    let id = Uuid::new_v4();
    db.de_create_backfill(&NewBackfill {
        backfill_id: id,
        target_id: None,
        address: fixtures::addr("partial-owner"),
        commitment: "finalized".into(),
        start_before_signature: None,
        until_signature: None,
        min_slot: None,
        page_size: 1,
        max_pages: 1,
        max_signatures: 100,
        credits_estimated: 2,
    })
    .await
    .unwrap();
    e.backfill_tick(&CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    let bf = db.de_backfill(id).await.unwrap().unwrap();
    assert_eq!(bf.status, "COMPLETED");
    assert_eq!(bf.coverage_state, "PARTIAL_HISTORY");
    assert!(!bf.history_exhausted, "a limit is not the end of history");
    let issues = db.de_issues(None, None, false, 10).await.unwrap();
    let gap = issues
        .iter()
        .find(|i| i.code == "BACKFILL_INCOMPLETE")
        .unwrap();
    assert_eq!(gap.backfill_id, Some(id));
    assert!(gap.resolved_at.is_none());
}
