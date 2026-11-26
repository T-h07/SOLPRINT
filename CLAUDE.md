# SOLPRINT — Claude Code instructions

SOLPRINT is a self-hosted, modular trading-intelligence platform for Solana markets,
built in phases (see [docs/phases/ROADMAP.md](docs/phases/ROADMAP.md)). It is
experimental software: never describe it as profitable, autonomous or production-ready.

## Scope discipline

- Work only on the current phase. Do not implement future-phase functionality, start
  the next phase, or "prepare" modules that have no approved scope.
- Phase 00 is **non-executing**: no transaction building, signing or submission, no
  wallets, keys or seed phrases. Never ask for private keys or recovery phrases.
- Do not contact external providers unless the phase requires it; never present
  historical measurements as current.
- No speculative CORTEX formulas or trading thresholds.

## Architecture principles

- Modular, event-driven **monolith** in one Rust workspace (`backend/crates/*`). No
  microservices, brokers, Redis or Kubernetes without a documented, measured need.
- Dependencies point inward to `solprint-core` (no I/O). Only `solprint-app` composes;
  only `solprint-storage` speaks SQL; only `solprint-api` defines wire contracts. See
  [docs/architecture/MODULE_BOUNDARIES.md](docs/architecture/MODULE_BOUNDARIES.md).
- The backend is authoritative for operating mode and every displayed value. API types
  are generated Rust → TypeScript (`cargo test -p solprint-api`); never edit
  `frontend/src/types/generated` by hand.
- Migrations are forward-only; never edit a migration that has been pushed.

## Financial precision

- No floating point for amounts or prices. Lamports/raw token amounts are `u64` with
  checked arithmetic and travel as strings; prices are `Decimal` strings; the frontend
  formats with `BigInt`. Rounding direction is always explicit.
- Timestamps are UTC RFC 3339 (ms, `Z`); events keep source/observed/processed times.
  Details: [docs/architecture/DATA_CONVENTIONS.md](docs/architecture/DATA_CONVENTIONS.md).

## Security requirements

- Never read, print, modify, overwrite or commit the real `.env`. Inspect key names only
  if needed. `scripts/setup.ps1` merges keys and backs up first.
- Never log or return secrets: API keys, Authorization headers, DB passwords, URLs with
  credentials, keys, recovery phrases, raw environment. Wrap secrets in `Secret<T>`.
- Live trading must stay rejected by configuration until an approved execution phase.
- Loopback-only exposure; non-root, read-only, capability-free containers.
- Never run destructive database operations against the development database; tests
  use `scripts/test.ps1` (disposable container). Never run `docker compose down -v`
  automatically.
- Pin versions (Cargo.lock, package-lock.json, image tags, action SHAs).
  Details: [docs/architecture/SECURITY_MODEL.md](docs/architecture/SECURITY_MODEL.md).

## Frontend / UI work

Any task that creates or materially changes frontend UI must follow
[docs/design/UI_DESIGN_DIRECTIVE.md](docs/design/UI_DESIGN_DIRECTIVE.md) (mandatory) and
[docs/design/UI_UX_SPEC.md](docs/design/UI_UX_SPEC.md). Read both before writing UI code.
The implemented system is documented in
[docs/design/UI_DESIGN_SYSTEM.md](docs/design/UI_DESIGN_SYSTEM.md).

Non-negotiables:

- All-black neutral theme; color only where it carries meaning, never color alone.
- Tabular numerals for all financial and operational numbers.
- PAPER / LIVE state persistently visible; live activation and emergency stop are
  safety-critical UI.
- Every data component handles loading, empty, error, stale and partial states. Never
  fabricate production values; fixtures are labeled `FIXTURE` / `DEMO DATA`.
- Change styling through shared tokens/components, not local one-off styles. Do not
  redesign surrounding screens opportunistically.
- A UI task is not complete until it has been rendered at the standard viewports and
  checked against the quality gates in the spec (§8–9).

## Testing requirements

Before declaring work done, run and report honestly (never claim a pass without output):

- `./scripts/check.ps1` — fmt, clippy `-D warnings`, tests, contract drift, frontend
  format/types/lint/unit/build.
- `./scripts/test.ps1` — adds PostgreSQL integration tests on a disposable database.
- `./scripts/playwright.ps1 -Project ui` / `-Project visual` — UI states, keyboard, axe,
  responsive, screenshots (pinned container only).
- `./scripts/smoke-test.ps1` and `./scripts/playwright.ps1 -Project e2e` against a
  running stack when backend or infrastructure changed.

Visual baselines are candidates until the owner approves them; never update snapshots
blindly or disable a failing visual test.

## Branch workflow

- Phase work happens on `feature/NN-<name>` (Phase 00: `feature/00-foundation`).
- Never commit to, reset, force-push or rewrite `main`, `develop` or `release`; never
  merge into `develop` without the owner; never create release or production tags.
- Small, logical commits with conventional prefixes (`feat`, `fix`, `docs`, `test`,
  `build`, `ci`, `chore`). Never commit secrets, `.env`, dumps or build output.

## Documentation conventions

- Docs live in `docs/architecture`, `docs/development`, `docs/design`, `docs/phases`.
- Document what exists, not what is planned, unless clearly labelled as planned.
- Never put credentials or sensitive operational details in docs; do not invent figures.
- Update the relevant doc in the same change as the code it describes.

## Acceptance and review

Each phase has numbered acceptance criteria recorded in `docs/phases/PHASE_NN.md`
(PASS / FAIL / BLOCKED / NOT RUN / PENDING OWNER REVIEW, with evidence). Owner approval
criteria stay pending until the owner explicitly approves. A phase is complete only when
all criteria pass and the owner signs off.
