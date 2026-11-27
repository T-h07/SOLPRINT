//! Micro-benchmarks of the DATA ENGINE's CPU-bound work: decoding stored
//! evidence and building the persistence write set. No provider and no
//! database are contacted; inputs are the synthetic fixture catalogue. Run
//! with `cargo bench -p solprint-data-engine`. Results are machine-specific
//! and are recorded in `docs/architecture/DATA_ENGINE_CAPACITY_MODEL.md` only
//! as measured on the stated machine.

// Floating point is used only to print a throughput figure.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::float_arithmetic
)]

use std::time::{Duration, Instant};

use solprint_core::UtcTimestamp;
use solprint_data_engine::fixtures::{self, Fixture};
use solprint_data_engine::mapping::{self, EvidenceMeta};
use solprint_data_engine::{Registry, decode};
use solprint_storage::data_engine::StoredTxState;
use uuid::Uuid;

fn report(name: &str, iterations: u32, elapsed: Duration) {
    let per = elapsed / iterations;
    let rate = f64::from(iterations) / elapsed.as_secs_f64();
    println!("{name:<44} {iterations:>8} iters  {per:>10.2?}/op  {rate:>12.0} op/s");
}

fn fresh_state() -> StoredTxState {
    StoredTxState {
        exists: false,
        current_revision: 0,
        finality: None,
        decoder_version: None,
        content_sha256: None,
        canonical_swap_ids: Vec::new(),
        first_observed_at: None,
    }
}

fn bench_decode_each(catalog: &[Fixture], registry: &Registry) {
    let n = 2_000;
    for f in catalog {
        let t = Instant::now();
        for _ in 0..n {
            let d = decode(&f.json, registry);
            std::hint::black_box(&d);
        }
        report(&format!("decode {}", f.name), n, t.elapsed());
    }
}

fn bench_catalog_mix(catalog: &[Fixture], registry: &Registry) {
    let rounds = 500;
    let total = rounds * u32::try_from(catalog.len()).unwrap();
    let t = Instant::now();
    for _ in 0..rounds {
        for f in catalog {
            let d = decode(&f.json, registry);
            std::hint::black_box(&d);
        }
    }
    report("decode (catalogue mix)", total, t.elapsed());
}

fn bench_write_set(catalog: &[Fixture], registry: &Registry) {
    let decoded: Vec<_> = catalog
        .iter()
        .filter_map(|f| decode(&f.json, registry).ok().map(|d| (d, f.json.clone())))
        .collect();
    let rounds = 500;
    let total = rounds * u32::try_from(decoded.len()).unwrap();
    let state = fresh_state();
    let t = Instant::now();
    for _ in 0..rounds {
        for (d, raw) in &decoded {
            let meta = EvidenceMeta {
                commitment: "confirmed",
                provider: "FIXTURE",
                fetched_at: UtcTimestamp::now(),
                raw_json: raw.clone(),
                first_observed_at: UtcTimestamp::now(),
                next_finality_check_at: None,
                correlation_id: Uuid::nil(),
            };
            let w = mapping::transaction_write(d, &meta, &state);
            std::hint::black_box(&w);
        }
    }
    report("write set incl. sha256 (catalogue mix)", total, t.elapsed());
}

fn main() {
    let catalog = fixtures::catalog();
    let registry = Registry::standard();
    println!(
        "DATA ENGINE micro-benchmarks ({} synthetic fixtures)",
        catalog.len()
    );
    bench_decode_each(&catalog, &registry);
    bench_catalog_mix(&catalog, &registry);
    bench_write_set(&catalog, &registry);
}
