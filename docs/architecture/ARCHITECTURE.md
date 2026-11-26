# Architecture

Status: **Phase 00 (Foundation)**. Nothing in this build observes markets, contacts a
provider, holds a key or executes a transaction.

## System overview

```
 Browser ──same origin──▶ frontend (nginx :8080 in Compose, Vite :5173 natively)
                              │  /api/*  /health/*  reverse proxy
                              ▼
                          backend (Rust, Axum)  ── solprint binary
                              │  SQLx pool (least-privilege role solprint_app)
                              ▼
                          PostgreSQL 18 (schema `solprint`)
```

* **One origin.** The browser only ever talks to the frontend origin. API calls are
  proxied, so there is no CORS surface and the backend is never published to the host in
  Compose.
* **Backend is authoritative** for operating mode, readiness and every value the UI shows.
  The UI derives nothing safety-relevant from browser state; when the backend is
  unreachable the shell shows `MODE UNVERIFIED`, never an assumed `PAPER`.
* **Contracts are generated.** API response types are Rust structs
  (`solprint-api/src/contracts.rs`) exported to TypeScript with `ts-rs` into
  `frontend/src/types/generated`. CI fails if the committed TypeScript drifts.

## Backend workspace (`backend/`)

| Crate | Responsibility | I/O |
|---|---|---|
| `solprint-core` | Domain primitives: exact amounts (`Lamports`, `TokenAmount`, `BasisPoints`, `Price`), Solana identifiers, UTC timestamps, IDs (UUIDv7), `TradingMode`, `ReasonCode`, `DataFreshness`, `EventEnvelope` | none |
| `solprint-config` | Typed configuration from env/.env, full validation, `Secret<T>`, live-trading rejection | reads env |
| `solprint-observability` | `tracing` subscriber (pretty/JSON), filter policy, request-id header | stdout |
| `solprint-storage` | PostgreSQL pool, embedded migrations, instance registration, health probe | database |
| `solprint-api` | Axum router, contracts, middleware (request id, access log, error envelope, security headers, limits) | HTTP |
| `solprint-app` | The `solprint` binary: lifecycle, signals, graceful shutdown, `healthcheck` subcommand | process |

Dependency rules are in [MODULE_BOUNDARIES.md](MODULE_BOUNDARIES.md).

### Process lifecycle (`solprint-app`)

1. Load and validate configuration. Any invalid key → exit code 2 with every problem
   listed by key name (values are never printed).
2. Initialise logging, then the Tokio runtime.
3. Connect to PostgreSQL with bounded retry/backoff until `DATABASE_STARTUP_TIMEOUT_SECS`.
4. Apply embedded migrations (`DATABASE_RUN_MIGRATIONS=true`) or verify they are all
   applied.
5. Register the start in `solprint.instance` (stable `instance_id`, `start_count`).
6. Bind (loopback unless `APP_ALLOW_NON_LOOPBACK=true`) and serve.
7. On SIGTERM/SIGINT (Unix) or Ctrl-C/Ctrl-Break/close (Windows): stop accepting, drain
   in-flight requests for up to 10 s, close the pool, exit 0.

### HTTP surface (Phase 00)

| Route | Purpose |
|---|---|
| `GET /health/live` | Process is alive. Never touches dependencies. |
| `GET /health/ready` | 200 `READY` when the database probe succeeds, 503 `NOT_READY` otherwise (structured body either way). |
| `GET /api/v1/system/status` | Operating mode (`PAPER`, live disabled, `execution_capability: NONE`), database state, migrations, provider integration state (`NOT_INITIALIZED`), uptime. |
| `GET /api/v1/system/version` | Name, version, API version, git commit (if built with one), build profile. |

Every response carries `x-request-id` and security headers; every non-2xx response is a
JSON error envelope `{ "error": { "code", "message", "request_id" } }`. Request bodies
are capped (`HTTP_BODY_LIMIT_BYTES`) and requests time out (`HTTP_REQUEST_TIMEOUT_SECS`,
408).

## Database

* Migrations live in `backend/migrations` and are embedded in the binary
  (`sqlx::migrate!`). They are forward-only; a migration that has been pushed is never
  edited.
* Phase 00 schema: `solprint.instance` (singleton row: `instance_id`, `created_at`,
  `last_started_at`, `start_count`, `last_app_version`).
