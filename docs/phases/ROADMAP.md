# Roadmap

Phases 00 through 14 are the established roadmap. Only the scopes marked **known** were
stated in the project brief; the others are placeholders whose names and scope the
**owner must confirm** before the phase starts. CORTEX formulas and trading thresholds are
deliberately not specified here.

| Phase | Scope | Status |
|---|---|---|
| 00 | **Foundation** — workspace, config and secrets, PostgreSQL/SQLx, health/status API, observability, Docker, TERMINAL shell and design system, tests, CI | implemented on `feature/00-foundation`; awaiting owner review |
| 01 | **GATEWAY / providers** — production provider integrations (Helius, Jupiter, DEX Screener, Solana RPC), caching, rate limits, API budgets *(known)* | not started |
| 02 | to be confirmed by the owner | not started |
| 03–09 | backend module phases (DATA ENGINE, RADAR, FILTER, CORTEX, GUARD, REPLAY, SANDBOX — order and grouping to be confirmed by the owner); each must expose UI-facing state deliberately (see [ROADMAP_UI_INTEGRATION.md](../design/ROADMAP_UI_INTEGRATION.md)) | not started |
| 10 | **TERMINAL** — workspace shell and all core views *(known)* | not started (Phase 00 shell exists) |
| 11 | **SNIPER** — authorised execution and reconciliation, built on existing safety primitives *(known)* | not started |
| 12 | **Hardening** *(known)* | not started |
| 13 | **Hardening / security** *(known)* | not started |
| 14 | **Release** — final audit and full baseline coverage *(known)* | not started |

## Rules carried across phases

* A phase starts only after the previous phase's acceptance criteria pass and the owner
  approves.
* Live trading stays rejected by configuration until a phase explicitly approved for
  execution changes that, with GUARD in place.
* Each phase records scope, implementation, test results, limitations and acceptance in
  `docs/phases/PHASE_NN.md`.
