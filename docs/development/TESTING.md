# Testing

**The gate is `./scripts/verify.ps1`.** It verifies the exact commit in a clean,
isolated Git worktree (no `.env`, no untracked files), runs everything below plus secret
scanning, Dockerfile lint, dependency advisories, clean Docker image builds and an
isolated Compose stack (own project name, image tags, ports and volumes; synthetic
`.env`; smoke test; e2e; container hardening; log secret check), then removes only what
it created. Each check is reported as PASS / FAIL / BLOCKED / NOT RUN with duration and
test counts; logs go to `.solprint/verify/<sha12>/`. Any FAIL or BLOCKED exits non-zero.
`-Quick` reuses a build cache and skips Playwright and the stack (reported NOT RUN; never
a full pass). SOLPRINT uses no hosted CI/CD.

| Suite | Command | Needs |
|---|---|---|
| **Full gate (exact commit, clean checkout)** | `./scripts/verify.ps1` | Docker |
| Rust unit + HTTP | `cd backend; cargo test --workspace --all-features` | — |
| Rust + PostgreSQL integration | `./scripts/test.ps1` | Docker |
| Frontend unit (Vitest, jsdom) | `cd frontend; npm test` / `npm run test:ci` | — |
| UI: states, keyboard, axe, overflow | `./scripts/playwright.ps1 -Project ui` | Docker |
| Visual regression | `./scripts/playwright.ps1 -Project visual` | Docker |
| End-to-end (real stack) | `./scripts/dev.ps1 -Mode docker; ./scripts/playwright.ps1 -Project e2e` | Docker |
| HTTP smoke | `./scripts/smoke-test.ps1` | running stack |
| Container-free checks on the working tree | `./scripts/check.ps1` | — |
| GATEWAY overhead benchmarks | `cd backend; cargo bench -p solprint-gateway` | — |
| GATEWAY diagnostics (offline / live opt-in) | `./scripts/gateway-diagnostics.ps1 [-Live]` | `.env`; `-Live` contacts providers |
| DATA ENGINE decode/write-set benchmarks | `cd backend; cargo bench -p solprint-data-engine` | — |
| DATA ENGINE fixtures (offline, no database) | `cd backend; cargo run --bin solprint -- data-engine decode-fixture` | — |
| DATA ENGINE diagnostics (offline / live opt-in) | `./scripts/data-engine-diagnostics.ps1 [-Live -Signature <SIG>]` | `.env`; `-Live` sends one `getTransaction` |

## Database tests never touch your data

PostgreSQL tests are `#[ignore]` by default and need `SOLPRINT_TEST_DATABASE_URL`.
`scripts/test.ps1` starts a throwaway `solprint-test-db` container (tmpfs storage, random
password, loopback port 55433), runs the tests with `--include-ignored`, and removes the
container afterwards, even on failure. Each test creates and drops its own uniquely named
database. Do not point `SOLPRINT_TEST_DATABASE_URL` at the development database.

## Backend coverage (Phase 00)

* `solprint-core`: amount parsing/arithmetic/overflow/rounding, identifier validation,
  timestamps, reason codes, freshness, envelopes.
* `solprint-config`: defaults, validation of every key, live-trading rejection, https
  enforcement, loopback enforcement, and a synthetic-secret leak test over `Debug`, errors
  and the safe summary.
* `solprint-api`: liveness, readiness 200/503, status, version, 404/405/413 envelopes,
  request-id handling, security headers, secret leak, graceful shutdown.
* `solprint-storage`: migrations, idempotent re-run, instance registration and
  persistence, health probe (database tests).
* Contract export: `cargo test -p solprint-api` regenerates
  `frontend/src/types/generated`; `check.ps1` fails on drift.

## GATEWAY coverage (Phase 01)

No test contacts a real provider. Providers are loopback mock servers (axum for HTTP,
`tokio-tungstenite` for WebSockets) driven by per-test scripts.

* Unit (`src/**`): budget thresholds and admission, cycle rollover, stream metering,
  rate limiter (burst, spacing, deadline refusal, upstream block), scheduler (priority,
  bounded queues, global limit, no leaked slots), breaker (threshold, capped backoff,
  probes), cache freshness and negative entries, coalescing (shared errors, leader
  hand-over), retry classification, health derivation, JSON-RPC id/error handling,
  exact number parsing, provider DTO normalization, destination policy and redaction,
  subscription validation.
* `tests/gateway_mock.rs`: caching and provenance, RPC errors inside HTTP 200, id
  mismatch, not-found, oversized payloads, 429 → fallback only when allowed, circuit
  opening, retries, budget shedding and persistence across restarts, unloaded ledger,
  coalescing, deadlines, cancellation, getBlock off, missing key, bounded pagination,
  getMultipleAccounts and DEX batch limits, versioned transactions, Jupiter quote-only
  (no taker; responses with a transaction refused), shared Jupiter rate group,
  snapshots and stored reads never contacting providers, flush failure and recovery.
