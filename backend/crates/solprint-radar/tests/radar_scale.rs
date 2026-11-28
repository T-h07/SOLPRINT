//! Registry scale benchmark against a disposable PostgreSQL database.
//!
//! Registers N synthetic wallets, persists an evaluation for each (lifecycle,
//! priority, a transition and a snapshot, as the engine does), repeats the
//! registration to measure duplicate handling, then pages the registry the
//! way the API does and runs the scheduler and status queries. Prints
//! throughput and p50/p95 latency per size. No provider is contacted.
//!
//! Ignored, and a no-op unless `SOLPRINT_RADAR_SCALE=1`, so routine
//! `scripts/test.ps1` runs stay fast. Needs `SOLPRINT_TEST_DATABASE_URL`.
//! Run for figures with
//! `SOLPRINT_RADAR_SCALE=1 cargo test -p solprint-radar --release --test radar_scale -- --ignored --nocapture`.
//! Results are machine-specific; see docs/architecture/RADAR_CAPACITY_MODEL.md.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::float_arithmetic,
    clippy::cast_precision_loss
)]

use std::time::{Duration, Instant};

use solprint_storage::Database;
use solprint_storage::radar::{EvaluationWrite, TransitionWrite, WalletFilter, WalletSort};
use solprint_storage::test_support::TestDatabase;
use uuid::Uuid;

const SIZES: [usize; 4] = [100, 500, 1_000, 10_000];
const LIFECYCLES: [&str; 5] = [
    "INSUFFICIENT_EVIDENCE",
    "DORMANT",
    "PRELIMINARY_QUALIFIED",
    "ARCHIVED",
    "REJECTED",
];

fn address(i: usize) -> String {
    // 44 base58 characters, unique per index (index in base58, padded).
    const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut out = String::from("Sca1e");
    let mut x = i;
    for _ in 0..39 {
        out.push(char::from(B58[x % 58]));
        x /= 58;
    }
    out
}

fn pct(sorted: &[Duration], percent: usize) -> Duration {
    // Nearest rank on the sorted samples.
    sorted[((sorted.len() - 1) * percent + 50) / 100]
}

fn line(name: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let total: Duration = samples.iter().sum();
    let rate = samples.len() as f64 / total.as_secs_f64();
    println!(
        "  {name:<38} n={:>6}  p50 {:>9.2?}  p95 {:>9.2?}  {:>9.0} op/s",
        samples.len(),
        pct(&samples, 50),
        pct(&samples, 95),
        rate
    );
}

fn evaluation(address: &str, i: usize, revision: i32) -> EvaluationWrite {
    let lifecycle = LIFECYCLES[i % LIFECYCLES.len()];
    EvaluationWrite {
        address: address.to_owned(),
        expected_revision: revision,
        lifecycle: lifecycle.to_owned(),
        eligibility: "ELIGIBLE_CANDIDATE".into(),
        eligibility_reasons: vec!["FINALIZED_VERIFIED_SWAP".into()],
        qualification: Some("INSUFFICIENT_ACTIVITY".into()),
        qualification_reasons: vec!["FEW_VERIFIED_SWAPS_30D".into()],
        priority_score: format!("{}.{:04}", i % 100, (i * 7_919) % 10_000),
        policy_version: "radar-priority-1".into(),
        history_stage: "A".into(),
        history_coverage_state: "REQUESTED_SCOPE_COMPLETE".into(),
        history_coverage_start: None,
        history_coverage_end: None,
        last_verified_trade_at: None,
        next_investigation_at: None,
        valid_evidence_count: 3,
        verified_swap_count: 3,
        distinct_token_count: 1,
        investigated: true,
        transitions: vec![TransitionWrite {
            from_state: Some("DISCOVERED".into()),
            to_state: lifecycle.to_owned(),
            trigger: "STAGE_A_COMPLETE".into(),
            reasons: vec!["SCALE_BENCHMARK".into()],
            metrics_json: "{}".into(),
        }],
        input_hash: format!("{i:064x}"),
        metrics_json: "{}".into(),
        events: Vec::new(),
    }
}

async fn run(db: &Database, n: usize) {
    println!("registry size {n}");
    let mut register = Vec::with_capacity(n);
    for i in 0..n {
        let t = Instant::now();
        assert!(
            db.radar_register_operator_wallet(Uuid::new_v4(), &address(i))
                .await
                .unwrap()
        );
        register.push(t.elapsed());
    }
    line("register wallet (+ transition)", register);

    let mut dup = Vec::with_capacity(n);
    for i in 0..n {
        let t = Instant::now();
        // Duplicate discovery: no row, no transition, no event.
        assert!(
            !db.radar_register_operator_wallet(Uuid::new_v4(), &address(i))
                .await
                .unwrap()
        );
        dup.push(t.elapsed());
    }
    line("duplicate registration (no-op)", dup);

    let mut eval = Vec::with_capacity(n);
    for i in 0..n {
        let a = address(i);
        let rev = db.radar_wallet(&a).await.unwrap().unwrap().revision;
        let t = Instant::now();
        assert!(
            db.radar_save_evaluation(&evaluation(&a, i, rev))
                .await
                .unwrap()
        );
        eval.push(t.elapsed());
    }
    line("save evaluation (+ snapshot)", eval);

    // Page the whole registry by priority, 50 per page, as the API does.
    let mut pages = Vec::new();
    let mut after = None;
    let mut seen = 0;
    loop {
        let f = WalletFilter {
            sort: WalletSort::Priority,
            after: after.clone(),
            limit: 50,
            ..WalletFilter::default()
        };
        let t = Instant::now();
        let rows = db.radar_wallets(&f).await.unwrap();
        pages.push(t.elapsed());
        seen += rows.len();
        match rows.last() {
            Some(w) if rows.len() == 50 => {
                after = Some((
                    w.priority_score.clone().unwrap_or_else(|| "-1".into()),
                    w.address.clone(),
                ));
            }
            _ => break,
        }
    }
    assert_eq!(seen, n, "keyset pagination must visit every wallet once");
    line("registry page (priority, 50 rows)", pages);

    let mut filtered = Vec::new();
    for l in LIFECYCLES {
        let f = WalletFilter {
            lifecycle: Some(l.to_owned()),
            sort: WalletSort::Priority,
            limit: 50,
            ..WalletFilter::default()
        };
        let t = Instant::now();
        db.radar_wallets(&f).await.unwrap();
        filtered.push(t.elapsed());
    }
    line("filtered page (lifecycle, 50 rows)", filtered);

    let mut sched = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        db.radar_wallets_to_evaluate(Duration::from_secs(900), 100)
            .await
            .unwrap();
        sched.push(t.elapsed());
    }
    line("wallets due for evaluation (100)", sched);

    let mut counts = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        db.radar_counts().await.unwrap();
        counts.push(t.elapsed());
    }
    line("status counts", counts);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs the disposable PostgreSQL server (scripts/test.ps1)"]
async fn registry_scale() {
    if std::env::var("SOLPRINT_RADAR_SCALE").as_deref() != Ok("1") {
        println!("registry_scale skipped: set SOLPRINT_RADAR_SCALE=1 to run it");
        return;
    }
    for n in SIZES {
        let tdb = TestDatabase::create().await;
        let db = Database::connect(&tdb.config()).await.unwrap();
        db.migrate().await.unwrap();
        run(&db, n).await;
        db.close().await;
        tdb.cleanup().await;
    }
}
