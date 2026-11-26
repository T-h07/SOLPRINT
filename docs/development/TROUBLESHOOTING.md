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

**Visual tests fail locally but not in CI (or vice versa).** Run them only through
`scripts/playwright.ps1` (pinned container). Host-rendered screenshots differ in fonts.

**Generated contract drift in CI.** Run `cd backend; cargo test -p solprint-api` and
commit `frontend/src/types/generated`.

**Git Bash rewrites container paths** (e.g. `/var/lib/postgresql`). Prefer PowerShell
scripts, or prefix with `MSYS_NO_PATHCONV=1`.