* `tests/gateway_stream.rs`: subscribe/ack, notifications and byte metering, failed
  subscriptions, reconnect with resubscription and generations, heartbeat detection of a
  silent connection, logs gap recovery (oldest first, dedup, checkpoints), incomplete
  recovery reported as a gap, signature subscriptions, backpressure, limits, shutdown.
* `tests/gateway_db.rs` (PostgreSQL, via `test.ps1`): ledger survives restarts, additive
  upserts, usage/metric accumulation and pruning, checkpoint constraints.
* `solprint-api/tests/http.rs`: every `/api/v1/gateway/*` endpoint answers from local
  state, malformed queries are rejected, no secret appears in any response.

## DATA ENGINE coverage (Phase 02)

No test contacts a real provider. Inputs are 17 synthetic fixtures
(`src/fixtures.rs`, one per decoder and per edge case), each with its expected outcome.
They are built from the programs' published layouts and are **not** captured mainnet
transactions ([DECODER_CAPABILITIES.md](../architecture/DECODER_CAPABILITIES.md)).

* Unit (`src/**`):
  * reconstruction: account order and signer/writable flags, the CPI tree from stack
    heights, and missing metadata reported rather than guessed;
  * evidence validation: non-JSON, unknown versions and missing `meta`;
  * borsh readers, discriminators checked against their preimages and the official
    Pump.fun IDL arrays, and program ids;
  * exact deltas, ratios and values;
  * every fixture decoding to its expected outcome, plus targeted cases: Jupiter route
    legs, relayer fee payer, program-controlled swaps, failed transactions, Token-2022
    fees, wrap flows, Pump fees and missing inner instructions;
  * transfers and liquidity deposits are never swaps, and an unsupported Token-2022
    extension raises `UNSUPPORTED_TOKEN_EXTENSION`;
  * write-set schema shapes and retraction events;
  * backoff and priorities.
* Property tests (`proptest`): arbitrary bytes and text never panic the borsh reader,
  evidence parser or ratio, and ratios are always positive.
* `tests/engine_db.rs` (PostgreSQL, via `test.ps1`, with an in-memory `ChainSource`):
  * ingest persists once and reprocessing is idempotent;
  * budget denial releases a job without consuming an attempt;
  * an unknown transaction is retried and never invented;
  * finality advances, and a `null` status is never treated as orphaned;
  * a backfill is bounded, resumable and deduplicated;
  * every fixture persists within the schema's constraints and reads back, and the
    verified-trade query (`verified_only`) excludes program-controlled, partially decoded
    and insufficient-evidence swaps;
  * invalid evidence is quarantined, and a slot mismatch is not treated as orphaning;
  * a backfill paused by shutdown and then by the budget resumes from its stored cursor
    in a new process, without refetching pages, and skips a signature already seen
    live;
  * an expired lease (crashed worker) returns its job, which is then processed once;
    undelivered events are delivered after a restart, and duplicate or late
    acknowledgements never undo a delivery;
  * out-of-order observations only raise priority, cause one retrieval, and never
    regress finality;
  * stream `GAP` and `CONTINUITY_UNKNOWN` events stay open as target issues, and live
    logs become jobs;
  * a full queue refuses intake with `QUEUE_SATURATED` and pauses backfills before any
    page is fetched; a page-limited backfill reports `PARTIAL_HISTORY` and an open
    `BACKFILL_INCOMPLETE` issue;
  * `u64::MAX` amounts and ±`u64::MAX` balance deltas round-trip exactly through the
    schema;
  * corrected (finalized) evidence writes revision 2, keeps the superseded evidence,
    and retracts the swap with `EVIDENCE_REVISION` and a `SwapRetracted` event.
* `solprint-api/tests/http.rs`: every `/api/v1/data/*` endpoint is listed as read-only.
  The status degrades truthfully without a database, and reads return a 503 envelope when
  the database is down.
* Frontend: `tests/unit/data.test.ts` covers formatting and guards. `tests/ui/shell.spec.ts`
  (`data engine workspace`) checks exact amounts, attribution and finality as text, the
  protocol filter, the keyboard inspector (focus returns on Esc), and the empty,
  database-down (partial), offline and loading states. The visual baselines are `data-*`
  at 1920/1440/1280/1024/390, and empty and database-down at 1440/390.

Real-provider ingestion (`-Live`) is an owner-run diagnostic and is never part of a
normal test run.

## RADAR coverage (Phase 03)

