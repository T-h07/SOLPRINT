# Configuration

Precedence: **process environment > `.env` > built-in default.** The backend loads
`SOLPRINT_ENV_FILE` if set, otherwise `./.env`, otherwise `../.env`. A missing file is
fine; an unreadable or malformed one is an error.

Validation collects **every** problem and reports them by key name; values are never
printed. Any problem → exit code 2.

## Backend

| Key | Default | Rules |
|---|---|---|
| `APP_ENV` | `development` | `development` \| `test` \| `production` |
| `APP_HOST` | `127.0.0.1` | IP address; non-loopback requires `APP_ALLOW_NON_LOOPBACK=true` |
| `APP_ALLOW_NON_LOOPBACK` | `false` | set to `true` only inside containers |
| `APP_PORT` | `8080` | 1–65535 |
| `HTTP_REQUEST_TIMEOUT_SECS` | `10` | 1–120 |
| `HTTP_BODY_LIMIT_BYTES` | `65536` | 1 KiB–1 MiB |
| `DATABASE_URL` | — (required) | `postgres://` or `postgresql://`; treated as a secret |
| `DATABASE_MAX_CONNECTIONS` | `5` | 1–50 |
| `DATABASE_ACQUIRE_TIMEOUT_SECS` | `5` | 1–60 |
| `DATABASE_STARTUP_TIMEOUT_SECS` | `30` | 0–300; how long startup retries the database |
| `DATABASE_RUN_MIGRATIONS` | `true` | `false` = verify only, fail if any are pending |
| `LOG_LEVEL` | `info` | `error` \| `warn` \| `info` \| `debug` \| `trace` |
| `LOG_FORMAT` | `pretty` in development, `json` otherwise | `pretty` \| `json` |
| `TRADING_MODE` | `paper` | **only `paper`**; `live` is rejected (`LIVE_TRADING_NOT_PERMITTED`) |
| `LIVE_TRADING_ENABLED` | `false` | **only `false`**; `true` is rejected |
| `HELIUS_API_KEY` | empty | optional, secret; reported only as configured/absent. Without it Helius RPC and streams are disabled |
| `JUPITER_API_KEY` | empty | optional, secret. Without it Jupiter runs on the keyless tier (rate capped at 30/min) |
| `SOLPRINT_GIT_COMMIT` | — | build-time; accepted only as 7–40 hex characters |

Exact ranges are enforced in `backend/crates/solprint-config/src/lib.rs`; if this table
and the code disagree, the code is authoritative and this table is a bug.

## GATEWAY (Phase 01)

Defaults are deliberately below the providers' documented limits (see
[PROVIDER_CAPABILITIES.md](../architecture/PROVIDER_CAPABILITIES.md)). Do not raise a
limit above what the provider documents, and do not raise `HELIUS_MONTHLY_CREDITS` above
what the plan includes: GATEWAY never enables paid usage.

| Key | Default | Rules |
|---|---|---|
| `GATEWAY_ENABLED` | `true` | `false` turns every provider off (the API still reports state) |
| `GATEWAY_DEFAULT_DEADLINE_MS` | `10000` | 100–60 000; covers queueing, rate-limit waits and retries |
| `GATEWAY_CONNECT_TIMEOUT_MS` | `5000` | 100–30 000 |
| `GATEWAY_GLOBAL_QUEUE_LIMIT` | `512` | 1–4 096; must be ≥ `GATEWAY_PROVIDER_QUEUE_LIMIT` |
| `GATEWAY_PROVIDER_QUEUE_LIMIT` | `128` | 1–1 024 |
| `GATEWAY_MAX_RESPONSE_BYTES` | `8388608` | 64 KiB–64 MiB |
| `GATEWAY_CACHE_MAX_ENTRIES` | `10000` | 0–1 000 000; `0` disables the cache |
| `GATEWAY_FLUSH_INTERVAL_SECS` | `30` | 5–600; usage/metrics persistence interval |
| `GATEWAY_USAGE_RETENTION_DAYS` | `90` | 1–3 650 |
| `GATEWAY_METRICS_RETENTION_DAYS` | `14` | 1–365 |
| `GATEWAY_STREAMS_ENABLED` | `false` | Helius WebSocket streams (need `HELIUS_API_KEY`) |
| `HELIUS_RPC_URL` | `https://mainnet.helius-rpc.com` | `https://`; no query or user info (the key comes from `HELIUS_API_KEY`) |
| `HELIUS_WS_URL` | `wss://mainnet.helius-rpc.com` | `wss://`; no query or user info |
| `HELIUS_MONTHLY_CREDITS` | `1000000` | 0–1 000 000 000; the plan's credits per cycle |
| `HELIUS_RESERVE_PERCENT` | `20` | 0–90; share reserved for P0–P1 |
| `HELIUS_BILLING_CYCLE_START_DAY` | `1` | 1–28 (UTC) |
| `HELIUS_RPC_RPS` | `5` | 1–10 |
| `HELIUS_MAX_IN_FLIGHT` | `4` | 1–16 |
| `HELIUS_WS_MAX_SUBSCRIPTIONS` | `100` | 1–1 000 |
| `HELIUS_GET_BLOCK_ENABLED` | `false` | getBlock is large and rarely needed |
| `JUPITER_BASE_URL` | `https://api.jup.ag` | `https://` only |
| `JUPITER_RPM` | `50` | 1–60, and at most 30 without `JUPITER_API_KEY` |
| `JUPITER_MAX_IN_FLIGHT` | `2` | 1–4 |
| `JUPITER_QUOTE_ENABLED` | `true` | quote-only Swap V2 `/order` (no taker, never executes) |
| `DEXSCREENER_BASE_URL` | `https://api.dexscreener.com` | `https://` only |
| `DEXSCREENER_RPM` | `240` | 1–300 |
| `DEXSCREENER_MAX_IN_FLIGHT` | `4` | 1–8 |
| `DEXSCREENER_DISCOVERY_ENABLED` | `false` | token profiles and boosts endpoints |
| `SOLANA_FALLBACK_RPC_URL` | `https://api.mainnet.solana.com` | `https://` only; secret (may embed a key) |
| `SOLANA_FALLBACK_ENABLED` | `true` | public RPC as fallback only, when the caller allows it |
| `SOLANA_FALLBACK_RPS` | `2` | 1–4 |

