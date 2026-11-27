# Security model

## Phase 00 guarantees

1. **Non-executing.** No code path signs, builds or submits a transaction. There are no
   wallets, keys or seed phrases in the system and the configuration has no field for
   them. Signing/wallet crates are banned in `backend/deny.toml`.
2. **Live trading is rejected at startup.** `TRADING_MODE=live` or
   `LIVE_TRADING_ENABLED=true` fails configuration validation
   (`LIVE_TRADING_NOT_PERMITTED`), exit code 2. The status API reports
   `execution_capability: NONE`.
3. **No provider is contacted.** Provider credentials may be present but are only
   reported as configured/absent.

## Secrets

* Secrets come from the process environment or the root `.env` (git-ignored; never
  copied into an image).
* `Secret<T>` redacts `Debug` and has no `Display`; configuration errors name the key,
  never the value. `SOLANA_FALLBACK_RPC_URL` is treated as a secret because RPC URLs
  often embed API keys.
* Never logged: API keys, `Authorization` headers, database passwords, URLs with
  credentials, private keys, recovery phrases, raw environment dumps. Tests assert that
  synthetic secrets do not appear in logs or HTTP responses; `scripts/verify.ps1` checks
  container logs for the generated database passwords.
* Database errors are sanitised before logging (no connection strings).
* `scripts/verify.ps1` runs gitleaks (pinned image, offline, redacted) over the candidate
  revision, the staged changes and the full history.
* Provider credentials (Helius, Jupiter) are used only inside GATEWAY, wrapped in
  `Secret<T>` until a request is built; URLs carrying keys are never logged, stored or
  returned, and provider error messages are sanitised. Credentials were exposed in an
  earlier conversation and their rotation has not been independently confirmed:
  **rotation is an owner responsibility.**

## Outbound provider access (GATEWAY)

* Only GATEWAY makes outbound requests, only to the four configured origins (Helius,
  Jupiter, DEX Screener, Solana public RPC), over TLS (Rustls, WebPKI roots).
* Transaction-sending JSON-RPC methods and execution/submission endpoints are refused
  in code, independent of configuration; there is no generic proxy.
* Monitoring endpoints read local state only; viewing the dashboard causes no provider
  traffic.
* Live provider calls outside normal operation happen only through
  `scripts/gateway-diagnostics.ps1 -Live` (explicit opt-in, bounded, read-only).

## Network exposure (Compose)

| Service | Published | Notes |
|---|---|---|
| frontend | `127.0.0.1:3000` | nginx, same-origin proxy to the backend |
| backend | not published | reachable only on the Compose network |
| postgres | `127.0.0.1:55432` | for the native workflow; SCRAM-SHA-256 auth |

The native backend binds `127.0.0.1` and refuses any other address unless
`APP_ALLOW_NON_LOOPBACK=true` (set only inside the container image).

## Containers

Non-root users (backend uid 10001; nginx-unprivileged), read-only root filesystems,
`cap_drop: [ALL]`, `no-new-privileges`, pinned base images, bounded json-file logs,
healthchecks. The backend image contains only the static binary on a slim Debian runtime.

## HTTP hardening

`X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy:
no-referrer`, `Cache-Control: no-store` on API responses, a `default-src 'none'` CSP on
API responses and a strict CSP on the frontend, `Cross-Origin-Resource-Policy:
same-origin`, body size limit, request timeout, structured errors without internals.

## Database

The application connects as `solprint_app` (not a superuser, owns only the `solprint`
database). The admin role is used only by the Postgres image's first-run initialisation.
Integration tests use a disposable server and create/drop uniquely named databases; they
never touch the development database.

## Supply chain

Rust: `Cargo.lock` committed, `--locked` builds, cargo-deny (advisories, licenses, bans,
sources). Frontend: `package-lock.json`, `npm ci`, `npm audit --audit-level=high`. All of these run locally
in `scripts/verify.ps1`; SOLPRINT uses no hosted CI/CD (the Phase 00 GitHub Actions
workflow was removed in Phase 01). Container images and tool images are pinned by tag.

**Advisory policy:** any RustSec vulnerability/unsound advisory or any high/critical npm
advisory fails `scripts/verify.ps1`. Exceptions require the advisory ID, a written justification, a review
date and owner approval, recorded in `deny.toml` (Rust) or this document (npm).

## Owner actions (outside the repository)

* **Rotate previously exposed provider credentials.** Credentials that were shared
  during earlier manual provider tests (Helius, Jupiter, any RPC URL containing a key)
  should be treated as exposed and rotated at the provider, then updated in `.env`.
  Nothing in this repository reads or prints their values.
* **Check `SOLANA_FALLBACK_RPC_URL` in your `.env` uses `https://`.** The backend rejects
  any other scheme at startup.
* Keep `.env` out of backups that sync to third-party services, or encrypt them.

## Out of scope / not approved

Public deployment, TLS termination, authentication/authorisation for multiple users,
key management and execution. These are designed in later phases (12–13 hardening) and
require explicit approval.