No test contacts a real provider. Discovery listings come from a fixture feed, and chain
data comes from the DATA ENGINE over an in-memory `ChainSource`. Wallets, pools and
mints in fixtures are synthetic.

* Unit (`crates/solprint-radar/src/**`, 42 tests, no database):
  * market classification (known mints excluded, launchpad pools, tags over heuristics)
    and the universe: bucket allocation, missing fields defer instead of reading as zero,
    stale observations and recently sampled pools defer, promotional-only and excluded
    classes, no market picked twice;
  * wallet eligibility rules and evidence-gated, ordered qualification;
  * FIFO screening with exact `Decimal` arithmetic: pre-existing inventory has unknown
    cost and is never zero cost, unobserved outflows taint a cycle, partial sells
    allocate cost exactly, denominations never mix, fees count only when the wallet paid
    them, open positions are not outcomes; activity windows;
  * priority: bounded, recency half-life, shrinkage toward neutral with few outcomes, and
    the documented value without outcomes;
  * lifecycle (progressive stages, coverage-gated dormancy, archival and reactivation),
    monitoring rotation (hysteresis, tenure, cooldown, capacity, reduced activity) and
    budget admission (share of remaining minus outstanding; degraded or unknown pauses).
* `tests/radar_db.rs` (PostgreSQL, via `test.ps1`), all end to end through the real
  DATA ENGINE:
  * an autonomous cycle from an empty registry: listings, pool validation, sampling,
    staged history A to C, screening, priority, qualification and monitoring;
  * event redelivery is idempotent, and a `SwapRetracted` invalidates the evidence;
  * degraded or unknown budget pauses provider work;
  * a provider outage is recorded, and local sources still run;
  * with discovery off, no provider job runs;
  * a restart resumes from durable state without duplicate jobs;
  * operator controls and registry queries;
  * dormancy is coverage-aware, and new activity reactivates a wallet.
  * eight concurrent claimers never lease the same job twice (`FOR UPDATE SKIP LOCKED`).
* `tests/radar_scale.rs`: an opt-in registry scale benchmark (`SOLPRINT_RADAR_SCALE=1`,
  100 to 10,000 wallets). It is ignored and a no-op in routine runs
  ([RADAR_CAPACITY_MODEL.md](../architecture/RADAR_CAPACITY_MODEL.md)).
* `benches/radar_bench.rs`: CPU policy benchmarks (`cargo bench -p solprint-radar`).
* `solprint-api/tests/http.rs`: every `/api/v1/radar/*` endpoint is read-only, queries
  are validated before storage is touched, reads return a 503 envelope when the database
  is down, and the registry is served from local state.
* Frontend: `tests/unit/radar.test.ts` covers formatting and guards.
  `tests/ui/shell.spec.ts` (`radar workspace`) covers states as text, the lifecycle
  filter, the keyboard wallet inspector (focus returns on Esc), paused discovery with a
  conservation budget, and the empty, database-down, offline and loading states. The
  visual baselines are `radar-*` (candidates until the owner approves them).

`./scripts/radar-diagnostics.ps1` runs the policy tests and reads local state.
`-EndToEnd` runs `radar_db` on its own disposable container. `-Live -RequestCycle` is an
owner-run opt-in and is never part of a normal test run.

## Frontend coverage

* Unit: number/time formatting (u64 exactness, rounding, signs), connection-state
  derivation (connecting/connected/degraded/offline/stale), API client failure
  classification (unreachable, proxy 502, structured error, contract mismatch, malformed
  JSON, timeout), generated-contract barrel completeness, fixture isolation, shell
  rendering (PAPER, MODE UNVERIFIED, navigation, skip link, single h1, not-found).
* UI (Playwright, mocked API): healthy / offline / database-down / loading states, the
  Gateway workspace (active, idle, pressure, database-down, offline, loading),
  keyboard navigation and visible focus, axe WCAG 2.2 AA on every route and the offline
  state, no horizontal overflow and mode + connection always visible at every standard
  viewport.

## Visual regression

* Viewports: 1920×1080, 1440×900, 1280×800, 1024×768, 390×844. Pages: Overview, Gateway
  (active, idle, pressure), System, Design system (full page), plus offline and
  database-down states at 1440 and 390.
* The API is mocked and the clock frozen at `2026-11-26T12:00:00Z`, so captures are
  deterministic. They run only inside `mcr.microsoft.com/playwright:v1.65.1-noble`
  (via `scripts/playwright.ps1`, also from `verify.ps1`) so fonts render identically.
* Baselines: `frontend/tests/visual/__screenshots__/`. **Baselines are candidates until
  the owner approves them.** To update after an intentional change:

  ```powershell
  ./scripts/playwright.ps1 -Project visual -UpdateSnapshots
  ```

  Review every changed PNG in the diff before committing.