In Compose these keys are passed through with an empty default, which means "use the
backend default".

## DATA ENGINE (Phase 02)

The engine is read-only: it fetches, decodes and stores transactions and never signs or
submits anything. Every provider request goes through GATEWAY's budget and rate limits.
Live intake and automatic ingestion are off by default. See
[DATA_ENGINE.md](../architecture/DATA_ENGINE.md).

| Key | Default | Rules |
|---|---|---|
| `DATA_ENGINE_ENABLED` | `true` | `false` = no workers, reconciler, backfills or intake; the API still reads stored data |
| `DATA_ENGINE_AUTO_INGEST_ENABLED` | `false` | live intake from GATEWAY `logsSubscribe` for enabled wallet targets (also needs `GATEWAY_STREAMS_ENABLED`) |
| `DATA_ENGINE_MAX_ACTIVE_TARGETS` | `25` | 1–100; must not exceed `HELIUS_WS_MAX_SUBSCRIPTIONS` |
| `DATA_ENGINE_FETCH_CONCURRENCY` | `4` | 1–16; must not exceed `HELIUS_MAX_IN_FLIGHT` |
| `DATA_ENGINE_DECODE_CONCURRENCY` | `4` | 1–16; job workers |
| `DATA_ENGINE_QUEUE_CAPACITY` | `2048` | 16–100 000 non-terminal jobs; above it intake is refused and backfills pause |
| `DATA_ENGINE_JOB_LEASE_SECONDS` | `60` | 10–600 |
| `DATA_ENGINE_JOB_MAX_ATTEMPTS` | `5` | 1–20 |
| `DATA_ENGINE_DEFAULT_COMMITMENT` | `confirmed` | `confirmed` \| `finalized` (`getTransaction` rejects processed) |
| `DATA_ENGINE_FINALITY_RECONCILE_INTERVAL_SECS` | `15` | 5–3 600 |
| `DATA_ENGINE_FINALITY_BATCH_SIZE` | `64` | 1–256 signatures per `getSignatureStatuses` |
| `DATA_ENGINE_BACKFILL_ENABLED` | `true` | backfills run only when the owner created one (`backfill create --yes`) |
| `DATA_ENGINE_MAX_CONCURRENT_BACKFILLS` | `1` | 1–2 |
| `DATA_ENGINE_BACKFILL_PAGE_SIZE` | `100` | 1–1 000 |
| `DATA_ENGINE_BACKFILL_MAX_PAGES` | `10` | 1–100 |
| `DATA_ENGINE_BACKFILL_MAX_SIGNATURES` | `1000` | 1–10 000 |
| `DATA_ENGINE_PRICE_ENRICHMENT_ENABLED` | `false` | USD valuation is **not implemented** in Phase 02; swaps report `NOT_ENRICHED`, or `PRICE_UNAVAILABLE` when this is `true` |
| `DATA_ENGINE_PRICE_MAX_AGE_SECS` | `15` | 1–300; reserved for point-in-time valuation |
| `DATA_ENGINE_OUTBOX_BATCH_SIZE` | `100` | 1–1 000 |
| `DATA_ENGINE_OUTBOX_POLL_INTERVAL_MS` | `250` | 50–10 000 |
| `DATA_ENGINE_METRICS_REFRESH_SECS` | `15` | 1–300 |
| `DATA_ENGINE_EVIDENCE_MAX_BYTES` | `1048576` | 64 KiB–8 MiB; must not exceed `GATEWAY_MAX_RESPONSE_BYTES` |
| `DATA_ENGINE_EVIDENCE_RETENTION_DAYS` | `90` | 0–3 650; prunes raw JSON of finalized transactions (hash kept); `0` never prunes |
| `DATA_ENGINE_JOB_RETENTION_DAYS` | `14` | 1–365; `COMPLETED` and `CANCELLED` jobs |
| `DATA_ENGINE_OUTBOX_RETENTION_DAYS` | `7` | 1–365; events every consumer has delivered |

## Compose / scripts

| Key | Default | Purpose |
|---|---|---|
| `POSTGRES_ADMIN_PASSWORD` | generated by `setup.ps1` | bootstrap superuser (first run only) |
| `SOLPRINT_DB_PASSWORD` | generated by `setup.ps1` | application role `solprint_app` |
| `POSTGRES_HOST_PORT` | `55432` | loopback port for the native workflow |
| `SOLPRINT_WEB_PORT` | `3000` | loopback port for the frontend container |

Changing `SOLPRINT_DB_PASSWORD` after the database volume exists does not change the
role's password (Postgres initialisation runs once). See
[TROUBLESHOOTING.md](TROUBLESHOOTING.md).

## Frontend (build time)

| Key | Default | Purpose |
|---|---|---|
| `SOLPRINT_BACKEND_URL` | `http://127.0.0.1:8080` | Vite dev/preview proxy target |
| `VITE_ENABLE_DESIGN_SYSTEM` | `false` | include `/design-system` in a production build (never for deployment) |
