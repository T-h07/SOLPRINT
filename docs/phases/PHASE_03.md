# Phase 03 — RADAR (autonomous wallet discovery)

Branch: `feature/03-wallet-discovery` (based on `develop` at `d2e27c8`, Phase 02 merged).
Status: **implemented, awaiting owner review.** Not merged; no release or tag.

## Scope

In scope: RADAR, a read-only, evidence-backed wallet discovery and candidate registry.
It covers:

- autonomous market discovery from public listings through GATEWAY;
- an explainable market universe with classification, screening and bucketed selection;
- bounded pool sampling through the DATA ENGINE;
- wallet extraction from finalized, verified, signer-attributed swaps only;
- a registry with a lifecycle, provenance and transition history;
- staged, bounded history investigation (A, B and C) through DATA ENGINE backfills, with
  explicit coverage;
- preliminary screening (activity windows, FIFO matching with exact arithmetic);
- a documented investigation priority, with qualification kept separate from it;
- a budget-aware scheduler and monitoring rotation;
- coverage-aware dormancy, archival and reactivation;
- a transactional outbox with RADAR events, and a typed query interface for later
  phases;
- a read-only API, the `solprint radar` CLI, `scripts/radar-diagnostics.ps1`, the
  `/radar` workspace, tests, benchmarks and documentation.

Out of scope (not implemented):

- wallet intelligence, profitability, skill or copy-trading judgements (Phase 04 and
  later);
- token security analysis (FILTER);
- CORTEX, signals, strategies, GUARD, backtesting, paper or real execution;
- wallets, keys, signing, broadcasting;
- historical USD pricing;
- live GATEWAY stream subscriptions for monitoring;
- clustering or identification of real-world individuals;
- runtime LLM or ML;
- paid provider features and hosted CI/CD.

## What was built

| Area | Implementation |
|---|---|
| Crate | `solprint-radar`: market classification (`radar-class-1`), market screening and bucketed selection, wallet eligibility, FIFO screening, priority (`radar-priority-1`), qualification, lifecycle (`radar-lifecycle-1`), monitoring rotation, budget admission, the engine (discovery cycle, job dispatcher, DATA ENGINE event consumer, evaluation and maintenance loops) and `query::RadarQuery` for later phases ([RADAR.md](../architecture/RADAR.md)) |
| GATEWAY | typed market discovery listings: Jupiter Tokens V2 categories and `/recent`; DEX Screener pairs, plus boosts only if enabled; `getMultipleAccounts` pool validation. Costs and sources are in [PROVIDER_CAPABILITIES.md](../architecture/PROVIDER_CAPABILITIES.md) |
| DATA ENGINE | outbox consumer registration (`radar`), dynamic backfill targets, fixture timing hooks; fix for finality on unchanged finalized refetch (see below) |
| Persistence | migration `20261127000004_radar.sql`: markets, pools, observations, cycles, source runs, wallets, wallet sources, evidence, transitions, snapshots, jobs, control, operator actions, outbox and deliveries |
| API | `GET /api/v1/radar/{status,wallets,wallets/{address},markets,cycles,sources,jobs,events}`, read from PostgreSQL only |
| Configuration | 46 `RADAR_*` keys with validated ranges and cross-checks against DATA ENGINE limits ([CONFIGURATION.md](../development/CONFIGURATION.md)). Autonomous discovery is **off by default** |
| CLI and tools | `solprint radar` (status, pause/resume, cycle, investigate, reevaluate, exclude/include, reactivate, cancel, wallets, wallet, markets, jobs); `scripts/radar-diagnostics.ps1` (offline by default, `-EndToEnd`, `-Live -RequestCycle` opt-in) |
| Frontend | `/radar` workspace: registry with lifecycle filter, sort and pagination; wallet inspector (evidence, priority breakdown, coverage, screening, transitions); discovery cycles and sources; market universe; budget, monitoring and jobs. Every pane handles all data states ([UI_DESIGN_SYSTEM.md](../design/UI_DESIGN_SYSTEM.md)) |
| Tests | 42 unit tests; 9 PostgreSQL end-to-end tests through the real DATA ENGINE with an in-memory chain and fixture feed; API tests; UI and visual tests; an opt-in registry scale benchmark and CPU benchmarks |
| Docs | RADAR and RADAR_CAPACITY_MODEL (new); PROVIDER_CAPABILITIES, DATA_ENGINE, MODULE_BOUNDARIES, CONFIGURATION, TESTING, TROUBLESHOOTING, UI_DESIGN_SYSTEM, ROADMAP, README and CLAUDE.md (updated) |

