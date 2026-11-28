# SOLPRINT

**On-chain trading intelligence for Solana — experimental, self-hosted.**

SOLPRINT is a self-hosted quantitative trading platform being built for Solana memecoin
markets, combining on-chain intelligence, wallet analytics and (in later phases) paper
and automated trading strategies.

## Development status

**Phase 03 — RADAR** (on `feature/03-wallet-discovery`, awaiting owner review).
Phases 00 (foundation), 01 (GATEWAY, the single read-only access layer to Helius,
Jupiter, DEX Screener and the public Solana RPC, with budgets, rate limits and caching)
and 02 (DATA ENGINE) are complete and merged into `develop`.

Phase 03 adds RADAR, a read-only wallet discovery and candidate registry. It builds a
market universe from public listings, samples supported pools through the DATA ENGINE,
registers wallets only from finalized, verified, signer-attributed swaps, investigates
history in bounded stages, and orders candidates by a documented preliminary priority.
All provider work is under GATEWAY budget admission. Autonomous discovery is **off by
default**. Priority is an investigation order, not a judgement of skill or a signal. The
read-only `/radar` workspace shows the registry, sources, budget and monitoring.

The DATA ENGINE
fetches transactions through GATEWAY, stores the raw evidence, reconstructs them, and
decodes swaps for Jupiter, Pump.fun, PumpSwap, Raydium (AMM v4, CPMM, CLMM) and Orca
Whirlpool. Each swap carries exact amounts, evidence-based wallet attribution, finality
tracking and data-quality issues. The engine also runs bounded history backfills and a
transactional event outbox, and serves a read-only Data workspace.

* SOLPRINT does **not** score tokens, generate signals or trade. Nothing is
  ingested automatically by default. Live intake and backfills are explicit, bounded
  and budgeted.
* Decoders are verified against synthetic fixtures built from published layouts, not
  against captured mainnet traffic. USD valuation is not implemented.
* Live trading is **rejected by configuration**. There are no wallets or keys, and
  GATEWAY refuses every transaction-sending or execution endpoint.
* Paid provider features stay disabled.
* It is not approved for public deployment.

Roadmap: [docs/phases/ROADMAP.md](docs/phases/ROADMAP.md) · Phase details:
[PHASE_00.md](docs/phases/PHASE_00.md), [PHASE_01.md](docs/phases/PHASE_01.md),
[PHASE_02.md](docs/phases/PHASE_02.md), [PHASE_03.md](docs/phases/PHASE_03.md) · GATEWAY:
[docs/architecture/GATEWAY.md](docs/architecture/GATEWAY.md) · DATA ENGINE:
[docs/architecture/DATA_ENGINE.md](docs/architecture/DATA_ENGINE.md) · RADAR:
[docs/architecture/RADAR.md](docs/architecture/RADAR.md).

## Technology

| Layer | Stack |
|---|---|
| Backend | Rust 1.99 (edition 2024), Tokio, Axum, SQLx, reqwest + Rustls, tokio-tungstenite, moka, tracing, ts-rs |
| Database | PostgreSQL 18 |
| Frontend | React 19, TypeScript (strict), Vite, Tailwind CSS 4, TanStack Query, Radix |
| Testing | cargo test, Vitest, Testing Library, Playwright, axe-core |
| Infrastructure | Docker Compose, nginx, local verification scripts (no hosted CI) |

## Architecture

A modular monolith: nine backend crates (`core`, `config`, `observability`, `storage`,
`gateway`, `data-engine`, `radar`, `api`, `app`) behind a same-origin reverse proxy, with API types generated from Rust into
TypeScript. See [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md).

```
browser ─▶ frontend (nginx / Vite) ─▶ backend (Axum) ─▶ PostgreSQL
                                          ├─ RADAR ─▶ DATA ENGINE
                                          ├─ DATA ENGINE ─▶ (via GATEWAY)
                                          └─ GATEWAY ─▶ Helius · Jupiter · DEX Screener · Solana RPC
```

## Prerequisites

Windows with PowerShell 7, Git, Docker Desktop, Rust via rustup (the toolchain is pinned
by `rust-toolchain.toml`) and Node.js 24. Details:
[docs/development/SETUP.md](docs/development/SETUP.md).

## Setup and development

```powershell
./scripts/setup.ps1               # check tools, create/merge .env, npm ci
./scripts/dev.ps1                 # Postgres in Docker + native backend and Vite (http://127.0.0.1:5173)
./scripts/dev.ps1 -Mode docker    # full stack in Compose (http://127.0.0.1:3000)
./scripts/smoke-test.ps1          # HTTP smoke test of a running stack
./scripts/gateway-diagnostics.ps1 # GATEWAY configuration report (contacts nothing)
./scripts/gateway-diagnostics.ps1 -Live  # explicit opt-in: a few bounded read-only provider calls
./scripts/radar-diagnostics.ps1   # RADAR policy tests and local registry state (contacts nothing)
./scripts/stop.ps1                # stop containers (data is kept)
```

Configuration reference: [docs/development/CONFIGURATION.md](docs/development/CONFIGURATION.md).

## Testing

```powershell
./scripts/verify.ps1                  # THE gate: exact commit, clean checkout, everything below,
                                      # advisories, secret scan, clean Docker builds, Compose smoke
./scripts/check.ps1                   # format, lint, types, unit tests, contracts, build
./scripts/test.ps1                    # + PostgreSQL integration tests (disposable DB)
./scripts/playwright.ps1 -Project ui  # states, keyboard, accessibility, responsive
./scripts/playwright.ps1 -Project visual
./scripts/playwright.ps1 -Project e2e # against the running Compose stack
```

Verification is local only; there is no hosted CI/CD. See
[docs/development/TESTING.md](docs/development/TESTING.md).

## Design

The TERMINAL interface follows the [UI/UX Design Specification](docs/design/UI_UX_SPEC.md)
and the [UI Design Directive](docs/design/UI_DESIGN_DIRECTIVE.md); the implemented system
is in [UI_DESIGN_SYSTEM.md](docs/design/UI_DESIGN_SYSTEM.md). UI quality is a
cross-cutting roadmap requirement — see
[Roadmap UI Integration](docs/design/ROADMAP_UI_INTEGRATION.md).

## Branches

- `main` — stable releases
- `develop` — integration
- `release` — release preparation
- `feature/NN-<name>` — phase work (Phase 01: `feature/01-gateway`)

Merges into `develop` and releases are manual and owner-approved.

## Disclaimer

Experimental software for research and educational purposes. It is not financial advice
and makes no claim of profitability. Trading digital assets involves significant
financial risk, including total loss.
