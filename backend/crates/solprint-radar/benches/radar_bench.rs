//! Micro-benchmarks of RADAR's CPU-bound policy work: FIFO screening of a
//! wallet's trades, activity windows, priority scoring, market selection and
//! monitoring rotation. No provider and no database are contacted; inputs
//! are synthetic and sized like the configured limits (the per-wallet swap
//! limit, a full market universe, a large monitoring candidate set). Run with
//! `cargo bench -p solprint-radar`. Results are machine-specific and are
//! recorded in `docs/architecture/RADAR_CAPACITY_MODEL.md` only as measured
//! on the stated machine.

// Floating point is used only to print a throughput figure.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::float_arithmetic
)]

use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use solprint_radar::market::{self, MarketCandidate, PoolView, Thresholds};
use solprint_radar::model::{Bucket, Denomination, WSOL_MINT};
use solprint_radar::monitoring::{self, Assignment, Candidate, RotationPolicy};
use solprint_radar::scoring::{self, Coverage, EvidenceSummary, PriorityInputs};
use solprint_radar::screening::{self, PreBalances, Trade};
use time::OffsetDateTime;

fn report(name: &str, iterations: u32, elapsed: Duration) {
    let per = elapsed / iterations;
    let rate = f64::from(iterations) / elapsed.as_secs_f64();
    println!("{name:<44} {iterations:>8} iters  {per:>10.2?}/op  {rate:>12.0} op/s");
}

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()
}

fn mint(i: u64) -> String {
    format!("Mint{i:040}")
}

/// `swaps` alternating buys and sells over `tokens` tokens, all SOL-quoted.
fn trades(swaps: u64, tokens: u64) -> (Vec<Trade>, PreBalances) {
    let mut out = Vec::new();
    let mut pre = PreBalances::new();
    let mut held = vec![0_u64; usize::try_from(tokens).unwrap()];
    for slot in 0..swaps {
        let t = slot % tokens;
        let idx = usize::try_from(t).unwrap();
        let buy = (slot / tokens).is_multiple_of(2);
        let sig = format!("sig{slot}");
        pre.insert((sig.clone(), mint(t)), Some(held[idx]));
        let (input_mint, input_kind, input_amount, output_mint, output_kind, output_amount) = if buy
        {
            held[idx] += 1_000;
            (
                WSOL_MINT.to_owned(),
                "NATIVE_SOL",
                10_000_000,
                mint(t),
                "SPL_TOKEN",
                1_000,
            )
        } else {
            held[idx] -= 1_000;
            (
                mint(t),
                "SPL_TOKEN",
                1_000,
                WSOL_MINT.to_owned(),
                "NATIVE_SOL",
                11_000_000,
            )
        };
        out.push(Trade {
            swap_id: format!("swap:{sig}:0"),
            signature: sig,
            slot,
            order: (slot, 0, "0".into()),
            block_time: Some(now() - time::Duration::minutes(i64::try_from(swaps - slot).unwrap())),
            input_mint: Some(input_mint),
            input_kind: input_kind.into(),
            input_amount: Some(input_amount),
            output_mint: Some(output_mint),
            output_kind: output_kind.into(),
            output_amount: Some(output_amount),
            wallet_paid_fee: true,
            fee_lamports: 5_000,
            finalized: true,
        });
    }
    (out, pre)
}

fn bench_screening() {
    for (swaps, tokens, rounds) in [(50, 5, 2_000), (500, 40, 200), (2_000, 100, 50)] {
        let (ts, pre) = trades(swaps, tokens);
        let t = Instant::now();
        for _ in 0..rounds {
            std::hint::black_box(screening::screen(&ts, &pre));
        }
        report(
            &format!("screen (FIFO) {swaps} swaps / {tokens} tokens"),
            rounds,
            t.elapsed(),
        );
    }
    let (ts, _) = trades(2_000, 100);
    let rounds = 200;
    let t = Instant::now();
    for _ in 0..rounds {
        std::hint::black_box(screening::activity(&ts, now()));
    }
    report("activity windows, 2000 swaps", rounds, t.elapsed());
}

