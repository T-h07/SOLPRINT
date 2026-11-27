# Troubleshooting

**Backend exits with code 2 at startup.** Configuration is invalid. The log lists every
offending key (never values). Common causes: `TRADING_MODE=live` or
`LIVE_TRADING_ENABLED=true` (rejected by design in Phase 00), an `http://` provider URL,
missing `DATABASE_URL`.

**`.env missing. Run scripts/setup.ps1 first.`** Run it; it merges into an existing
`.env` and never overwrites values.

**Docker engine not reachable.** Start Docker Desktop and wait until it reports
"Engine running".

**Port already in use (5432/55432/3000/5173/8080).** Change `POSTGRES_HOST_PORT`,
`SOLPRINT_WEB_PORT` or `APP_PORT` in `.env`. Vite uses `strictPort` and will not silently
move to another port.

**Backend cannot authenticate to PostgreSQL after changing `SOLPRINT_DB_PASSWORD`.**
The role password is set only when the volume is first initialised. Either restore the
previous value, or change the role password:
`docker compose exec postgres psql -U solprint_admin -d postgres -c "ALTER ROLE solprint_app PASSWORD '<new>'"`
(type it in your own terminal), or, if you do not need the data, wipe the volume yourself
with `docker compose down -v`.

**Readiness is 503 / UI shows DEGRADED.** The backend is up but the database probe
failed. Check `docker compose ps` and `docker compose logs postgres`.

**UI shows MODE UNVERIFIED / BACKEND OFFLINE.** The browser cannot reach the backend
through the proxy. Native: is `cargo run` running on 8080? Compose: is the `backend`
container healthy?

**`/design-system` shows "Not found".** Expected in production builds. Use the Vite dev
server or `npm run build:showcase`.

**Visual tests differ between machines.** Run them only through `scripts/playwright.ps1`
(pinned container). Host-rendered screenshots differ in fonts.

**Generated contract drift (`check.ps1` / `verify.ps1`).** Run `cd backend; cargo test -p solprint-api` and
commit `frontend/src/types/generated`.

**Git Bash rewrites container paths** (e.g. `/var/lib/postgresql`). Prefer PowerShell
scripts, or prefix with `MSYS_NO_PATHCONV=1`.

## GATEWAY

**A provider shows `NOT MEASURED` / `UNKNOWN`.** Expected until something requests data
from it: GATEWAY has no background probes. Run `./scripts/gateway-diagnostics.ps1 -Live`
(explicit opt-in, a few bounded read-only calls) if you want a current check.

**A provider shows `DISABLED`.** It is turned off in configuration or its credential is
missing. `./scripts/gateway-diagnostics.ps1` (offline) lists which credentials are
present, by name only.

**Requests fail with `BUDGET_DENIED`.** The Helius budget state no longer admits that
priority (see [API_BUDGET_POLICY.md](../architecture/API_BUDGET_POLICY.md)). Check the
Budget pane. Do not raise `HELIUS_MONTHLY_CREDITS` above what the plan actually includes.

**Budget shows `ACCOUNT_USAGE_UNKNOWN`.** The stored ledger could not be loaded
(database unavailable at start). Only P0–P2 are admitted until it loads; fix the
database connection.

**`RATE_LIMITED` / circuit `OPEN`.** The provider answered 429 or failed repeatedly.
GATEWAY honours `Retry-After` and backs off by itself; the circuit retries with one probe
after 30 s (doubling to 5 min). Lower the configured rate if this recurs.

**Streams stay `DISABLED`.** `GATEWAY_STREAMS_ENABLED` is `false` by default, and streams
also need `HELIUS_API_KEY`. `BUDGET_EXHAUSTED` means the budget no longer admits P2.

**A stream shows `GAP` or `CONTINUITY UNKNOWN`.** Events may be missing after a
reconnect or because a consumer fell behind. This is reported on purpose; see
[STREAM_RECOVERY.md](../architecture/STREAM_RECOVERY.md).

**`scripts/verify.ps1` reports `BLOCKED`.** A required tool is missing (for example
`cargo-deny`) or Docker is not running. BLOCKED is not a pass; install the tool or start
Docker and re-run. A leftover verification project can be removed with
`docker compose -p solprint-verify-<sha12> down --volumes --rmi local` (it never touches
the `solprint` project).
