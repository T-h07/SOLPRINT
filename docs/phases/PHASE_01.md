# Phase 01 — GATEWAY

Branch: `feature/01-gateway` (based on `develop` at `f2e0c3e`, Phase 00 accepted).
Status: **implemented, awaiting owner review.** Not merged; no release or tag.

## Scope

In scope: GATEWAY, the single, typed, asynchronous, cost-aware and observable read-only
access layer to Helius (HTTP RPC and standard WebSocket), Jupiter (Price V3, Tokens V2,
quote-only Swap V2 `/order`), DEX Screener and the Solana public RPC as a fallback; budget,
rate limits, scheduler, cache, single-flight, circuit breakers, health, metrics,
persistence; monitoring API and Gateway workspace; diagnostics; local verification
replacing hosted CI.

Out of scope (not implemented): ingestion, transaction decoding, wallet discovery or
ranking, token security classification, CORTEX, signals, GUARD sizing, portfolio,
backtesting, paper or real execution, wallets, keys, signing, broadcasting, ML, runtime
LLMs, paid API features, the full TERMINAL dashboard, hosted CI/CD.

## What was built

| Area | Implementation |
|---|---|
| Crate | `solprint-gateway`: engine, scheduler, GCRA rate limiter, budget, moka cache, single-flight, breaker, health, metrics, transport, JSON-RPC, Helius RPC, Jupiter, DEX Screener, stream manager, snapshots, store trait, typed traits ([GATEWAY.md](../architecture/GATEWAY.md)) |
| Persistence | migration `20261126000002_gateway.sql`: billing cycles, per-minute usage and metric buckets, stream checkpoints; `GatewayStore` implemented for `Database` |
| API | `/api/v1/gateway/{status,providers,usage,metrics,budgets,streams,cache}`; `/system/status` providers now carry measured health (`UNKNOWN` until traffic) |
| Configuration | 29 new keys with validated ranges (and a new default fallback URL) ([CONFIGURATION.md](../development/CONFIGURATION.md)); defaults below documented provider limits; streams, discovery and getBlock off |
| Frontend | `/gateway` workspace (budget, gateway state, providers, operations, usage, streams, cache), all data states; `DefinitionList` `wrap` option; `ModeIndicator` loading state given `role="status"` (axe fix) |
| Tools | `scripts/gateway-diagnostics.ps1` (offline by default, `-Live` opt-in); `scripts/verify.ps1` (full local gate on the exact commit); smoke test extended for GATEWAY |
| Policy | `.github/workflows/ci.yml` inspected and removed; docs and CLAUDE.md now require local verification |
| Docs | GATEWAY, API_BUDGET_POLICY, GATEWAY_CAPACITY_MODEL, STREAM_RECOVERY, PROVIDER_CAPABILITIES (new); DATA_CONVENTIONS, ARCHITECTURE, MODULE_BOUNDARIES, SECURITY_MODEL, CONFIGURATION, TESTING, TROUBLESHOOTING, UI_DESIGN_SYSTEM, ROADMAP, README, CLAUDE.md (updated) |

## Verification

