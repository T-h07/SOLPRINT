//! Micro-benchmarks of GATEWAY's own overhead (no provider is contacted;
//! the RPC "provider" is a loopback mock). Run with
//! `cargo bench -p solprint-gateway`. Results are machine-specific and are
//! recorded in `docs/architecture/GATEWAY_CAPACITY_MODEL.md` only as
//! measured on the stated machine.

// Floating point is used only to print a throughput figure.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::float_arithmetic
)]

#[path = "../tests/support/mod.rs"]
mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use solprint_gateway::budget::{BudgetPolicy, CreditBudget};
use solprint_gateway::metrics::{Metrics, Outcome};
use solprint_gateway::{CachePolicy, Gateway, MemoryStore, Priority, ProviderId, RequestContext};
use time::OffsetDateTime;

fn report(name: &str, iterations: u32, elapsed: Duration) {
    let per = elapsed / iterations;
    let rate = f64::from(iterations) / elapsed.as_secs_f64();
    println!("{name:<44} {iterations:>8} iters  {per:>10.2?}/op  {rate:>12.0} op/s");
}

fn bench_budget() {
    let policy = BudgetPolicy {
        monthly_credits: u64::MAX / 4,
        reserve_percent: 20,
        cycle_start_day: 1,
    };
    let now = OffsetDateTime::now_utc();
    let b = CreditBudget::new(ProviderId::Helius, policy, now);
    b.load(
        solprint_gateway::budget::BillingCycle::containing(now, 1),
        0,
    );
    let n = 200_000;
    let t = Instant::now();
    for _ in 0..n {
        let r = b.try_reserve(Priority::P1, 1, now).unwrap();
        std::hint::black_box(&r);
    }
    report("budget.try_reserve", n, t.elapsed());
}

fn bench_metrics() {
    let m = Metrics::default();
    let n = 200_000;
    let t = Instant::now();
    for i in 0..n {
        m.record(
            ProviderId::Helius,
            "getSlot",
            Outcome::Success,
            Duration::from_micros(u64::from(i % 5_000)),
        );
    }
    report("metrics.record", n, t.elapsed());
}

async fn bench_cached_reads() {
    let mock = support::MockServer::start(|r| support::rpc_result(r, &json!(1))).await;
    let mut s = support::settings();
    s.helius.rpc_url = mock.url.clone();
    let g = Gateway::new(s, Arc::new(MemoryStore::default())).unwrap();
    g.load_ledger().await;
    let ctx = RequestContext::new("bench", Priority::P1);
    g.get_slot(&ctx).await.unwrap();
    let n = 50_000;
    let t = Instant::now();
    for _ in 0..n {
        let c = RequestContext::new("bench", Priority::P1).with_cache(CachePolicy::PreferCache);
        std::hint::black_box(g.get_slot(&c).await.unwrap());
    }
    report("gateway.get_slot (fresh cache hit)", n, t.elapsed());

    let n = 20;
    let t = Instant::now();
    for _ in 0..n {
        let c = RequestContext::new("bench", Priority::P1).with_cache(CachePolicy::Refresh);
        std::hint::black_box(g.get_slot(&c).await.unwrap());
    }
    report(
        "gateway.get_slot (loopback miss, rate-limited)",
        n,
        t.elapsed(),
    );

    let n = 20_000;
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(g.snapshot());
    }
    report("gateway.snapshot", n, t.elapsed());
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    println!("GATEWAY overhead benchmarks (machine-specific)");
    bench_budget();
    bench_metrics();
    rt.block_on(bench_cached_reads());
}
