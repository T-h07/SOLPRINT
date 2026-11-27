# DATA ENGINE capacity model

Measured CPU cost, provider-credit arithmetic and the limits that bound the DATA ENGINE.
Only the benchmark figures below were measured. Everything else is derived from the
configuration and the documented provider costs, and is labelled as derived. No live
mainnet throughput has been measured.

## Measured: decode and write-set cost

`cargo bench -p solprint-data-engine` (optimized build). Inputs are the 17 synthetic
fixtures, so real mainnet transactions, which are often larger, will cost more per
transaction. No database or provider is involved.

Machine: Intel Core i5-1245U (12th Gen, laptop), 32 GB RAM, Windows 11 Pro, rustc 1.99.1.
Measured 2026-11-27.

| Benchmark | Per operation | Throughput (one core) |
|---|---|---|
| decode, catalogue mix | 31.8 µs | ~31,400 tx/s |
| decode, fastest fixture (`unsupported_program`) | 6.3 µs | ~159,000 tx/s |
| decode, slowest fixture (`pump_fun_buy`, event parsing) | 73.8 µs | ~13,500 tx/s |
| decode, `jupiter_route_two_legs` | 50.0 µs | ~20,000 tx/s |
| write set including SHA-256 of the evidence, catalogue mix | 12.8 µs | ~78,000 tx/s |

**Conclusion (derived):** CPU is not the bottleneck. Even at 10× these costs, one core
decodes far more transactions than the provider budget and rate limits allow the engine
to fetch.

## Derived: provider credits

Costs are from [PROVIDER_CAPABILITIES.md](PROVIDER_CAPABILITIES.md), Helius Free plan,
documentation only: 1 credit per `getTransaction`, `getSignaturesForAddress` and
`getSignatureStatuses` request.

| Work | Credits |
|---|---|
| ingest one transaction (`getTransaction`) | 1 (0 when served from GATEWAY's cache) |
| finality checks for one transaction | each reconciler pass sends **one** `getSignatureStatuses` for up to `DATA_ENGINE_FINALITY_BATCH_SIZE` (64) due transactions, so ≈ 1/64 credit per check; checks back off 2ⁿ and stop at finalization or after 20 checks |
| finality refetch (status `null` or slot mismatch) | 1 more `getTransaction` |
| one backfill page | 1 (`getSignaturesForAddress`, up to 100 signatures) |
| one backfill, upper bound | `pages + max_signatures`; at defaults 10 + 1,000 = **1,010**. The CLI prints this before creating anything |
| reprocessing | 0 (stored evidence) |

At the Free plan's 1,000,000 credits per month, with GATEWAY's reserve and priority
admission, this is on the order of several hundred thousand transactions per month
(derived, not measured). That holds only if nothing else spends credits. GATEWAY's ledger
is authoritative, and budget denial pauses the engine rather than overspending.

Live intake subscriptions (`logsSubscribe`) cost stream credits per GATEWAY's model
([GATEWAY_CAPACITY_MODEL.md](GATEWAY_CAPACITY_MODEL.md)), and each observed signature then
costs one `getTransaction`. A busy target wallet can therefore consume credits quickly.
This is why live intake is off by default and bounded by `DATA_ENGINE_MAX_ACTIVE_TARGETS`.

## Rate bounds (configuration)

| Bound | Default |
|---|---|
| concurrent fetches (`DATA_ENGINE_FETCH_CONCURRENCY`) | 4, never above `HELIUS_MAX_IN_FLIGHT` |
| job workers (`DATA_ENGINE_DECODE_CONCURRENCY`) | 4 |
| queue (`DATA_ENGINE_QUEUE_CAPACITY`) | 2,048 non-terminal jobs; above it, intake is refused and backfills pause |
| request rate | GATEWAY's Helius limiter (`HELIUS_RPC_RPS`, default 5; 10 req/s documented) |

## Storage (derived)

Per transaction, the engine stores:

- one evidence row: raw JSON up to `DATA_ENGINE_EVIDENCE_MAX_BYTES` (default 1 MiB, hard
  cap), plus a hash and metadata;
- one transaction row;
- one balance-change row per account whose balance changed;
- one row per activity, swap, route leg and fee.

Raw JSON dominates. Its real size depends on the transaction and has not been measured
against mainnet. Raw evidence of finalized transactions is pruned after
`DATA_ENGINE_EVIDENCE_RETENTION_DAYS` (90), which bounds long-term growth. Normalized
records are kept. Jobs (14 days) and delivered outbox events (7 days) are also pruned
([DATA_ENGINE.md](DATA_ENGINE.md#retention)).

## Not measured

- end-to-end throughput against PostgreSQL under load;
- real mainnet evidence sizes;
- live stream event rates for real targets;
- provider latency.

These need an owner-approved live run and are reported as **NOT RUN** in
[PHASE_02.md](../phases/PHASE_02.md).