fn bench_priority() {
    let inputs = PriorityInputs {
        swaps_30d: 37,
        days_since_last_trade: Some(2),
        half_life_days: 7,
        coverage: Coverage {
            signatures_discovered: 498,
            transactions_retrieved: 496,
            transactions_unavailable: 2,
            transactions_verified: 412,
            transactions_unsupported: 84,
        },
        evidence: EvidenceSummary {
            total: 412,
            valid: 411,
            valid_finalized: 409,
            valid_quote_denominated: 380,
            invalid_attribution: 0,
            distinct_tokens: 38,
        },
        screening_coverage_bps: Some(9_561),
        primary: Some((Denomination::Sol, 171, Some(Decimal::new(1_170_717, 6)))),
    };
    let rounds = 100_000;
    let t = Instant::now();
    for _ in 0..rounds {
        std::hint::black_box(scoring::priority(std::hint::black_box(&inputs)));
    }
    report("priority score", rounds, t.elapsed());
}

fn universe(n: u64) -> Vec<MarketCandidate> {
    (0..n)
        .map(|i| MarketCandidate {
            mint: mint(i),
            bucket: Bucket::ALL[usize::try_from(i % 5).unwrap()],
            excluded_class: (i % 17 == 0).then(|| "STABLECOIN".to_owned()),
            promotional_only: i % 23 == 0,
            pool: (i % 11 != 0).then(|| PoolView {
                pool: format!("Pool{i:040}"),
                supported: i % 7 != 0,
                liquidity_usd: Some(Decimal::from(2_000 + i * 997 % 900_000)),
                volume_24h_usd: Some(Decimal::from(5_000 + i * 7_919 % 5_000_000)),
                txns_1h: Some(u32::try_from(i * 31 % 2_000).unwrap()),
                observed_at: now() - time::Duration::seconds(i64::try_from(i % 600).unwrap()),
                last_sampled_at: (i % 3 == 0).then(|| now() - time::Duration::hours(2)),
            }),
            samples_completed: u32::try_from(i % 4).unwrap(),
            wallets_found: u32::try_from(i % 60).unwrap(),
            local_swaps: u32::try_from(i % 90).unwrap(),
        })
        .collect()
}

fn bench_selection() {
    let thresholds = Thresholds {
        min_liquidity_usd: Decimal::from(5_000),
        min_volume_24h_usd: Decimal::from(10_000),
        min_txns_1h: 20,
        max_observation_age: Duration::from_secs(300),
        resample_after: Duration::from_secs(3_600),
    };
    for (n, rounds) in [(100, 2_000), (1_000, 200), (5_000, 40)] {
        let markets = universe(n);
        let t = Instant::now();
        for seed in 0..rounds {
            std::hint::black_box(market::select(
                &markets,
                5,
                [3_500, 2_500, 2_000, 1_000, 1_000],
                &thresholds,
                i64::from(seed),
                now(),
            ));
        }
        report(
            &format!("market selection, {n} markets"),
            rounds,
            t.elapsed(),
        );
    }
}

fn bench_rotation() {
    let policy = RotationPolicy {
        capacity: 10,
        min_duration: Duration::from_secs(3_600),
        cooldown: Duration::from_secs(6 * 3_600),
        margin: Decimal::from(5),
    };
    let current: Vec<Assignment> = (0..10_u32)
        .map(|i| Assignment {
            address: format!("Wallet{i:038}"),
            priority: Decimal::from(50 + i),
            started_at: now() - time::Duration::hours(3),
        })
        .collect();
    for (n, rounds) in [(100_u32, 5_000), (5_000, 100)] {
        let candidates: Vec<Candidate> = (0..n)
            .map(|i| Candidate {
                address: format!("Wallet{i:038}"),
                priority: Decimal::from(i % 100),
                last_ended_at: None,
                quiet: false,
            })
            .collect();
        let t = Instant::now();
        for _ in 0..rounds {
            std::hint::black_box(monitoring::rotate(&current, &candidates, &policy, now()));
        }
        report(
            &format!("monitoring rotation, {n} candidates"),
            rounds,
            t.elapsed(),
        );
    }
}

fn main() {
    println!("RADAR CPU micro-benchmarks (synthetic inputs, no I/O)");
    bench_screening();
    bench_priority();
    bench_selection();
    bench_rotation();
}
