# Phase 00 — Foundation

Branch: `feature/00-foundation` (merged into `develop` as `3bc46cc`, branch deleted).
Status: **accepted** — all criteria PASS, owner approval recorded 2026-11-26. No release or tag.

> **Policy change in Phase 01.** SOLPRINT no longer uses hosted CI/CD. The GitHub Actions
> workflow described below was removed on `feature/01-gateway`; its checks now run
> locally through `scripts/verify.ps1` on the exact candidate commit. The CI results
> recorded in this document (for example P00-T39) are kept unchanged as historical
> evidence of Phase 00.

## Scope

In scope: Rust workspace and shared domain types, typed configuration and secret
handling, PostgreSQL/SQLx with migrations, health/readiness/status/version API,
structured logging, Docker Compose, PowerShell workflows, the TERMINAL shell and design
system, tests (unit, integration, UI, accessibility, visual, end-to-end), CI and
documentation.

Out of scope (not implemented): provider integrations (Phase 01), ingestion, wallet
tracking, token analysis, strategy, risk, backtesting, paper trading, execution,
wallets/keys/signing, authentication, public deployment.

## What was built

| Area | Implementation |
|---|---|
| Backend | 6 crates: `core`, `config`, `observability`, `storage`, `api`, `app` ([ARCHITECTURE](../architecture/ARCHITECTURE.md)) |
| Domain | exact amounts (`Lamports`, `TokenAmount`, `BasisPoints`, `Price`), Solana identifiers, UTC timestamps, UUIDv7 ids, `TradingMode`, `ReasonCode`, `DataFreshness`, `EventEnvelope` |
| Config | full validation with per-key issues, `Secret<T>`, live trading rejected, https-only provider URLs, loopback-only bind by default |
| Database | PostgreSQL 18, embedded forward-only migrations, `solprint.instance`, least-privilege `solprint_app` role |
| API | `/health/live`, `/health/ready`, `/api/v1/system/status`, `/api/v1/system/version`; request ids, error envelope, security headers, body limit, timeout, graceful shutdown |
| Frontend | shell (mode, connectivity, UTC clock, navigation), Overview, System, Not found, internal Design system; tokens, primitives, data states; generated contracts |
| Infrastructure | Compose (postgres, backend, frontend), hardened containers, scripts `setup`, `dev`, `stop`, `check`, `test`, `smoke-test`, `playwright` |
| CI | `.github/workflows/ci.yml`: backend, supply chain, secret scan, frontend, UI/visual, stack (compose, smoke, e2e, hardening, log secret check) |

## Test results (local, 2026-11-26, Windows 11, Docker Desktop)

| Suite | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy -D warnings` | pass |
| Backend tests incl. PostgreSQL (`scripts/test.ps1`) | 70 passed (core 24, config 8, observability 1, api 20 + 12 HTTP, storage 5 DB) |
| Frontend format, typecheck, lint, build | pass |
| Frontend unit (Vitest) | 29 passed |
| Playwright `ui` (mocked; states, keyboard, axe WCAG 2.2 AA, overflow at 5 viewports) | 27 passed |
| Playwright `visual` (19 screenshots, pinned container) | 19 passed against freshly recorded **candidate** baselines (re-run reproduces them exactly) |
| Playwright `e2e` (real Compose stack) | 4 passed |
| Smoke test (Compose, port 3000) | 9/9 |
| Smoke test (native backend + Vite, port 5173) | 9/9 |
| `npm audit` | 0 vulnerabilities |
| `cargo deny check` | not run locally (download of the binary was not permitted); passes in CI after replacing `chrono` (RUSTSEC-2026-0543, unmaintained) with `time` |

Measured locally (not benchmarks): backend startup to ready 20–34 ms after the database
is reachable; container images backend 123 MB, frontend 91.4 MB; idle memory backend
≈1.4 MiB, frontend ≈6.7 MiB, PostgreSQL ≈27.7 MiB.

## Limitations

* Single-operator, loopback-only; no authentication. Not approved for deployment.
* Provider state is always `NOT_INITIALIZED`; nothing is contacted.
* Visual baselines are rendered in Linux Chromium (Playwright container) and are
  candidates pending owner approval.
* `eslint-plugin-jsx-a11y` does not support ESLint 10; accessibility is enforced by axe
  in Playwright instead of lint rules.

## Owner actions

1. Review the UI and visual baselines (`frontend/tests/visual/__screenshots__/`) and
   approve or request changes (P00-T43).
2. Rotate provider credentials that were exposed during earlier manual tests.
3. Confirm `SOLANA_FALLBACK_RPC_URL` in your `.env` uses `https://` (the backend rejects
   anything else at startup).
4. Confirm the scope of roadmap phases 02–09 ([ROADMAP.md](ROADMAP.md)).
5. Decide on merging `feature/00-foundation` into `develop`.

