# Testing

| Suite | Command | Needs |
|---|---|---|
| Rust unit + HTTP | `cd backend; cargo test --workspace --all-features` | — |
| Rust + PostgreSQL integration | `./scripts/test.ps1` | Docker |
| Frontend unit (Vitest, jsdom) | `cd frontend; npm test` / `npm run test:ci` | — |
| UI: states, keyboard, axe, overflow | `./scripts/playwright.ps1 -Project ui` | Docker |
| Visual regression | `./scripts/playwright.ps1 -Project visual` | Docker |
| End-to-end (real stack) | `./scripts/dev.ps1 -Mode docker; ./scripts/playwright.ps1 -Project e2e` | Docker |
| HTTP smoke | `./scripts/smoke-test.ps1` | running stack |
| Everything CI runs without containers | `./scripts/check.ps1` | — |

## Database tests never touch your data

PostgreSQL tests are `#[ignore]` by default and need `SOLPRINT_TEST_DATABASE_URL`.
`scripts/test.ps1` starts a throwaway `solprint-test-db` container (tmpfs storage, random
password, loopback port 55433), runs the tests with `--include-ignored`, and removes the
container afterwards, even on failure. Each test creates and drops its own uniquely named
database. Do not point `SOLPRINT_TEST_DATABASE_URL` at the development database.

## Backend coverage (Phase 00)

* `solprint-core`: amount parsing/arithmetic/overflow/rounding, identifier validation,
  timestamps, reason codes, freshness, envelopes.
* `solprint-config`: defaults, validation of every key, live-trading rejection, https
  enforcement, loopback enforcement, and a synthetic-secret leak test over `Debug`, errors
  and the safe summary.
* `solprint-api`: liveness, readiness 200/503, status, version, 404/405/413 envelopes,
  request-id handling, security headers, secret leak, graceful shutdown.
* `solprint-storage`: migrations, idempotent re-run, instance registration and
  persistence, health probe (database tests).
* Contract export: `cargo test -p solprint-api` regenerates
  `frontend/src/types/generated`; CI fails on drift.

## Frontend coverage

* Unit: number/time formatting (u64 exactness, rounding, signs), connection-state
  derivation (connecting/connected/degraded/offline/stale), API client failure
  classification (unreachable, proxy 502, structured error, contract mismatch, malformed
  JSON, timeout), generated-contract barrel completeness, fixture isolation, shell
  rendering (PAPER, MODE UNVERIFIED, navigation, skip link, single h1, not-found).
* UI (Playwright, mocked API): healthy / offline / database-down / loading states,
  keyboard navigation and visible focus, axe WCAG 2.2 AA on every route and the offline
  state, no horizontal overflow and mode + connection always visible at every standard
  viewport.

## Visual regression

* Viewports: 1920×1080, 1440×900, 1280×800, 1024×768, 390×844. Pages: Overview, System,
  Design system (full page), plus offline and database-down states at 1440 and 390.
* The API is mocked and the clock frozen at `2026-11-26T12:00:00Z`, so captures are
  deterministic. They run only inside `mcr.microsoft.com/playwright:v1.65.1-noble`
  (locally via `scripts/playwright.ps1`, and in CI) so fonts render identically.
* Baselines: `frontend/tests/visual/__screenshots__/`. **Baselines are candidates until
  the owner approves them.** To update after an intentional change:

  ```powershell
  ./scripts/playwright.ps1 -Project visual -UpdateSnapshots
  ```

  Review every changed PNG in the diff before committing.
