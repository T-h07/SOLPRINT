# GATEWAY

GATEWAY (`backend/crates/solprint-gateway`, Phase 01) is the **only** code in SOLPRINT
that talks to external market and blockchain providers. It is read-only: it builds,
signs and submits no transactions, loads no keys and has no execution capability. Every
other module reaches providers only through its typed interfaces.

Related: [PROVIDER_CAPABILITIES.md](PROVIDER_CAPABILITIES.md) (what each provider
offers, with sources), [API_BUDGET_POLICY.md](API_BUDGET_POLICY.md) (credits),
[STREAM_RECOVERY.md](STREAM_RECOVERY.md) (WebSockets),
[GATEWAY_CAPACITY_MODEL.md](GATEWAY_CAPACITY_MODEL.md) (throughput and overhead).

## Providers and operations

| Provider | Operations | Auth | Default state |
|---|---|---|---|
| Helius RPC | getHealth, getVersion, getSlot, getBlockTime, getBalance, getAccountInfo, getMultipleAccounts (≤ 100 per request, chunked), getSignaturesForAddress (bounded pagination: ≤ 1 000 per page, ≤ 10 pages), getTransaction / batched getTransactions (≤ 20 per batch, `maxSupportedTransactionVersion: 0`), getTokenAccountBalance, getTokenAccountsByOwner, getTokenSupply, getTokenLargestAccounts, getBlock | `api-key` query parameter | enabled when `HELIUS_API_KEY` is set; **getBlock disabled** |
| Helius WebSocket | slot, root, logs (one address), account, signature, program (filtered) subscriptions | `api-key` query parameter | **disabled** (`GATEWAY_STREAMS_ENABLED=false`) |
| Jupiter | Price V3 (≤ 50 mints per request, chunked), Tokens V2 search, Swap V2 `/order` **quote only** (no `taker`; a response carrying a transaction is refused) | `x-api-key` header when set | enabled |
| DEX Screener | `tokens/v1` (≤ 30 mints, chunked), token pairs, pair, search | none | enabled; discovery (profiles, boosts) **disabled** |
| Solana public RPC | the same JSON-RPC reads as **fallback only**: used when the caller sets `allow_fallback` and Helius is disabled, rate-limited, over budget, circuit-open, timing out, unreachable, queue-full or answering 5xx, with ≥ 500 ms of deadline left | none | enabled as fallback, 2 req/s, 1 in flight |

Hard refusals, independent of configuration:

* JSON-RPC methods `sendTransaction`, `simulateTransaction`, `requestAirdrop`,
  `sendBundle` (`PROHIBITED_RPC_METHODS`); any method without a registered credit cost.
* Paths containing execution or submission fragments (`/swap/v2/execute`,
  `/swap/v2/build`, `/swap/v1/swap`, `/tx/v1/submit`, `/ultra/v1/execute`, any
  `/execute` or `/submit`) and the host `tx.jup.ag` (`transport::BLOCKED_*`).
* Any destination outside the four configured origins (`HOST_NOT_ALLOWED`); plain HTTP
  (TLS only, Rustls with WebPKI roots; loopback HTTP exists for tests only).
* Generic proxying: there is no pass-through endpoint, only typed methods.

## Request pipeline

Every call goes through one engine (`engine.rs`):

```text
cache ─fresh hit─▶ FRESH_CACHE (0 credits)
  │ miss
single-flight ─follower─▶ shares the leader's result
  │ leader
attempt loop (≤ 2 retries, never past the deadline)
  scheduler permit → rate limiter → circuit breaker → budget reservation
  → transport → classify → health / metrics / usage
  │ ok → cache put → DIRECT
  │ err + AllowStaleOnError + stale entry → STALE_CACHE
  │ err + fallback allowed + eligible error → public RPC → FALLBACK
```

* **Request context** (`RequestContext`): caller label (bounded set), priority P0–P5,
  absolute deadline (default 10 s, covers queueing, rate-limit waits and retries),
  commitment, cache policy (`PreferCache`, `Refresh`, `CacheOnly`, `AllowStaleOnError`),
  `max_data_age`, `allow_fallback`, cancellation token, correlation id.
* **Scheduler**: per-provider lanes with a concurrency cap (`*_MAX_IN_FLIGHT`), a bounded
  queue per provider (128) and globally (512). Waiters are served by priority, then FIFO.
  A full queue is refused immediately (`QUEUE_FULL`); abandoned waiters never leak slots.
* **Rate limiting**: GCRA in integer nanoseconds, one limiter per *rate group*. Jupiter's
  price, token and quote endpoints share `jupiter.general`. A 429 with `Retry-After` (or
  a reset header) blocks the whole group for that time; a wait that would pass the
  caller's deadline is refused without consuming capacity.
* **Circuit breaker** per provider: opens after 5 consecutive transport/5xx/timeout
  failures, stays open 30 s doubling to 300 s, admits one probe when half-open. Client
  errors (4xx other than 429) and cancellations do not trip it.
* **Retries**: at most 2, with jittered backoff 200 ms → 2 s, only for retryable errors
  (timeouts, connection failures, 5xx, documented transient RPC codes, 429 after its
  cooldown), never past the deadline, each attempt budget-admitted. Authentication and
  permission failures (401/403), invalid requests and decode errors are never retried.
* **Budget**: Helius calls reserve their credits before sending (see
  [API_BUDGET_POLICY.md](API_BUDGET_POLICY.md)); a call that never reached the provider
  is refunded.
* **Coalescing**: concurrent identical requests (same operation, parameters, commitment)
  share one upstream call. A cancelled leader hands over to a follower.
