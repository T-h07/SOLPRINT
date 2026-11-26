# Development setup (Windows)

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.99.1 | installed automatically by rustup from `rust-toolchain.toml` |
| Node.js | 24 LTS (`frontend/.nvmrc`) | npm 11 |
| Docker Desktop | Engine 29+, Compose v2 | PostgreSQL and the full stack |
| PowerShell | 7+ | all scripts are `scripts/*.ps1` |
| Git | any recent | |

Nothing else is installed globally. Playwright browsers run inside the pinned
`mcr.microsoft.com/playwright` container, not on the host.

## First run

```powershell
./scripts/setup.ps1
```

`setup.ps1` checks the toolchain, creates `.env` from `.env.example` **or merges missing
keys into an existing `.env`** (existing values are never changed, reordered or printed;
the original is backed up to `.env.backup-<timestamp>` first), generates random database
passwords when missing, keeps `DATABASE_URL` consistent, and runs `npm ci`.

## Run

```powershell
./scripts/dev.ps1                # native: Postgres in Docker, backend + Vite locally
./scripts/dev.ps1 -Mode docker   # full stack in Compose
./scripts/dev.ps1 -Mode db       # PostgreSQL only
```

| Workflow | UI | API |
|---|---|---|
| native | http://127.0.0.1:5173 | proxied by Vite to 127.0.0.1:8080 |
| docker | http://127.0.0.1:3000 | proxied by nginx to the `backend` service |

```powershell
./scripts/smoke-test.ps1                                   # against Compose
./scripts/smoke-test.ps1 -BaseUrl http://127.0.0.1:5173    # against native
./scripts/stop.ps1                                         # stop containers, keep data
```

`stop.ps1` never deletes volumes. To deliberately wipe the local database, run
`docker compose down -v` yourself.

## Checks before pushing

```powershell
./scripts/check.ps1                 # fmt, clippy, tests, contracts, frontend checks
./scripts/test.ps1                  # + PostgreSQL integration tests (disposable DB)
./scripts/playwright.ps1 -Project ui
./scripts/playwright.ps1 -Project visual
```

See [TESTING.md](TESTING.md), [CONFIGURATION.md](CONFIGURATION.md) and
[TROUBLESHOOTING.md](TROUBLESHOOTING.md).
