# RADAR capacity model

Measured local cost, provider-credit arithmetic and the limits that bound RADAR. Only the
benchmark figures in the two "Measured" sections were measured. Everything else is
derived from configuration defaults and the documented provider costs, and is labelled
**derived**. No live mainnet discovery run has been measured (P03-T98 is NOT RUN).

## Measured: CPU policy cost

`cargo bench -p solprint-radar` (optimized build, one core). Inputs are synthetic: SOL-quoted
round trips across N tokens, a generated market universe and a generated monitoring
candidate set. No database or provider is involved.

Machine: Intel Core i5-1245U (12th Gen, laptop), 32 GB RAM, Windows 11 Pro, rustc 1.99.1.
Measured 2026-11-28.

| Benchmark | Per operation | Throughput (one core) |
|---|---|---|
| FIFO screening, 50 swaps / 5 tokens | 28.6 µs | ~34,900 wallets/s |
| FIFO screening, 500 swaps / 40 tokens | 433 µs | ~2,300 wallets/s |
| FIFO screening, 2,000 swaps / 100 tokens | 1.85 ms | ~540 wallets/s |
| activity windows, 2,000 swaps | 985 µs | ~1,000 wallets/s |
| priority score (all five components, `Decimal`) | 7.9 µs | ~126,000/s |
| market selection, 100 markets | 171 µs | ~5,800 cycles/s |
| market selection, 1,000 markets | 3.6 ms | ~275 cycles/s |
| market selection, 5,000 markets | 53 ms | ~19 cycles/s |
| monitoring rotation, 100 candidates | 8.1 µs | ~124,000/s |
| monitoring rotation, 5,000 candidates | 730 µs | ~1,370/s |

Market selection grows faster than linearly. At the configured universe ceiling
(`RADAR_MAX_MARKET_TARGETS` = 100, at most 1,000) one selection costs at most a few
milliseconds per cycle, and a cycle runs every 300 s by default.

## Measured: registry persistence

`SOLPRINT_RADAR_SCALE=1 cargo test -p solprint-radar --release --test radar_scale -- --ignored --nocapture`
against the disposable PostgreSQL 18.7 container from `scripts/test.ps1` (tmpfs, same
machine, Docker Desktop). One sequential client, so these are per-statement latencies, not
concurrent throughput. Each size uses a fresh database.

| Operation | 100 wallets p50 / p95 | 1,000 wallets p50 / p95 | 10,000 wallets p50 / p95 |
|---|---|---|---|
| register a wallet (row + transition, one transaction) | 3.4 / 5.7 ms | 3.0 / 4.6 ms | 2.9 / 4.2 ms |
| duplicate registration (no row, no transition) | 2.1 / 3.3 ms | 2.1 / 2.9 ms | 2.1 / 3.1 ms |
| save an evaluation (wallet, transition, snapshot; revision-guarded) | 4.0 / 5.1 ms | 4.0 / 5.7 ms | 4.1 / 5.8 ms |
| registry page, priority order, 50 rows (keyset) | 3.5 / 4.7 ms | 2.4 / 4.4 ms | 7.7 / 10.9 ms |
| lifecycle-filtered page, 50 rows | 1.4 / 1.8 ms | 2.0 / 2.7 ms | 4.2 / 4.3 ms |
| wallets due for evaluation (100) | 1.4 / 3.3 ms | 1.8 / 3.6 ms | 3.5 / 4.9 ms |
| status counts (`/api/v1/radar/status` totals) | 6.6 / 12.7 ms | 7.7 / 13.0 ms | 15.2 / 21.0 ms |

Keyset pagination visited every wallet exactly once at every size (asserted). Duplicate
discovery is a no-op: no second row, transition or event.

**Not measured:** event consumption from a large DATA ENGINE outbox, memory use, storage
growth per wallet, restart recovery time under load, and concurrent claim contention. The
DB tests cover their correctness (restart, idempotent redelivery, job claims) but do not
time them.

**Conclusion (derived):** local CPU and database cost are not the bottleneck at the
configured scale (the registry was measured up to 10,000 wallets; active candidates and investigations are capped at 500 and 25).
Provider credits are.

## Derived: provider credits per unit of work

Costs are from [PROVIDER_CAPABILITIES.md](PROVIDER_CAPABILITIES.md), Helius Free plan,
documentation only: 1 credit per `getSignaturesForAddress` page, `getTransaction` and
`getMultipleAccounts` call. Jupiter and DEX Screener listings cost no Helius credits; they
are limited by their own rate buckets. RADAR's estimates are upper bounds. The DATA ENGINE
does not refetch transactions it already stores, so actual spend is lower whenever
evidence is reused, and the job records both figures (`estimated_credits`,
`credits_spent`).