**Documentation consolidation.** The prompt suggested separate documents for discovery
sources, market universe, wallet registry, historical investigation, preliminary
screening and scheduler. They are consolidated as sections of
[RADAR.md](../architecture/RADAR.md), with the capacity model kept separate.

## Defects found and fixed during verification

- **DATA ENGINE: unchanged finalized refetch did not finalize** (`1e3b0d5`). A refetch at
  finalized commitment that returned the stored evidence unchanged promoted only the
  transaction row. Swaps stayed `PROVISIONAL`, `finalized_at` was not set and no
  `TransactionFinalized` event was emitted, so RADAR never saw finalization. It surfaced
  as an intermittent failure of `radar_db::autonomous_discovery_from_an_empty_registry`
  (4 of 8 runs). The unchanged path now applies finality through the same transition as
  a status check. Regression test:
  `engine_db::unchanged_finalized_refetch_finalizes_transaction_and_swaps`, which fails
  without the fix. The autonomous test then passed 10 of 10 runs.
- **Reduced activity was configured but unused** (`4657789`).
  `RADAR_REDUCE_MONITORING_AFTER_DAYS` is now applied: quiet incumbents leave monitoring
  and quiet challengers are not started.

## Limitations and open items

- **No live provider run.** All discovery tests use fixture listings and an in-memory
  chain. A bounded live cycle (`radar-diagnostics.ps1 -Live -RequestCycle`) needs the
  owner to enable `RADAR_AUTO_DISCOVERY_ENABLED` and has not been run (P03-T98).
- **Monitoring is periodic.** Without GATEWAY stream subscriptions, monitored wallets are
  refreshed with one bounded history page per reevaluation interval
  (`PERIODIC_HISTORY_REFRESH`).
- **No historical USD.** Outcomes are per quote denomination (SOL, USDC, USDT) and never
  summed across denominations. USDC and USDT cycles flag incomplete fees.
- **Preliminary figures.** Screening covers at most the newest 5,000 verified swaps
  inside the investigated window. It is never wallet profitability.
- **Capacity.** The default upper bounds of sampling and monitoring together exceed the
  Helius Free daily normal allowance. Budget states bound cumulative spend, not the 40 %
  share ([RADAR_CAPACITY_MODEL.md](../architecture/RADAR_CAPACITY_MODEL.md)).
- **The concurrency cap is per dispatcher.** Claims are safe: `FOR UPDATE SKIP LOCKED`
  never leases a job twice (tested). The `max_running` cap is checked before the claim in
  the same transaction, so two dispatchers claiming at the same moment could briefly
  exceed it. One process runs one dispatcher.
- Multiple pools per token are handled by choosing the best supported pool, the one with
  the highest reported liquidity (`engine.rs`, `market.rs`). No dedicated multi-pool test
  exists.
- The desktop visual baselines show only the first viewport (inner scroll container),
  as in earlier phases.

## Owner actions

1. Review the visual candidates (P03-T96). New, in
   `frontend/tests/visual/__screenshots__/pages.spec.ts/`:
   - `radar-{1920x1080,1440x900,1280x800,1024x768,390x844}`;
   - `radar-empty-{1440x900,390x844}`;
   - `radar-paused-{1440x900,390x844}`;
   - `radar-database-down-{1440x900,390x844}`.