* **Transport**: one `reqwest` client, connect timeout 5 s, response bodies capped at
  `GATEWAY_MAX_RESPONSE_BYTES` (8 MiB) while streaming, JSON-RPC `id` checked, an error
  inside HTTP 200 is a typed `RPC` error. Error messages are sanitised (URLs, keys,
  control characters removed) before they are stored or returned.

## Responses: provenance and freshness

Every successful call returns `GatewayResponse<T>` with the value and:

| Field | Meaning |
|---|---|
| `provider` | who answered (`HELIUS`, `JUPITER`, `DEX_SCREENER`, `SOLANA_RPC`) |
| `provenance` | `DIRECT`, `FRESH_CACHE`, `STALE_CACHE`, `FALLBACK`, `RECOVERY`, `FIXTURE` |
| `freshness` | `FRESH`, `STALE`, `EXPIRED`, `UNAVAILABLE`, `UNKNOWN_SOURCE_AGE` |
| `observed_at` | when SOLPRINT received it (UTC) |
| `source_timestamp` | the provider's own time, if it gives one |
| `context_slot` | slot the RPC evaluated at, when reported |
| `age` | monotonic age at return |
| `credits_charged` | credits charged for this call (0 for cache hits) |

Cached data keeps its original `observed_at`; a cache hit is never relabelled as newly
observed. Numbers are exact: lamports and raw token amounts are `u64`, prices are
`Decimal` parsed from the provider's text (floats such as `uiAmount` are ignored).

### Cache

`moka` cache, `GATEWAY_CACHE_MAX_ENTRIES` (10 000) entries, namespaced by operation.
Each entry has a *fresh* window and a longer *keep* window (stale but servable only under
`AllowStaleOnError`); some namespaces also cache "not found" briefly.

| Namespace | Fresh | Keep | Not-found |
|---|---|---|---|
| getSlot | 400 ms | 2 s | — |
| getAccountInfo / balances / token accounts | 2 s | 30 s | 5 s |
| getTokenSupply / largest accounts | 30 s | 5 min | 30 s |
| getBlockTime | 1 h | 24 h | 10 s |
| getTransaction (finalized / confirmed) | 24 h / 30 s | 24 h / 2 min | 2 s |
| getVersion | 10 min | 1 h | — |
| Jupiter price | 5 s | 60 s | — |
| Jupiter tokens | 5 min | 1 h | 60 s |
| DEX pairs | 15 s | 2 min | 30 s |
| DEX discovery | 60 s | 5 min | — |
| health checks, quotes | not cached | | |

## Health

Health is derived **only from real traffic**; there are no background probes. A provider
is `UNKNOWN` until its first request, then `HEALTHY`, `DEGRADED` (recent failures,
half-open circuit, or a provider rate limit within the last 60 s) or `UNAVAILABLE` (open
circuit or every recent request failed), based on the last 20 outcomes. `DISABLED` means
turned off or missing its credential; `BUDGET_EXHAUSTED` means the budget no longer
admits normal traffic.

## Metrics, persistence and the monitoring API

* In-process metrics per operation: ok / error counts by error class, cache hits,
  coalesced, retries, latency histogram (15 fixed buckets; percentiles are reported as
  bucket upper bounds). Labels come from the closed `operations::ALL` set, so
  cardinality is fixed.
* Every `GATEWAY_FLUSH_INTERVAL_SECS` (30 s) and at shutdown, GATEWAY writes additive
  hourly usage and metric buckets and the billing-cycle credit total to PostgreSQL
  (migration `20261126000002_gateway.sql`: `gateway_billing_cycles`,
  `gateway_usage_buckets`, `gateway_metric_buckets`, `gateway_stream_checkpoints`).
  A failed flush keeps the deltas and retries. Rows older than the retention settings
  (usage 90 days, metrics 14 days) are pruned hourly.
* `GET /api/v1/gateway/{status,providers,usage,metrics,budgets,streams,cache}` and the
  provider list in `/api/v1/system/status` read **only** in-process snapshots and stored
  aggregates. Refreshing the dashboard never causes a provider request (tested in
  `snapshots_and_stored_reads_never_contact_providers`, the API tests and the smoke
  test).

## Interfaces for future modules

`traits.rs` defines `BlockchainProvider`, `BlockchainStreamProvider`,
`TokenPriceProvider`, `TokenMetadataProvider`, `MarketDataProvider` and
`SwapQuoteProvider`, all implemented by `Gateway`. Future modules depend on these traits,
not on provider DTOs: provider JSON is decoded inside GATEWAY into the normalized models
in `models.rs`.

## Lifecycle

`solprint-app` builds `Gateway::new(GatewaySettings::from_config(..), store)`, loads the
credit ledger, and starts the flush/retention task (and the stream manager when enabled).
On shutdown, after the HTTP server drains, GATEWAY is cancelled and performs a final
flush (bounded by a timeout). If the ledger cannot be loaded, the budget is
`ACCOUNT_USAGE_UNKNOWN` and admits only P0–P2 until it loads.

## Diagnostics

```powershell
./scripts/gateway-diagnostics.ps1          # offline: configuration only, contacts nothing
./scripts/gateway-diagnostics.ps1 -Live    # explicit opt-in: bounded read-only calls
```

`-Live` performs at most: Helius getHealth, getVersion, getSlot (3 credits); one Jupiter
price request and one DEX Screener token request for wrapped SOL; the public RPC only
with `-IncludeFallback`. Credentials are reported by name and presence, never by value.
Results describe that moment only and are not stored as current measurements. Credits
are charged to the persisted ledger when PostgreSQL is reachable; otherwise the report
says they were not recorded.

## Credential handling

Provider keys live in the owner's `.env`, are wrapped in `Secret<T>` and are exposed
only when a request is built. URLs carrying keys are never logged or returned. Provider
credentials were exposed in an earlier conversation and their rotation has not been
independently confirmed: **rotating them is an owner responsibility**.
