# SOLPRINT

**On-chain trading intelligence for Solana — experimental, self-hosted.**

SOLPRINT is a self-hosted quantitative trading platform being built for Solana memecoin
markets, combining on-chain intelligence, wallet analytics and (in later phases) paper
and automated trading strategies.

## Development status

**Phase 01 — GATEWAY** (on `feature/01-gateway`, awaiting owner review). Phase 00 (the
engineering foundation: Rust workspace, PostgreSQL, health/status API, logging, Docker
Compose, the TERMINAL shell and design system, tests) is complete. Phase 01 adds
GATEWAY, the single read-only access layer to Helius (RPC and WebSocket), Jupiter
(prices, tokens, quotes only), DEX Screener and the public Solana RPC as a fallback,
with credit budgets, rate limits, caching, circuit breakers, persisted usage telemetry
and a Gateway monitoring page.

* It does **not** analyse tokens, rank wallets, generate signals or trade. GATEWAY makes
  provider requests only when asked; nothing in Phase 01 polls providers on its own.
* Live trading is **rejected by configuration**; there are no wallets or keys, and
  GATEWAY refuses every transaction-sending or execution endpoint.
* Paid provider features stay disabled.
* It is not approved for public deployment.

Roadmap: [docs/phases/ROADMAP.md](docs/phases/ROADMAP.md) · Phase details:
[PHASE_00.md](docs/phases/PHASE_00.md), [PHASE_01.md](docs/phases/PHASE_01.md) ·
GATEWAY: [docs/architecture/GATEWAY.md](docs/architecture/GATEWAY.md).

## Technology

| Layer | Stack |
|---|---|
| Backend | Rust 1.99 (edition 2024), Tokio, Axum, SQLx, reqwest + Rustls, tokio-tungstenite, moka, tracing, ts-rs |
| Database | PostgreSQL 18 |
| Frontend | React 19, TypeScript (strict), Vite, Tailwind CSS 4, TanStack Query, Radix |
| Testing | cargo test, Vitest, Testing Library, Playwright, axe-core |
| Infrastructure | Docker Compose, nginx, local verification scripts (no hosted CI) |

## Architecture

A modular monolith: seven backend crates (`core`, `config`, `observability`, `storage`,
`gateway`, `api`, `app`) behind a same-origin reverse proxy, with API types generated from Rust into
TypeScript. See [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md).

```
browser ─▶ frontend (nginx / Vite) ─▶ backend (Axum) ─▶ PostgreSQL
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
