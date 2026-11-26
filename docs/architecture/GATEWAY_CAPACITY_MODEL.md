# GATEWAY capacity model

What GATEWAY can sustain with the default configuration, which limit binds first, and
how its own overhead was measured. The provider figures below are **arithmetic from
configured and documented limits**, not measurements of provider behaviour. The overhead
figures are **measurements on one machine** and say nothing about provider latency.

## Binding constraints (defaults)

| Provider | Configured rate | Max requests / day at that rate | Other bound |
|---|---|---|---|
| Helius RPC | 5 req/s (documented 10) | 432 000 | **1 000 000 credits / cycle** |
| Jupiter | 50 req/min, shared by price, tokens and quotes (documented 60) | 72 000 | none metered |
| DEX Screener | 240 req/min (documented 300 for pair endpoints) | 345 600 | none metered |
| Solana public RPC | 2 req/s, fallback only | 172 800 | shared public service: never a primary source |

For Helius, the **credit budget binds long before the rate limit**. Running at the
configured 5 req/s would use the whole allowance in about 2.3 days. Spread evenly over a
30-day cycle, the allowance is:

| Budget | Credits | Average sustainable rate (30 days) |
|---|---|---|
| Whole allowance | 1 000 000 | ≈ 0.39 credits/s (≈ 1 389 / hour) |
| Normal budget (excl. 20 % reserve) | 800 000 | ≈ 0.31 credits/s (≈ 1 111 / hour) |
| Until CONSERVATION (70 % of normal) | 560 000 | ≈ 0.22 credits/s |

Examples (arithmetic, 30-day cycle, 1 credit per call):

| Workload | Credits / cycle | Share of normal budget |
|---|---|---|
| getSlot polled every 10 s | 259 200 | 32 % |
| getSlot polled every 1 s | 2 592 000 | exceeds the allowance: do not poll |
| 100 accounts checked every 60 s via getMultipleAccounts (1 call) | 43 200 | 5 % |
| 100 accounts checked every 60 s via 100 getAccountInfo calls | 4 320 000 | exceeds the allowance |
| WebSocket, per MB received | 20 | — |
| WebSocket, 1 GB received | ≈ 20 480 | 2.6 % |

Consequences already built into GATEWAY: the cache absorbs repeated reads within the
freshness windows; coalescing turns concurrent identical reads into one call; batch
methods (`getMultipleAccounts`, DEX `tokens/v1`) are used and chunked at the documented
maxima; `getBlock` is disabled; streams are opt-in; and the budget states shed
low-priority traffic before the reserve is touched. Actual consumption depends on what
future modules request and must be watched on the Gateway page; it has not been
measured, because no module generates sustained traffic in Phase 01.

## Memory bounds

| Structure | Bound |
|---|---|
| Response cache | `GATEWAY_CACHE_MAX_ENTRIES` (10 000) entries; values are decoded models, response bodies ≤ `GATEWAY_MAX_RESPONSE_BYTES` (8 MiB) |
| Scheduler queues | 128 waiters per provider, 512 overall |
| Metrics | fixed: closed operation set × providers × 16 latency buckets |
| Stream events | 1 024-event channel; 4 096-entry dedup set; ≤ `HELIUS_WS_MAX_SUBSCRIPTIONS` (100) subscriptions; ≤ 64 log lines of ≤ 512 characters per notification |
| Stored telemetry | hourly buckets per provider × operation; usage kept 90 days, metrics 14 days, pruned hourly |

## Overhead benchmarks

Method: `cargo bench -p solprint-gateway` (`benches/gateway_bench.rs`, release profile,
`harness = false`). Each case runs a fixed number of iterations and reports the mean time per operation. The "provider" is a loopback mock server; no
external service is contacted.

Measured on 2026-11-26, Intel Core i5-1245U (12th gen, laptop), 32 GB RAM, Windows 11,
Rust 1.99.1. **These figures are specific to that machine and run.** They are not
guarantees and will differ elsewhere.

| Case | Iterations | Mean per op | What it measures |
|---|---|---|---|
| `budget.try_reserve` | 200 000 | 26 ns | atomic credit reservation |
| `metrics.record` | 200 000 | 88 ns | one outcome + latency histogram update |
| `gateway.get_slot`, fresh cache hit | 50 000 | 1.30 µs | full request path for a cache hit |
| `gateway.snapshot` | 20 000 | 1.64 µs | building the monitoring snapshot |
| `gateway.get_slot`, loopback miss | 20 | 52 ms | dominated by the configured rate limit, **not** GATEWAY overhead |

Reading: GATEWAY's own bookkeeping costs microseconds per request, three to five orders
of magnitude below network round-trips to a provider. Per-request overhead is therefore
not a capacity concern; provider limits and the credit budget are. The loopback-miss case
mainly shows that the rate limiter spaces requests as configured.

To re-measure after a change, run the command above on an otherwise idle machine and
record the machine, date and toolchain alongside the numbers.
