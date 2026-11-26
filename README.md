# SOLPRINT

**On-chain trading intelligence for Solana — experimental, self-hosted.**

SOLPRINT is a self-hosted quantitative trading platform being built for Solana memecoin
markets, combining on-chain intelligence, wallet analytics and (in later phases) paper
and automated trading strategies.

## Development status

**Phase 00 — Foundation.** The repository contains the engineering foundation only:
Rust backend workspace, PostgreSQL with migrations, health/status API, structured
logging, Docker Compose, the TERMINAL shell with its design system, tests and CI.

* It does **not** observe markets, contact providers, analyse tokens or trade.
* Live trading is **rejected by configuration**; there are no wallets or keys.
* It is not approved for public deployment.

Roadmap: [docs/phases/ROADMAP.md](docs/phases/ROADMAP.md) · Phase 00 details:
[docs/phases/PHASE_00.md](docs/phases/PHASE_00.md).

## Technology

| Layer | Stack |
|---|---|
| Backend | Rust 1.99 (edition 2024), Tokio, Axum, SQLx, tracing, ts-rs |
| Database | PostgreSQL 18 |
| Frontend | React 19, TypeScript (strict), Vite, Tailwind CSS 4, TanStack Query, Radix |
| Testing | cargo test, Vitest, Testing Library, Playwright, axe-core |
| Infrastructure | Docker Compose, nginx, GitHub Actions |

## Architecture

A modular monolith: six backend crates (`core`, `config`, `observability`, `storage`,
`api`, `app`) behind a same-origin reverse proxy, with API types generated from Rust into
TypeScript. See [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md).

```
browser ─▶ frontend (nginx / Vite) ─▶ backend (Axum) ─▶ PostgreSQL
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
./scripts/stop.ps1                # stop containers (data is kept)
```

Configuration reference: [docs/development/CONFIGURATION.md](docs/development/CONFIGURATION.md).

## Testing

```powershell
./scripts/check.ps1                   # format, lint, types, unit tests, contracts, build
./scripts/test.ps1                    # + PostgreSQL integration tests (disposable DB)
./scripts/playwright.ps1 -Project ui  # states, keyboard, accessibility, responsive
./scripts/playwright.ps1 -Project visual
./scripts/playwright.ps1 -Project e2e # against the running Compose stack
```

See [docs/development/TESTING.md](docs/development/TESTING.md).

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
- `feature/NN-<name>` — phase work (Phase 00: `feature/00-foundation`)

## Disclaimer

Experimental software for research and educational purposes. It is not financial advice
and makes no claim of profitability. Trading digital assets involves significant
financial risk, including total loss.