## Acceptance criteria

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P00-T01 | Correct feature branch is active | PASS | work committed on local `phase00`, which tracks and pushes to `origin/feature/00-foundation` |
| P00-T02 | Existing `.env` is preserved and untracked | PASS | `.env` git-ignored; real `.env` never read for values or modified; `setup.ps1` merge verified on a synthetic copy |
| P00-T03 | Cargo workspace builds successfully | PASS | debug and release builds, Docker build |
| P00-T04 | Rust formatting checks pass | PASS | `cargo fmt --check` |
| P00-T05 | Clippy passes without warnings | PASS | `clippy --all-targets --all-features -D warnings` |
| P00-T06 | Backend unit tests pass | PASS | 65 unit/HTTP tests |
| P00-T07 | Backend integration tests pass | PASS | 5 PostgreSQL tests on a disposable server |
| P00-T08 | Frontend dependencies install reproducibly | PASS | `npm ci` from lockfile (host, Docker build, Playwright container) |
| P00-T09 | TypeScript strict type checking passes | PASS | `tsc -b` (strict, `exactOptionalPropertyTypes`) |
| P00-T10 | Frontend linting and formatting checks pass | PASS | ESLint strict-type-checked, Prettier |
| P00-T11 | Frontend tests pass | PASS | Vitest 29, Playwright ui 27 |
| P00-T12 | Frontend production build succeeds | PASS | `vite build`; showcase/fixtures absent from bundle |
| P00-T13 | PostgreSQL initializes successfully | PASS | Compose healthcheck, init script creates `solprint_app` |
| P00-T14 | Fresh database migrations succeed | PASS | storage tests on fresh databases; fresh Compose volume |
| P00-T15 | Repeated startup does not corrupt database state | PASS | idempotent migration test; `start_count` increments, `instance_id` stable |
| P00-T16 | Database data survives container restart | PASS | same `instance_id` across restart, rebuild and stop/start |
| P00-T17 | Liveness endpoint works | PASS | HTTP test, smoke |
| P00-T18 | Readiness reports dependency health correctly | PASS | 200 with DB, 503 without (HTTP tests), UI degraded state |
| P00-T19 | System status returns accurate information | PASS | HTTP test, smoke, e2e |
| P00-T20 | API errors are structured and sanitized | PASS | 404/405/413 envelope tests; JSON-wrapping middleware for all non-2xx |
| P00-T21 | Configuration validation rejects invalid settings | PASS | config tests (every key, all issues reported) |
| P00-T22 | Live-trading configuration is rejected | PASS | config tests; exit code 2 |
| P00-T23 | No wallet or signing functionality exists | PASS | code review; signing crates banned in `deny.toml` |
| P00-T24 | Application runs without real API credentials | PASS | CI and tests run with empty/synthetic keys |
| P00-T25 | Synthetic secrets not exposed via logs or API | PASS | config/HTTP leak tests; container log check |
| P00-T26 | Docker Compose builds and starts | PASS | all services healthy; smoke 9/9; e2e 4/4 |
| P00-T27 | Native development workflow operates correctly | PASS | native backend + Vite; smoke 9/9 on :5173 |
| P00-T28 | Application shuts down gracefully | PASS | HTTP shutdown test; SIGTERM in container exits cleanly |
| P00-T29 | Frontend communicates with the real backend | PASS | e2e against Compose; native smoke via Vite proxy |
| P00-T30 | Persistent paper-mode indicator is accurate | PASS | unit + ui tests (PAPER, UNVERIFIED when offline), visible at all viewports |
| P00-T31 | Uninitialized providers labeled truthfully | PASS | `NOT_INITIALIZED`, `last_checked_at: null`; smoke + ui tests |
| P00-T32 | All-black design tokens implemented | PASS | `index.css` tokens; [UI_DESIGN_SYSTEM](../design/UI_DESIGN_SYSTEM.md) |
| P00-T33 | No generic purple/dark-blue dashboard aesthetic | PASS | palette reset; screenshots inspected |
| P00-T34 | Responsive layouts at required viewports | PASS | no overflow at 5 viewports (ui tests); screenshots inspected |
| P00-T35 | Keyboard navigation and focus indicators | PASS | ui keyboard test (skip link, visible focus, Enter navigates) |
| P00-T36 | Accessibility checks pass | PASS | axe WCAG 2.2 AA: 0 violations on all routes and offline state |
| P00-T37 | Required UI data states implemented | PASS | loading, empty, error, stale, degraded, offline, unverified (ui tests, showcase) |
| P00-T38 | Visual screenshot candidates captured and inspected | PASS | 19 candidates captured and reviewed; two layout defects found and fixed |
| P00-T39 | GitHub Actions CI passes | PASS | run 45871846138 on `871904a`: all 6 jobs green (first run failed: unmaintained `chrono` advisory and a faulty port check; both fixed) |
| P00-T40 | Documentation complete and accurate | PASS | README, CLAUDE.md, architecture, development, design, phases |
| P00-T41 | No sensitive files staged or committed | PASS | staged file review; gitleaks in CI |
| P00-T42 | No future-phase functionality implemented | PASS | providers report state only; no ingestion/strategy/execution code |
| P00-T43 | Owner approves UI and visual baselines | PASS | approved by the owner on 2026-11-26 (after merge into `develop`), before Phase 01 started |