2. Optionally enable autonomous discovery and authorise one bounded live cycle:
   `RADAR_AUTO_DISCOVERY_ENABLED=true`, then `./scripts/radar-diagnostics.ps1 -Live
   -RequestCycle`. At most `RADAR_MAX_MARKET_SAMPLES_PER_CYCLE` (5) samples, about 130
   Helius credits; the history that follows is admitted under RADAR's budget share.
3. Credential rotation remains the owner's responsibility.

## Acceptance criteria

Statuses: PASS / FAIL / BLOCKED / NOT RUN / PENDING OWNER REVIEW.

Test references:

- `radar_db::<name>` is `backend/crates/solprint-radar/tests/radar_db.rs` (PostgreSQL,
  disposable database, real DATA ENGINE, in-memory chain and fixture feed);
- `<module>::<name>` refers to unit tests in `backend/crates/solprint-radar/src`;
- `http::<name>` is `backend/crates/solprint-api/tests/http.rs`;
- `engine_db::<name>` is the DATA ENGINE suite;
- "UI `<title>`" is `frontend/tests/ui/shell.spec.ts` (`radar workspace`).

No criterion claims verification against a live provider.

### Architecture and discovery

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T01 | Correct implementation branch is active | PASS | Commits are pushed to `feature/03-wallet-discovery`, based on `develop` at `d2e27c8` |
| P03-T02 | Previous-phase regression checks pass | NOT RUN | Filled from the verification run: all GATEWAY and DATA ENGINE tests run in `check.ps1` and `test.ps1` ([Verification log](#verification-log)) |
| P03-T03 | RADAR architecture and dependency boundaries are documented | PASS | [RADAR.md](../architecture/RADAR.md) §Placement and boundaries; [MODULE_BOUNDARIES.md](../architecture/MODULE_BOUNDARIES.md) rule 8 and the dependency graph |
| P03-T04 | All external provider operations pass through GATEWAY | PASS | `solprint-radar` has no HTTP client dependency (`Cargo.toml`). Listings use GATEWAY's typed discovery traits, and chain data comes through the DATA ENGINE's `ChainSource`. The GATEWAY methods are registered with cost and source in PROVIDER_CAPABILITIES |
| P03-T05 | Transaction interpretation uses the existing Data Engine | PASS | RADAR reads DATA ENGINE swaps and outbox events (`SwapRecorded`, `SwapRetracted`, `TransactionFinalized`) and never decodes transactions. Samples and history are DATA ENGINE backfills (`radar_db::autonomous_discovery_from_an_empty_registry`) |
| P03-T06 | Verified discovery-source interfaces are implemented | PASS | `feed::Listing` over GATEWAY: Jupiter Tokens V2 `toptraded`, `toporganicscore` and `/recent`; DEX Screener pairs and boosts. The endpoints are documented in PROVIDER_CAPABILITIES. There is no undocumented `TOP_TRENDING` source |
| P03-T07 | Market discovery starts without manually supplied wallets | PASS | `radar_db::autonomous_discovery_from_an_empty_registry` starts with 0 wallets and 0 DATA ENGINE targets, and ends with a monitored wallet |
| P03-T08 | Relevant token markets are discovered automatically | PASS | The same test: the listed mint becomes a `MEME_CANDIDATE` market with a validated `SUPPORTED` Raydium pool (`markets_new = 1`, `samples_scheduled = 1`) |
| P03-T09 | Multiple pools for the same token are handled correctly | PASS | Pools are stored per pool address under one mint. Sampling uses the best supported pool, the one with the highest liquidity (`engine.rs` pool view, `market.rs`). Unsupported pools are never sampled. Implementation evidence only; see Limitations |
| P03-T10 | Token identity uses mint addresses | PASS | `radar_markets` is keyed by (network, mint); symbols are display hints. `classify::known_mints_are_excluded_from_sampling` matches by mint |
| P03-T11 | Memecoin relevance classification is explainable | PASS | An ordered ruleset (`radar-class-1`) stores a reason per market. `classify::launchpad_and_bonding_curve_pools_mark_meme_candidates`, `classify::tags_win_over_launchpad_and_age_needs_evidence` |
| P03-T12 | Promotional activity is not interpreted as organic demand | PASS | DEX Screener boosts are recorded `promotional_only` and are off by default. `market::promotional_only_markets_are_exploration_only` |
| P03-T13 | Market selection supports multiple discovery categories | PASS | Five buckets with shares and roll-over. `market::allocation_follows_shares_with_largest_remainder`, `market::unused_capacity_flows_to_other_buckets_and_exploration_keeps_a_floor`, `market::selection_spreads_across_buckets_and_never_picks_twice` |
| P03-T14 | Market observations retain source and freshness metadata | PASS | `radar_market_observations` stores source, rank, promotional flag and observed time. `market::stale_observations_and_recent_samples_defer` |
| P03-T15 | Transaction sampling is bounded | PASS | 25 or 100 signatures per sample, 5 samples per cycle, 2 concurrent sample jobs (configuration validated). An estimate of 1 + N credits under budget admission |
| P03-T16 | Duplicate transaction sampling is avoided | PASS | A unique partial index on the open job (class, subject), the six-interval resample guard, and DATA ENGINE reuse of stored transactions. `radar_db::restart_resumes_from_durable_state_without_duplicate_jobs` (a second cycle schedules 0 samples) |

### Wallet identification and registry

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T17 | Candidates are extracted from evidenced economic activity | PASS | Only verified, canonical swaps with `VERIFIED_OWNER` or `VERIFIED_AUTHORITY` attribution create evidence. `scoring::eligibility_rules`; `radar_db::autonomous_discovery_from_an_empty_registry` (12 evidence rows for 6 round trips) |
| P03-T18 | Fee payer is not automatically treated as trader | PASS | Extraction uses the DATA ENGINE's evidence-based attribution, where the fee payer is never a substitute (WALLET_ATTRIBUTION.md). An unattributed swap yields no wallet |
| P03-T19 | Known infrastructure accounts are excluded appropriately | PASS | Programs and known pool accounts are `KNOWN_INFRASTRUCTURE` (`programs::is_infrastructure`, `scoring::eligibility_rules`) and are not registered as new wallets. Known mints are excluded from sampling (`classify::known_mints_are_excluded_from_sampling`) |
| P03-T20 | Pool vaults are not misclassified as trader wallets | PASS | Token accounts, including pool vaults, are `UNSUPPORTED_ACCOUNT_ROLE`, and known pools are `KNOWN_INFRASTRUCTURE` (`scoring::eligibility_rules`) |
| P03-T21 | Ambiguous attribution is explicitly represented | PASS | `AMBIGUOUS_ATTRIBUTION` eligibility when every piece of evidence was de-attributed; screening counts `ambiguous_tokens` (`scoring::eligibility_rules`, `screening::unknown_pre_balance_marks_inventory_ambiguous`) |
| P03-T22 | Candidate registry identity is unique | PASS | Primary key (network, address). `radar_scale` asserts that duplicate registration writes no row at 100 to 10,000 wallets |
| P03-T23 | Multiple discovery sources preserve provenance | PASS | `radar_wallet_sources` keeps every source. `radar_db::autonomous_discovery_from_an_empty_registry` asserts the sampling source |
| P03-T24 | Candidate upserts are atomic and idempotent | PASS | Upsert, evidence and transition are written in one transaction, and evaluations are revision-guarded. `radar_db::event_redelivery_is_idempotent_and_retraction_invalidates_evidence` (redelivery leaves 2 evidence rows and 1 wallet) |
| P03-T25 | Candidate lifecycle states are implemented | PASS | `lifecycle::progressive_stages`, `lifecycle::dormancy_requires_established_inactivity`, `lifecycle::archival_and_reactivation`, `lifecycle::ineligible_wallets_are_rejected_once` |
| P03-T26 | Lifecycle transition history is preserved | PASS | `radar_wallet_transitions`. `radar_db::autonomous_discovery_from_an_empty_registry` asserts DISCOVERED → HISTORY_QUEUED → PRELIMINARY_QUALIFIED → PRIORITIZED → MONITORED |
| P03-T27 | Invalidated discovery evidence affects eligibility | PASS | `radar_db::event_redelivery_is_idempotent_and_retraction_invalidates_evidence`: the evidence is kept invalid with its reason, the valid count drops and the wallet is marked for evaluation |
| P03-T28 | Wallets with insufficient evidence remain identifiable as such | PASS | The one-off buyer ends `INSUFFICIENT_EVIDENCE` (`radar_db::autonomous_discovery_from_an_empty_registry`) |

### Historical investigation

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T29 | Initial historical investigation is bounded | PASS | Stage A: 7 days, 50 signatures, estimate 51. At most 2 concurrent history jobs and 25 investigations |
| P03-T30 | Progressive investigation depth is supported | PASS | `lifecycle::progressive_stages`; the trader reaches stage C in `radar_db::autonomous_discovery_from_an_empty_registry` |
| P03-T31 | Historical retrieval uses the existing Data Engine | PASS | History jobs are DATA ENGINE backfills (`radar_job_waiting` stores the backfill id) |
| P03-T32 | Previously stored evidence is reused | PASS | The DATA ENGINE does not refetch stored transactions. Reactivation does not repeat completed stages (`radar_db::dormancy_is_coverage_aware_and_new_activity_reactivates`) |
| P03-T33 | Duplicate history jobs are prevented | PASS | Unique open job per (class, subject). `radar_db::restart_resumes_from_durable_state_without_duplicate_jobs`, `radar_db::concurrent_claims_never_lease_a_job_twice` |
| P03-T34 | Historical coverage limitations are explicit | PASS | Nine coverage states, stored retrieval counts, and coverage shown as text in the inspector (RADAR.md §Historical investigation) |
| P03-T35 | Incomplete history is not presented as complete | PASS | A capped stage is `BOUNDED_SAMPLE_COMPLETE` or `REQUESTED_SCOPE_COMPLETE`, never complete history. Pending or unavailable transactions give `UNRESOLVED_GAPS` |
| P03-T36 | Investigations pause safely under budget constraints | PASS | `budget::degraded_or_unknown_budgets_pause`, `budget::exhausted_allowance_admits_nothing_costly`, `radar_db::degraded_or_unknown_budget_pauses_provider_work` (0 provider calls, 0 open jobs) |
| P03-T37 | Historical jobs recover after restart | PASS | `radar_db::restart_resumes_from_durable_state_without_duplicate_jobs` |
| P03-T38 | Cancellation preserves relevant durable evidence | PASS | `solprint radar cancel` marks the job cancelled. Stored transactions and evidence are not deleted (CLI smoke: `radar_operator_actions` recorded `APPLIED`). Implementation and CLI evidence; no dedicated DB test |
| P03-T39 | Unsupported transaction history is reported | PASS | `transactions_unsupported` is stored per wallet. When more than half are unsupported, coverage is `DECODER_COVERAGE_LIMITED` (`engine.rs` coverage function) |
| P03-T40 | Historical knowledge timestamps are preserved | PASS | Observations, evidence, transitions and snapshots record RADAR's knowledge time separately from block time (migration columns; RADAR.md) |

### Preliminary screening

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T41 | Verified activity metrics are calculated correctly | PASS | `screening::activity_windows_count_days_and_tokens` |
| P03-T42 | Multiple observation windows are supported | PASS | 24 h, 7 d, 30 d, 90 d and all-time windows (the same test) |
| P03-T43 | Aggregator legs are not double-counted | PASS | The DATA ENGINE publishes one user-level swap per route, and legs are not swaps (Phase 02 Jupiter route-leg tests). RADAR records one evidence row per swap |
| P03-T44 | Supported matched acquisition/disposal observations work | PASS | `screening::closed_cycle_with_fees_is_one_outcome` |
| P03-T45 | Partial disposals are handled correctly | PASS | `screening::partial_sells_allocate_cost_exactly` |
| P03-T46 | Unknown acquisition costs remain unknown | PASS | `screening::preexisting_inventory_has_unknown_cost_and_is_never_zero_cost` |
| P03-T47 | Unknown opening inventory cannot generate false profits | PASS | The same test, plus `screening::unknown_pre_balance_marks_inventory_ambiguous` and `screening::unobserved_outflow_taints_the_cycle` |
| P03-T48 | Incompatible financial denominations are not summed | PASS | `screening::denominations_are_never_mixed`, `model::native_and_wrapped_sol_are_one_denomination` |
| P03-T49 | Known attributable fees are handled correctly | PASS | `screening::fee_counts_only_when_the_wallet_paid_it_and_is_split`, `screening::usdc_cycles_flag_incomplete_fees` |
| P03-T50 | Missing historical prices remain unavailable | PASS | No historical price is fetched and no USD value is computed. Outcomes stay in their quote denomination (RADAR.md §Preliminary screening) |
| P03-T51 | Preliminary financial metrics preserve exact arithmetic | PASS | Raw `u64` with checked arithmetic and `Decimal` ratios, with no floating point (workspace lint `float_arithmetic`); `screening::partial_sells_allocate_cost_exactly` |
| P03-T52 | Priority calculations are deterministic | PASS | Pure function of stored inputs. An `input_hash` is stored per snapshot; `scoring::perfect_evidence_without_outcomes_scores_the_documented_value` |
| P03-T53 | Priority scores remain within defined bounds | PASS | `scoring::score_is_bounded` (0–100, 4 dp, clamped) |
| P03-T54 | Sample-size shrinkage behaves correctly | PASS | `scoring::performance_shrinks_toward_neutral_with_few_outcomes` |
| P03-T55 | Missing performance evidence cannot satisfy financial qualification | PASS | `scoring::qualification_is_ordered_and_evidence_gated` (`INSUFFICIENT_FINANCIAL_EVIDENCE`) |
| P03-T56 | Candidate qualification is separate from investigation priority | PASS | Separate fields, functions and policy; `scoring::qualification_is_ordered_and_evidence_gated` |
| P03-T57 | Priority components and policy versions are persisted | PASS | `radar_wallet_snapshots` stores the components, policy version and input hash. `radar_db::autonomous_discovery_from_an_empty_registry` asserts `evidenceQuality` in the snapshot |
| P03-T58 | Preliminary results are not presented as complete wallet profitability | PASS | The registry states that priority orders RADAR's own investigation work and is not a prediction or recommendation, and the inspector labels the entry an investigation state (UI `registry renders states as text and priority with its disclaimer`). Screening figures are per denomination and window, never a profitability total |

### Scheduling, monitoring and lifecycle

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T59 | Discovery scheduling operates autonomously | PASS | The four engine loops; `radar_db::autonomous_discovery_from_an_empty_registry` runs them without operator input |
| P03-T60 | Scheduler work and concurrency are bounded | PASS | Validated caps (samples, history jobs, investigations, pending jobs, monitored wallets); `radar_claim_jobs` honours `max_running` per dispatcher (see Limitations) |
| P03-T61 | Exploration allocation prevents candidate starvation | PASS | `market::unused_capacity_flows_to_other_buckets_and_exploration_keeps_a_floor` |
| P03-T62 | RADAR respects Gateway budget admission | PASS | `budget::share_of_remaining_minus_outstanding`, `budget::degraded_or_unknown_budgets_pause`, `radar_db::degraded_or_unknown_budget_pauses_provider_work`; GATEWAY also admits each request |
| P03-T63 | Protected API reserves are not consumed by ordinary discovery | PASS | The allowance is a share of the remaining *normal* allowance, which excludes GATEWAY's reserve (`budget::share_of_remaining_minus_outstanding`) |
| P03-T64 | Scheduler state survives restart | PASS | `radar_db::restart_resumes_from_durable_state_without_duplicate_jobs`; the pause control row survives (`radar_db::operator_controls_and_registry_queries`) |
| P03-T65 | Provider cooldowns are respected | PASS | `radar_db::provider_outage_is_recorded_and_local_sources_still_run` (`PROVIDER_COOLDOWN`, no retry within the cycle, local sources run) |
| P03-T66 | Effective monitoring subscription limits are enforced | PASS | `monitoring::fills_free_slots_by_priority`, `monitoring::capacity_reduction_releases_weakest` |
| P03-T67 | Subscription rotation avoids unnecessary churn | PASS | `monitoring::hysteresis_margin_and_minimum_tenure`, `monitoring::cooldown_and_disqualification` |
| P03-T68 | Inactive candidates are removed from expensive active workloads | PASS | `monitoring::quiet_wallets_leave_monitoring_and_are_not_started`; dormant and archived wallets get no refreshes |
| P03-T69 | Inactivity is distinguished from missing observation coverage | PASS | `lifecycle::dormancy_requires_established_inactivity`, `radar_db::dormancy_is_coverage_aware_and_new_activity_reactivates` |
| P03-T70 | Archived candidates retain historical evidence | PASS | Archival changes only the lifecycle; evidence, transitions and snapshots are kept (`lifecycle::archival_and_reactivation`; retention keeps evidence and transitions) |
| P03-T71 | Archived or dormant candidates can reactivate | PASS | `radar_db::dormancy_is_coverage_aware_and_new_activity_reactivates`; `solprint radar reactivate` |
| P03-T72 | Discovery-source effectiveness is measured | PASS | `radar_source_metrics` (runs, failures, unique and qualified wallets per source), served by `/api/v1/radar/sources` and shown in the workspace (`radar_db::provider_outage_is_recorded_and_local_sources_still_run` reads it) |

### Persistence and event integration

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T73 | Fresh database migrations pass | PASS | Every DB test migrates a fresh database; the Compose smoke test migrates the isolated stack ([Verification log](#verification-log)) |
| P03-T74 | Registry constraints prevent duplicate identities | PASS | Primary and unique keys on wallets, evidence (wallet, swap), sources and open jobs; `radar_scale` duplicate registration |
| P03-T75 | Concurrent scheduler claims are safe | PASS | `radar_db::concurrent_claims_never_lease_a_job_twice` (8 concurrent claimers, 40 jobs, each leased once) |
| P03-T76 | Data Engine events are consumed idempotently | PASS | Per-consumer delivery state; evidence keyed by (wallet, swap) with revision guards. `radar_db::event_redelivery_is_idempotent_and_retraction_invalidates_evidence` |
| P03-T77 | Candidate metrics react correctly to corrected swaps | PASS | A corrected swap updates its evidence to the new revision and never goes backwards. The wallet is re-evaluated (RADAR.md §Wallet extraction; same test) |
| P03-T78 | Retracted swaps invalidate affected preliminary conclusions | PASS | The same test: the evidence is invalidated, the valid count drops and `needs_evaluation` is set |
| P03-T79 | RADAR domain events use atomic outbox persistence | PASS | Events are written in the same transaction as the evaluation, with a unique (aggregate, revision, type) key. `radar_db::autonomous_discovery_from_an_empty_registry` asserts `WalletCandidateDiscovered` and `WalletMonitoringChanged` |
| P03-T80 | Phase 04 candidate query interfaces are operational | PASS | `query::RadarQuery` (`wallet`, `ready_for_analysis`, `monitored`, `evidence`) exercised in `radar_db::autonomous_discovery_from_an_empty_registry` |
| P03-T81 | Historical discovery and priority snapshots are preserved | PASS | Versioned `radar_wallet_snapshots` and market observations, retained 90 and 30 days (same test) |
| P03-T82 | Performance and capacity results are documented | PASS | [RADAR_CAPACITY_MODEL.md](../architecture/RADAR_CAPACITY_MODEL.md): measured CPU and registry figures (to 10,000 wallets) with machine and date; derived credit bounds labelled as derived |

### Interface and visual quality

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T83 | RADAR workspace is operational | PASS | `/radar` route; UI tests and visual baselines ([Verification log](#verification-log)) |
| P03-T84 | Candidate registry uses actual backend data | PASS | The workspace reads `/api/v1/radar/*` through TanStack Query; the API is served from storage (`http::radar_registry_is_served_from_local_state`). Test fixtures are labelled and synthetic |
| P03-T85 | Candidate filtering and pagination work | PASS | UI `lifecycle filter narrows the registry`; keyset pagination (`radar_scale` visits every wallet once) |
| P03-T86 | Candidate inspector exposes discovery evidence | PASS | UI `wallet inspector opens by keyboard, explains the score and returns focus on Esc` |
| P03-T87 | Priority breakdown is explainable | PASS | The inspector shows each weighted component and the policy version (same test) |
| P03-T88 | Historical investigation coverage is visible | PASS | Coverage state and history stage shown as text in the registry and inspector |
| P03-T89 | Missing financial values are rendered honestly | PASS | Missing or invalid figures render as `—`, never zero (`tests/unit/radar.test.ts`), and unknown states stay verbatim |
| P03-T90 | Discovery activity and source effectiveness are visible | PASS | Discovery cycles and sources panes (visual `radar-*`) |
| P03-T91 | Monitoring and budget states are accurate | PASS | UI `paused discovery and conservation budget are spelled out`; monitoring mode shown as text |
| P03-T92 | Approved all-black design identity is preserved | PASS | Shared tokens and components only; the existing baselines are unchanged |
| P03-T93 | Required responsive viewports pass | PASS | Visual `radar-*` at 1920, 1440, 1280, 1024 and 390; overflow check includes `/radar` |
| P03-T94 | Accessibility and keyboard checks pass | PASS | axe run on `/radar`; the keyboard inspector test |
| P03-T95 | Screenshot candidates are captured and inspected | PASS | 11 new candidates captured by `playwright.ps1 -Project visual` and inspected at each viewport (the mobile wallet controls were moved into the panel body after inspection) |
| P03-T96 | Project owner approves the new visual implementation | PENDING OWNER REVIEW | Owner actions 1 |

### Security and delivery

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P03-T97 | Empty-registry autonomous discovery test passes | PASS | `radar_db::autonomous_discovery_from_an_empty_registry`, 10 of 10 consecutive runs after the DATA ENGINE fix |
| P03-T98 | Real-provider bounded discovery results are honestly reported | NOT RUN | No live cycle was run; it needs the owner's opt-in (Owner actions 2). Nothing in this document is a live measurement |
| P03-T99 | Existing credentials and database data are preserved | PASS | `.env` was never read or modified; only key names were added to `.env.example`. Every DB test uses a disposable container. The schema change is a new forward-only migration |
| P03-T100 | Secret scanning and dependency audits pass | NOT RUN | Filled from `verify.ps1` ([Verification log](#verification-log)) |
| P03-T101 | Untrusted input and resource bounds are enforced | PASS | `http::radar_queries_are_validated_before_touching_storage`; API and CLI parse addresses with `WalletAddress::parse` and keyset cursors strictly (invalid input is 400, or exit 2 in the CLI); listings are bounded by GATEWAY response limits; all work is capped by validated configuration |
| P03-T102 | Real execution remains impossible | PASS | Live trading is rejected by configuration. GATEWAY refuses transaction-sending methods. RADAR has no signing or wallet code, and the `deny.toml` bans apply |
| P03-T103 | Complete local verification passes | NOT RUN | [Verification log](#verification-log) |
| P03-T104 | Isolated clean-checkout verification passes | NOT RUN | [Verification log](#verification-log) |
| P03-T105 | Documentation is complete | PASS | See "What was built" → Docs and the consolidation note |
| P03-T106 | No hosted CI/CD is introduced | PASS | No `.github/` directory; verification is local only |
| P03-T107 | No premature Phase 04 intelligence implementation is introduced | PASS | Priority is an investigation order and qualification a gate for deeper analysis. There are no wallet intelligence profiles, clustering, signals, copy trading or execution |
| P03-T108 | Feature branch is committed and pushed for owner review | NOT RUN | Filled after the push |

## Verification log

Entries are added when a run completes, with the exact commit it verified.