* Compose creates two roles: `solprint_admin` (superuser, bootstrap only) and
  `solprint_app` (non-superuser, owns database `solprint`), which the backend uses.

## Frontend (`frontend/`)

React 19 + TypeScript (strict), Vite, Tailwind 4 with SOLPRINT tokens, TanStack Query for
server state, Radix primitives for accessible overlays. Structure:

```
src/app         App, router, query client, error boundaries, build flags
src/components  ui/ (design-system primitives) and shell/ (status bar, nav, indicators)
src/features    feature panels (system/)
src/pages       routed pages
src/lib         api client + guards + queries, formatting (numbers, time)
src/types       generated contracts (never edit) + barrel
src/fixtures    labelled fixtures, imported only by the design-system showcase
```

The `/design-system` showcase exists in development and `--mode showcase` builds only; it
is statically removed from production bundles (verified in CI).

## Deployment posture

Phase 00 is a **local development foundation**, not approved for public deployment.
Compose publishes only loopback ports, runs containers as non-root with read-only root
filesystems and no capabilities. See [SECURITY_MODEL.md](SECURITY_MODEL.md).

## Target architecture: modular, event-driven monolith

SOLPRINT grows into one process with ten modules. Phase 00 builds only the foundation
they share; nothing below is implemented yet.

| Module | Responsibility |
|---|---|
| GATEWAY | external provider communication, caching, rate limits, API budgets |
| DATA ENGINE | blockchain ingestion, transaction decoding, event normalisation |
| RADAR | wallet discovery, monitoring, ranking, intelligence |
| FILTER | token security, liquidity and manipulation analysis |
| CORTEX | quantitative strategy and decision-making |
| GUARD | independent risk management and authorisation |
| REPLAY | historical backtesting and strategy validation |
| SANDBOX | real-time paper trading |
| SNIPER | authorised transaction execution and reconciliation |
| TERMINAL | monitoring, analytics and application control (this frontend) |

### Planned event flow (indicative, to be specified per phase)

```
providers ─▶ GATEWAY ─▶ DATA ENGINE ─▶ EventEnvelope<T> ─┬─▶ RADAR ─┐
                                                          ├─▶ FILTER ┼─▶ CORTEX ─▶ GUARD ─▶ SANDBOX (paper)
                                                          │          │                  └─▶ SNIPER (live, gated)
                                                          └──────────┴─▶ PostgreSQL ─▶ REPLAY, TERMINAL
```

Every event carries source/observed/processed timestamps, a data source, a quality flag
and a schema version (`solprint-core::event`). In-process channels are the default
transport; a broker is introduced only if a measured requirement demands it.

### Provider boundary

Only GATEWAY will talk to external providers. It owns credentials, rate limits, caching
and the API budget (initially zero paid subscriptions). Other modules consume normalised
events and never see a provider client or key.

### Execution safety boundary

Only SNIPER will be able to build or submit transactions, only after GUARD authorises the
specific action, and only when live trading is explicitly enabled by the owner in a phase
that has been approved for it. Until then, configuration rejects live mode, the status
API reports `execution_capability: NONE`, and signing/wallet crates are banned from the
dependency graph. The UI treats live activation and emergency stop as safety-critical
(see `docs/design/UI_UX_SPEC.md`).

## Decisions and trade-offs (Phase 00)

| Decision | Why | Trade-off |
|---|---|---|
| Modular monolith, Rust workspace | one deployable, compile-time boundaries, runs on consumer hardware | modules share a process; isolation is by crate rules, not network |
| PostgreSQL only (no Redis/Kafka) | one stateful dependency; enough for Phase 00–01 | caching/broker added later only with evidence |
| SQLx with embedded, forward-only migrations | checked SQL, migrations ship with the binary | no down-migrations; fixes are new migrations |
| Contracts generated Rust → TypeScript (ts-rs) | single source of truth, CI drift check | backend build needed to change frontend types |
| Same-origin reverse proxy | no CORS surface, backend unpublished | proxy configuration in two places (Vite, nginx) |
| Amounts as integer strings, `BigInt` in the UI | exact u64; no float drift | more verbose formatting code |
| Playwright in a pinned container for visuals | reproducible fonts, nothing installed on the host | requires Docker for UI/visual suites |
| No authentication in Phase 00 | loopback-only, single operator, no execution | must be designed before any non-local exposure |