| Work (defaults) | Upper bound | Notes |
|---|---|---|
| market discovery listings | 0 Helius credits | Jupiter top-traded/organic/trending and recent, DEX Screener boosts only if enabled; cached 60 s |
| pool validation | 1 per 100 unverified pools | `getMultipleAccounts` owner check; once per pool |
| pool sample, initial | 1 + 25 = **26** | one signature page + up to 25 transactions |
| pool sample, expanded | 1 + 100 = 101 | when a pool already yielded wallets |
| one discovery cycle | 5 samples × 26 = **130** | `RADAR_MAX_MARKET_SAMPLES_PER_CYCLE` = 5; a pool is not resampled within six discovery intervals (30 min at defaults) |
| stage A history (7 d, 50 signatures) | 1 + 50 = **51** | |
| stage B history (30 d, 200 signatures) | 2 + 200 = **202** | stage A's transactions are reused |
| stage C history (90 d, 500 signatures) | 5 + 500 = **505** | |
| full investigation of one wallet, A + B + C | ≤ **758** | most wallets stop after stage A (insufficient evidence) |
| monitoring refresh | 1 + 25 = 26 | one page since the newest covered signature; 1 credit when nothing new |
| dormancy recheck | 1 + 25 = 26 | only when inactivity is not yet established by coverage |

## Derived: scenarios (per day, defaults)

Helius Free: 1,000,000 credits per month. With `HELIUS_RESERVE_PERCENT` = 20, the normal
allowance is 800,000 per month, about 26,300 per day. These are ceilings from the
configured work bounds, not forecasts.

| Scenario | Bound per day | Derivation |
|---|---|---|
| discovery cycles at the maximum rate | ≤ 37,440 | 288 cycles × 130. In practice far lower: the resample guard and the supply of eligible pools limit fresh samples |
| 10 monitored wallets, quiet | ~960 | 10 × 96 refreshes (every 900 s) × 1 credit |
| 10 monitored wallets, every refresh full | ≤ 24,960 | 10 × 96 × 26 |
| 25 wallets investigated A → C | ≤ 18,950 | 25 × 758, once per wallet, not per day |
| 50 wallets investigated A → C | ≤ 37,900 | needs `RADAR_MAX_INVESTIGATING_CANDIDATES` = 50 |
| 100 active candidates, stage A only | ≤ 5,100 | 100 × 51, once per wallet |
| 500 registry candidates | 0 ongoing | registered, dormant, rejected or archived wallets cost nothing until new evidence or a recheck |

Registered candidates cost nothing by themselves. Credits are spent only by wallets under
investigation (at most 25 at once, 2 history jobs concurrently) and by monitored wallets
(at most 10).

**Conclusion (derived):** the upper bounds of sampling and monitoring together exceed
the free tier's daily normal allowance. The defaults therefore rely on budget admission
rather than on fitting the bounds:

- **The 40 % share bounds in-flight work, not monthly spend.** `RADAR_NORMAL_BUDGET_SHARE_BPS`
  admits a job only while RADAR's outstanding estimates stay within 40 % of the
  *currently remaining* normal allowance.
- **Cumulative spend is bounded by GATEWAY's budget states.** In `CONSERVATION`, RADAR
  runs only stage A history and local-activity samples. In `CRITICAL`,
  `NORMAL_BUDGET_EXHAUSTED`, `HARD_LIMIT_REACHED` or unknown usage, RADAR pauses all
  provider work.
- **GATEWAY's reserve is never available to RADAR.**
- **Autonomous discovery is off by default** (`RADAR_AUTO_DISCOVERY_ENABLED=false`).

An owner who enables it should expect RADAR to compete with the DATA ENGINE for the same
allowance. If sustained use matters more than discovery speed, lower
`RADAR_MAX_MARKET_SAMPLES_PER_CYCLE` or raise `RADAR_MARKET_DISCOVERY_INTERVAL_SECS`.

## Rate bounds (configuration)

| Bound | Default |
|---|---|
| market universe / active markets | 100 / 20 |
| samples per cycle, concurrent sample jobs | 5, 2 |
| active candidates / under investigation | 500 / 25 |
| concurrent history jobs | 2 (validated to at most twice `DATA_ENGINE_MAX_CONCURRENT_BACKFILLS`) |
| monitored wallets | 10 (live subscriptions only when GATEWAY streams are available; otherwise periodic refresh) |
| pending jobs | 1,000 |
| per-wallet analysis | the newest 5,000 swaps and evidence rows (screening at that size is a few milliseconds; extrapolated from the 2,000-swap figure, not measured) |
| retention | observations 30 d, jobs 30 d, snapshots 90 d; evidence and transitions are kept |