Results are recorded in the [Verification log](#verification-log) at the end of this
document, with the exact commit they apply to.

## Limitations and open items

* **No live provider verification.** Every provider integration is verified against
  loopback mocks built from the documented contracts. The bounded live check
  (`gateway-diagnostics.ps1 -Live`) needs the owner's explicit opt-in and has not been
  run (P01-T54).
* **Visual baselines.** The Gateway pages have no approved baselines yet, and the
  Overview/System baselines change because provider health now reads "Not measured".
  Candidate baselines must be generated with
  `./scripts/playwright.ps1 -Project visual -UpdateSnapshots`, reviewed and approved by
  the owner. Until then the visual check fails, and so does the full gate.
* **cargo-deny** is not installed on the development machine, so `verify.ps1` reports
  the Rust advisory check as BLOCKED. Install with `cargo install cargo-deny --locked`.
* Recovery covers `logs` subscriptions only; other subscription kinds report
  `CONTINUITY_UNKNOWN` after a reconnect ([STREAM_RECOVERY.md](../architecture/STREAM_RECOVERY.md)).
* Local credit accounting is an estimate of Helius's own metering; the provider's
  dashboard is authoritative.
* Capacity figures are arithmetic from limits; overhead benchmarks are from one machine
  ([GATEWAY_CAPACITY_MODEL.md](../architecture/GATEWAY_CAPACITY_MODEL.md)).

## Owner actions

1. **Rotate the Helius and Jupiter credentials.** They were exposed in an earlier
   conversation and rotation has not been independently confirmed. Update `.env`
   yourself; never paste keys into a conversation.
2. Review the Gateway workspace and approve or reject the candidate visual baselines
   (P01-T70).
3. Optionally authorise the bounded live check: `./scripts/gateway-diagnostics.ps1 -Live`.
4. Install `cargo-deny` so the advisory check can run.
5. If the remote repository still has a required "CI" status check or branch protection
   referring to the removed workflow, remove it in the repository settings (not changed
   by this phase).

## Acceptance criteria

Statuses: PASS / FAIL / BLOCKED / NOT RUN / PENDING OWNER REVIEW. Test names refer to
`backend/crates/solprint-gateway/tests/*` and `src/**` unit tests unless stated.
"Mock provider" means a loopback server answering in the provider's documented format;
no criterion here claims verification against a live provider (see P01-T54).

### Architecture and integration

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P01-T01 | Correct feature branch is active | PASS | `git branch --show-current` → `feature/01-gateway` |
| P01-T02 | Existing Phase 00 functionality remains operational | PASS | Phase 00 config, API, storage, UI and e2e tests pass in `verify.ps1` on `72537c7`; Phase 00 visual baselines differ only by the intended provider-health text (see T78) |
| P01-T03 | Official provider capabilities are documented | PASS | [PROVIDER_CAPABILITIES.md](../architecture/PROVIDER_CAPABILITIES.md), sources dated 2026-11-26 |
| P01-T04 | GATEWAY is the centralized provider communication layer | PASS | only `solprint-gateway` depends on `reqwest`/`tokio-tungstenite`; destination allow-list (`transport::destination_policy`) |
| P01-T05 | Provider DTOs are isolated from domain models | PASS | provider JSON decoded in GATEWAY modules into `models.rs`; consumers use `traits.rs` |
| P01-T06 | Helius standard RPC operations work | PASS | mock provider: `rpc_success_is_metered_cached_and_carries_provenance`, `account_data_is_decoded_with_integer_lamports`, `null_account_means_not_found`, RPC error/id tests; live not run (T54) |
| P01-T07 | Historical RPC pagination is bounded and correct | PASS | `signature_pagination_is_bounded` |
| P01-T08 | Versioned transaction responses are supported | PASS | `versioned_and_legacy_transactions_are_supported` (`maxSupportedTransactionVersion: 0`) |
| P01-T09 | Jupiter Price V3 integration works | PASS | mock provider: `jupiter_prices_keep_exact_decimals`, `prices_keep_exact_digits_and_skip_unpriced` |
| P01-T10 | Jupiter token metadata integration works | PASS | mock provider: `tokens_are_bounded_and_validated` |
| P01-T11 | Jupiter quote-only integration works | PASS | mock provider: `jupiter_quote_is_quote_only` (no taker, `x-api-key`), `jupiter_response_with_a_transaction_is_refused`, `jupiter_no_route_is_typed` |
| P01-T12 | DEX Screener token and pair integrations work | PASS | mock provider: `dex_pairs_by_tokens_parse_and_cache`, `pairs_are_normalized_exactly_and_bounded`, `envelopes_with_null_pairs_are_empty` |
| P01-T13 | DEX Screener batching is correct | PASS | `dex_token_batches_never_exceed_thirty_addresses` |
| P01-T14 | Compatible public RPC fallback works | PASS | mock provider: `rate_limited_primary_falls_back_to_public_rpc_when_allowed`, `fallback_is_not_used_without_permission` |
| P01-T15 | Normalized responses preserve provenance and freshness | PASS | provenance/freshness asserted in mock tests; `freshness_moves_from_fresh_to_stale_to_expired` |

### WebSocket and recovery

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P01-T16 | Standard WebSocket connection works | PASS | mock provider: `slot_subscription_delivers_events_and_meters_bytes` |
| P01-T17 | Subscription acknowledgement is handled | PASS | same; `failed_subscription_degrades_the_connection` |
| P01-T18 | Subscription registration and removal work | PASS | `subscription_limits_and_validation`, `signature_subscription_ends_after_its_notification` |
| P01-T19 | Heartbeats and dead connection detection work | PASS | `silent_connection_is_detected_by_heartbeat_and_replaced` |
| P01-T20 | Reconnection uses bounded backoff | PASS | backoff 1 s → 60 s with jitter (`stream/manager.rs`); reconnect tests |
| P01-T21 | Subscriptions restore after reconnect | PASS | `reconnect_resubscribes_and_reports_unknown_continuity` |
| P01-T22 | Connection generations prevent identifier confusion | PASS | same test: new server id maps to the same subscription, generation 2 |
| P01-T23 | Duplicate observations are handled | PASS | `logs_gap_is_recovered_oldest_first_with_dedup_and_checkpoints` |
| P01-T24 | Bounded recovery works where supported | PASS | same; bounded to 3 × 100 signatures, 20 s |
| P01-T25 | Unresolved stream gaps are explicitly reported | PASS | `incomplete_recovery_reports_a_gap` |
| P01-T26 | Commitment metadata is preserved | PASS | events carry the subscription commitment; `logs_subscription_mentions_exactly_one_address_with_explicit_commitment` |
| P01-T27 | Stream backpressure is handled | PASS | `a_slow_consumer_never_blocks_the_connection` |
| P01-T28 | Stream tasks shut down cleanly | PASS | `shutdown_stops_the_manager` |

### API efficiency

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P01-T29 | Centralized provider rate limiting works | PASS | `per_second_rate_allows_burst_then_spaces_requests`, `per_minute_even_never_exceeds_the_window` |
| P01-T30 | Shared rate-limit groups are enforced | PASS | `jupiter_endpoints_share_one_rate_group` |
| P01-T31 | Request deadlines include queue and retry time | PASS | `deadlines_bound_slow_providers`, `refuses_when_the_wait_passes_the_deadline_without_consuming`, `deadline_and_abandoned_waiters_never_leak_slots` |
| P01-T32 | Scheduler queues are bounded | PASS | `full_queues_are_refused_immediately`, `global_limit_spans_providers` |
| P01-T33 | Priority scheduling operates correctly | PASS | `bounds_concurrency_and_serves_by_priority` |
| P01-T34 | Method-specific credit accounting works | PASS | `every_rpc_operation_has_a_cost_and_nothing_else_does`, `credits_charged` asserted per call |
| P01-T35 | Streaming consumption is measured | PASS | `stream_bytes_are_metered_with_a_running_ceiling`, byte metering in the slot stream test |
| P01-T36 | Protected credit reserves are enforced | PASS | `admission_sheds_low_priorities_first` |
| P01-T37 | Concurrent requests cannot exceed local budget admission | PASS | atomic reservation (`budget::try_reserve`); `budget_sheds_low_priorities_first_and_persists_across_restarts` |
| P01-T38 | Usage accounting survives restart | PASS | `credit_ledger_survives_restarts` (PostgreSQL) and the memory-store restart test |
| P01-T39 | Budget exhaustion stops applicable traffic | PASS | `budget_sheds_low_priorities_first_and_persists_across_restarts`, `thresholds_follow_the_documented_table` |
| P01-T40 | Adaptive budget conservation works | PASS | same; `unloaded_ledger_admits_only_high_priorities` |
| P01-T41 | Cache TTL and eviction operate correctly | PASS | `freshness_moves_from_fresh_to_stale_to_expired`, `stores_typed_and_negative_entries`, `zero_capacity_disables_the_cache` |
| P01-T42 | Stale data is never represented as fresh | PASS | provenance `STALE_CACHE` only under `AllowStaleOnError`; cached `observed_at` preserved |
| P01-T43 | Equivalent concurrent requests are coalesced | PASS | `concurrent_identical_requests_share_one_call`, `concurrent_identical_reads_are_coalesced` |
| P01-T44 | Provider-specific batch limits are enforced | PASS | `multiple_accounts_are_chunked_at_the_documented_maximum`, `dex_token_batches_never_exceed_thirty_addresses`; Jupiter price ids chunked at 50 per request (`PRICE_IDS_MAX`) |

### Resilience and observability

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P01-T45 | Retry policies respect deadlines and budgets | PASS | `transient_errors_are_retried_for_normal_priorities`, `deadlines_bound_slow_providers`; each retry is budget-admitted |
| P01-T46 | Provider cooldown instructions are honored | PASS | `jupiter_429_is_rate_limited_with_retry_after`, `upstream_block_is_clamped_and_respected` |
| P01-T47 | Circuit breakers operate correctly | PASS | `opens_after_threshold_probes_and_backs_off`, `open_interval_is_capped`, `circuit_opens_after_consecutive_failures` |
| P01-T48 | Permanent authentication failures do not retry indefinitely | PASS | `retry_classification_follows_provider_docs` (401/403 never retried), `client_errors_and_cancelled_probes_do_not_trip` |
| P01-T49 | Provider health accurately represents degradation | PASS | `health_is_unknown_until_traffic_and_tracks_outcomes`, `budget_exhaustion_and_rate_limits_are_visible` |
| P01-T50 | Request and latency metrics are accurate | PASS | `percentiles_report_bucket_upper_bounds`, `drain_and_restore_round_trip` |
| P01-T51 | Metric cardinality and storage growth are bounded | PASS | closed `operations::ALL` label set; retention pruning (`usage_and_metric_buckets_accumulate_and_prune`, PostgreSQL) |
| P01-T52 | Historical telemetry persists | PASS | `usage_and_metrics_are_flushed_to_the_store`, PostgreSQL tests in `gateway_db.rs` |
| P01-T53 | Monitoring API does not trigger external provider traffic | PASS | `snapshots_and_stored_reads_never_contact_providers`; `gateway_endpoints_report_local_state_only` (API); smoke check "monitoring reads create no provider requests" |
| P01-T54 | Real-provider smoke tests are reported honestly | NOT RUN | needs the owner's explicit opt-in (`gateway-diagnostics.ps1 -Live`); offline diagnostics ran and contacted nothing |
| P01-T55 | Capacity model and benchmark methodology are documented | PASS | [GATEWAY_CAPACITY_MODEL.md](../architecture/GATEWAY_CAPACITY_MODEL.md); `cargo bench -p solprint-gateway` |

### UI and visual quality

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P01-T56 | Gateway monitoring workspace is operational | PASS | Playwright "gateway workspace" tests (active, idle, pressure, database-down, offline) |
| P01-T57 | Provider health data comes from actual backend state | PASS | UI reads `/api/v1/gateway/*` only; idle scenario shows "Not measured" |
| P01-T58 | API usage and budget data are accurately represented | PASS | budget meter `aria-valuenow` equals API value; thresholds in text; exact integers |
| P01-T59 | Estimated usage is distinguished from verified usage | PASS | Budget pane footnote ("Estimated … Helius's dashboard is authoritative for billing"), projection labelled "linear"; Playwright assertion |
| P01-T60 | WebSocket health and recovery state are visible | PASS | Streams pane (state, generation, reconnects, gaps, recovered, duplicates, per-subscription continuity); pressure test |
| P01-T61 | Cache efficiency metrics are visible | PASS | Response cache pane (hits, misses, hit rate, stale) |
| P01-T62 | Charts use real telemetry or isolated labeled fixtures | PASS | the only graphic (budget meter) is drawn from API values; fixtures exist only in tests |
| P01-T63 | Missing and stale data states render correctly | PASS | loading, empty, error with retry, stale, partial per pane; Playwright offline/database-down/loading tests |
| P01-T64 | Existing all-black visual identity is preserved | PASS | shared tokens and components only; screenshots inspected at 390, 1440, 1920 |
| P01-T65 | No generic AI-dashboard aesthetic is introduced | PASS | screenshot review: dense tables, tabular numbers, text + glyph status, no cards, gradients or glow |
| P01-T66 | Required responsive viewports pass inspection | PASS | Playwright overflow tests on `/gateway` at all five viewports; inspection at 390/1440/1920 fixed truncation and clipping |
| P01-T67 | Keyboard and accessibility checks pass | PASS | axe WCAG 2.2 AA on `/gateway` in healthy, idle, pressure, offline, loading; keyboard tests |
| P01-T68 | UI updates do not cause significant layout shifts | PASS | measured CLS (Playwright, PerformanceObserver): polling updates 0.0000 at 1440 and 390; initial load 0.053 (1440) and 0.0035 (390) after sizing the budget skeleton |
| P01-T69 | Screenshots are captured and inspected | PASS | review captures inspected; findings fixed (wrapping unit, clipped column, truncated values, layout shift) |
| P01-T70 | Owner approves the visual implementation | PENDING OWNER REVIEW | — |

### Security, delivery and scope

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P01-T71 | Existing .env is preserved and untracked | PASS | `.env` never read for values, never modified; `git ls-files .env` empty; verify checkout refuses a tracked `.env` |
| P01-T72 | Credentials do not leak into logs or responses | PASS | `Secret<T>`; `request_debug_never_shows_url_or_headers`, `sanitizer_drops_urls_keys_and_control_characters`; API no-secret test covers gateway endpoints; container log check |
| P01-T73 | Provider endpoints and operations are restricted | PASS | `destination_policy`, prohibited RPC methods, blocked execution paths and hosts, `get_block_is_disabled_by_default_and_sends_nothing`, `dex_discovery_is_off_by_default` |
| P01-T74 | Live trading remains impossible | PASS | configuration rejects live; `execution_capability: NONE`; no signing code (`deny.toml` bans) |
| P01-T75 | No paid features are activated | PASS | no overage/top-up configuration exists; paid endpoints not implemented; defaults below free limits |
| P01-T76 | Application operates safely without provider credentials | PASS | `missing_helius_key_disables_helius`; API tests and the verification stack run with no keys |
| P01-T77 | Backend and frontend automated tests pass | PASS | on `72537c7`: Rust 203 (incl. PostgreSQL), Vitest 34, Playwright ui 42, e2e 5 — 0 failures |
| P01-T78 | Complete local verification passes on the exact candidate revision | FAIL | `verify.ps1` on `72537c7`: 16 PASS, visual FAIL (21 baselines missing or changed, awaiting owner approval), cargo deny BLOCKED (not installed). See log |
| P01-T79 | Documentation is complete and accurate | PASS | documents listed above, written against the implemented code |
| P01-T80 | No future-phase trading functionality is implemented | PASS | no signals, sizing, positions, execution or wallet code; scope review |

## Verification log

### 2026-11-26 — `verify.ps1` (full mode) on `72537c73536bbb0f47b318a2bd7f584a9039d0f9`

Toolchain: rustc 1.99.1, Node v24.19.0, Docker 29.9.3; Windows 11. Duration 826 s.
Exit code 1. The run removed its worktree, Compose project, volumes and images.

| Check | Result | Tests |
|---|---|---|
| Isolated checkout | PASS | |
| Secret scan: candidate revision | PASS | |
| Secret scan: staged changes and full history | PASS | |
| Dockerfile lint (hadolint) | PASS | |
| Locked frontend install (npm ci) | PASS | |
| Quality gate (check.ps1) | PASS | Rust 194, Vitest 34 |
| Production bundle excludes fixtures and showcase | PASS | |
| PostgreSQL integration tests (test.ps1) | PASS | Rust 203 |
| Rust advisories and licences (cargo deny) | BLOCKED | cargo-deny not installed |
| npm advisories (high and critical) | PASS | |
| Playwright ui | PASS | 42 |
| Playwright visual | FAIL | 7 passed, 21 failed |
| Synthetic .env and isolated Compose config | PASS | |
| Clean image builds and stack startup | PASS | |
| Container hardening | PASS | |
| Smoke test | PASS | |
| End-to-end (real stack) | PASS | 5 |
| Container logs contain no configured secret | PASS | |

Visual failures, all expected from intended changes: 9 Gateway screenshots have no
baseline yet (1920, 1440, 1280, 1024, 390; idle and pressure at 1440 and 390), and 12
Overview/System screenshots differ by 1–2 % of pixels because provider health now reads
"Not measured" and GATEWAY is listed as read-only. They need owner review and
`./scripts/playwright.ps1 -Project visual -UpdateSnapshots` as candidate baselines.

An earlier run on `f80cccf` failed the quality gate (Compose validation needed a `.env`
in the clean checkout) and the smoke test (gateway responses read with a wrong
envelope); both were fixed in `72537c7`. This document was updated after the run;
the change is documentation only.

