# Phase 02 — DATA ENGINE

Branch: `feature/02-data-engine` (based on `develop` at `54c0f92`, Phase 01 merged).
Status: **implemented, awaiting owner review.** Not merged; no release or tag.

## Scope

In scope: the DATA ENGINE. It ingests Solana transactions durably through GATEWAY,
stores the raw evidence, reconstructs each transaction, and decodes the swaps of seven
programs:

- Jupiter v6;
- Pump.fun bonding curve;
- PumpSwap;
- Raydium AMM v4, CPMM and CLMM;
- Orca Whirlpool.

Around the decoders it also covers:

- normalized user-level swaps with route legs and fees;
- evidence-based wallet attribution;
- finality tracking and reconciliation, with corrections and retractions;
- bounded and resumable history backfills;
- a transactional outbox with versioned domain events;
- data-quality issues;
- reprocessing from stored evidence, and retention;
- a read-only API, the `solprint data-engine` CLI and a diagnostics script;
- the `/data` workspace;
- tests, benchmarks and documentation.

Out of scope (not implemented):

- wallet discovery, ranking, scoring or clustering (RADAR);
- token security analysis (FILTER);
- CORTEX, signals and strategies;
- GUARD, portfolio, backtesting, paper or real execution;
- wallets, keys, signing, broadcasting;
- USD valuation of swaps (see P02-T58);
- normalized amounts for liquidity operations;
- paid provider features;
- hosted CI/CD.

## What was built

| Area | Implementation |
|---|---|
| Crate | `solprint-data-engine`: evidence parsing, reconstruction (account keys, lookup tables, CPI tree), balances, token movements, seven protocol decoders behind a deterministic registry (`de-2026.11.1`), swap normalization, attribution, mapping to write sets and outbox events, the engine runtime (job workers, finality reconciler, backfills, stream intake, outbox dispatcher, retention) ([DATA_ENGINE.md](../architecture/DATA_ENGINE.md)) |
| GATEWAY | `getSignatureStatuses` (≤ 256, cost and source documented in [PROVIDER_CAPABILITIES.md](../architecture/PROVIDER_CAPABILITIES.md)) |
| Persistence | migration `20261127000003_data_engine.sql`: observations, jobs, evidence, transactions, balances, activities, swaps, route legs, fees, tokens, price and valuation tables (unused), quality issues, targets, backfills, outbox and per-consumer deliveries; `u64` as `NUMERIC(20,0)`, deltas as `NUMERIC(21,0)`, ratios as `NUMERIC(48,18)` |
| API | `GET /api/v1/data/{status,decoders,transactions,transactions/{signature},swaps,issues,targets,backfills,events}`, read from PostgreSQL only; `swaps?verified_only=true` |
| Configuration | 25 `DATA_ENGINE_*` keys with validated ranges and cross-checks against GATEWAY limits ([CONFIGURATION.md](../development/CONFIGURATION.md)); live intake off by default |
| CLI and tools | `solprint data-engine` (status, decoders, decode-fixture, jobs, issues, targets, ingest, backfill with a credit estimate and `--yes`, reprocess); `scripts/data-engine-diagnostics.ps1` (offline by default, `-Live -Signature` opt-in) |
| Frontend | `/data` workspace with these panes: ingestion, engine state, swaps, transactions, data quality, targets, backfills, domain events and decoder coverage. A transaction inspector drawer shows evidence, balances, activities, swaps and issues. Every pane handles all data states ([UI_DESIGN_SYSTEM.md](../design/UI_DESIGN_SYSTEM.md#data-engine-workspace-data-phase-02)) |
| Tests | 17 synthetic fixtures with expected outcomes, unit and property tests, 15 PostgreSQL engine tests with an in-memory chain source, API tests, UI/visual/e2e tests, smoke checks |
| Docs | DATA_ENGINE, DECODER_CAPABILITIES, TRANSACTION_RECONSTRUCTION, SWAP_NORMALIZATION, WALLET_ATTRIBUTION, FINALITY_AND_RECONCILIATION, DATA_ENGINE_CAPACITY_MODEL (new); GATEWAY, PROVIDER_CAPABILITIES, MODULE_BOUNDARIES, DATA_CONVENTIONS, ARCHITECTURE, CONFIGURATION, TESTING, TROUBLESHOOTING, UI_DESIGN_SYSTEM, ROADMAP, README, CLAUDE.md (updated) |

## Defects found and fixed during verification

The PostgreSQL engine tests found defects that unit tests could not, because they run
the real schema and constraints:

- The finality reconciler's outbox events violated outbox constraints (`674eecd`).
- A backfill paused by shutdown (`PAUSED`) was never resumed.
- Swap retraction wrote a reason that the schema rejects, so a correction that removed a
  swap could not persist.
- A job whose processing errored stayed leased until its lease expired.

The last three are fixed in `ca204e9`. A pane-width defect in the Backfills pane at
≥ 1200 px is fixed in `09743be`.

## Limitations and open items

- **No live provider verification.** Every decoder is verified against synthetic
  fixtures built from the programs' published layouts and IDLs (source commits in
  [DECODER_CAPABILITIES.md](../architecture/DECODER_CAPABILITIES.md)), not against
  captured mainnet transactions. The bounded live diagnostic
  (`data-engine-diagnostics.ps1 -Live -Signature <SIG>`, at most 1 credit) needs the
  owner's opt-in and has not been run (P02-T91).
- **USD valuation is not implemented.** Swaps report `NOT_ENRICHED`, or
  `PRICE_UNAVAILABLE` when enrichment is configured on. The valuation tables exist but are
  never written (P02-T58).
- `ORPHANED` is never inferred automatically. A missing or mismatching status leads to
  `FINALITY_UNKNOWN` / `POSSIBLY_ORPHANED` and a finalized refetch.
- The attribution states `MULTIPLE_POSSIBLE_ACTORS`, `VERIFIED_BENEFICIARY` and
  `AMBIGUOUS` exist in the contract but no Phase 02 rule produces them. Attribution reads
  one protocol authority account, and the beneficiary is recorded separately.
- Liquidity operations and pool lifecycle are recorded as `PARTIALLY_DECODED` activities
  without normalized amounts.
- **Desktop visual baselines show only the first viewport.** The shell scrolls an inner
  container, so `fullPage` screenshots at ≥ 1024 px stop at the viewport height. At
  390 px the full page is captured. This predates Phase 02 and affects every page's
  desktop baselines. Fixing it would change the approved Phase 01 baselines, so it is
  left for an owner decision.
- The partial index `de_backfills_runnable_idx` does not include `PAUSED`. Claiming
  still works, because the claim query does not depend on the index. Changing the index
  needs a new migration.
- The outbox has one internal consumer (`diagnostic_log`). Later modules will register
  their own consumers.
- Capacity: only CPU microbenchmarks were measured. End-to-end throughput, mainnet
  evidence sizes and stream rates are not measured
  ([DATA_ENGINE_CAPACITY_MODEL.md](../architecture/DATA_ENGINE_CAPACITY_MODEL.md)).

## Owner actions

1. **Review and approve the visual candidates** (P02-T85). New:
   - `data-{1920x1080,1440x900,1280x800,1024x768,390x844}`;
   - `data-empty-{1440x900,390x844}`;
   - `data-database-down-{1440x900,390x844}`.

   Updated, because the navigation gained "Data engine":
   - `overview-1024x768`, `overview-390x844`;
   - `overview-offline-1440x900`, `overview-offline-390x844`.

   Approval covers the files in `frontend/tests/visual/__screenshots__/pages.spec.ts/`
   at the reviewed commit.
2. Optionally authorise one bounded live diagnostic:
   `./scripts/data-engine-diagnostics.ps1 -Live -Signature <SIG>` (one `getTransaction`,
   at most 1 Helius credit).
3. Credential rotation remains the owner's responsibility (see Phase 01).

## Acceptance criteria

Statuses: PASS / FAIL / BLOCKED / NOT RUN / PENDING OWNER REVIEW.

Test references:

- `engine_db::<name>` is `backend/crates/solprint-data-engine/tests/engine_db.rs`
  (PostgreSQL, disposable database, in-memory chain source);
- `fixtures::<name>` and other `module::<name>` refer to unit tests in
  `backend/crates/solprint-data-engine/src`;
- "fixture `x`" is the synthetic catalogue entry with that name.

No criterion claims verification against a live provider.

### Architecture and ingestion

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P02-T01 | Correct feature branch is active | PASS | Work is committed on top of `develop` (`54c0f92`) and published as `origin/feature/02-data-engine`. It was developed in an isolated worktree because `feature/02-data-engine` is checked out in the main checkout |
| P02-T02 | Phase 00 and Phase 01 regressions pass | NOT RUN | Pending `verify.ps1`: all Phase 00/01 Rust, Vitest, Playwright ui/visual/e2e and smoke checks ([Verification log](#verification-log)) |
| P02-T03 | Data Engine module boundaries are documented | PASS | [MODULE_BOUNDARIES.md](../architecture/MODULE_BOUNDARIES.md) rule 7 and the dependency table; [DATA_ENGINE.md](../architecture/DATA_ENGINE.md#module-layout) |
| P02-T04 | All external communication uses GATEWAY | PASS | The engine depends only on the `ChainSource` trait, implemented by `GatewaySource` over `solprint_gateway::Gateway` (`source.rs`). `solprint-data-engine/Cargo.toml` has no HTTP or WebSocket client |
| P02-T05 | Explicit ingestion targets are supported | PASS | `de_targets` and `solprint data-engine targets add/enable/disable`. None are created automatically. Live intake subscribes only to enabled targets, bounded by `DATA_ENGINE_MAX_ACTIVE_TARGETS`. `engine_db::stream_gaps_stay_visible_and_live_logs_become_jobs` uses a target |
| P02-T06 | Live transaction observations are consumed correctly | PASS | `engine_db::stream_gaps_stay_visible_and_live_logs_become_jobs`: a GATEWAY `Logs` event becomes a `LIVE_STREAM` job attributed to its target. GATEWAY's own subscription and recovery are tested in `gateway_stream.rs` (Phase 01) |
| P02-T07 | Transaction retrieval is durably queued | PASS | `de_jobs` in PostgreSQL with leases (`FOR UPDATE SKIP LOCKED`); `engine_db::crashed_leases_and_undelivered_events_survive_a_restart` |
| P02-T08 | Worker concurrency and queues are bounded | PASS | Validated ranges, with fetch concurrency at or below `HELIUS_MAX_IN_FLIGHT` (config tests). `engine_db::a_full_queue_refuses_intake_and_pauses_backfills_visibly`: at capacity, intake is refused with `QUEUE_SATURATED` and backfills pause before fetching |
| P02-T09 | Temporary retrieval failures are handled safely | PASS | `engine_db::budget_denial_releases_without_consuming_an_attempt` and `engine_db::unknown_transaction_is_retried_and_never_invented`. Transient errors use bounded backoff (`engine::backoff_is_bounded_and_monotonic`). Processing errors are handed back as `JOB_ERROR` (`ca204e9`) |
| P02-T10 | Restart recovers unfinished ingestion work | PASS | `engine_db::crashed_leases_and_undelivered_events_survive_a_restart` (expired lease → `FETCH_RETRY` → processed once); `engine_db::backfill_resumes_from_its_checkpoint_after_shutdown_and_budget_pause` |
| P02-T11 | Duplicate observations do not cause duplicate retrieval work | PASS | `engine_db::ingest_persists_once_and_reprocessing_is_idempotent` (`DuplicateObservation`, `Existing`); `engine_db::out_of_order_and_late_observations_never_regress_or_refetch` (four observations, one retrieval) |
| P02-T12 | Source provenance is preserved | PASS | Every observation is stored in `de_observations` with source, target, backfill, commitment, slot, stream generation and observed time. Evidence keeps provider, commitment, fetch time and SHA-256, and events keep source and observed times. Exercised by the engine tests above |

### Transaction reconstruction and financial correctness

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P02-T13 | Legacy transactions reconstruct correctly | PASS | 16 of the 17 fixtures are legacy (`version: "legacy"`) and decode to their expected outcomes (`fixtures::every_fixture_decodes_to_its_expected_outcome`, `engine_db::every_fixture_persists_within_schema_constraints_and_reads_back`) |
| P02-T14 | Versioned transactions reconstruct correctly | PASS | Fixture `raydium_clmm_swap_v2_alt` (v0, lookup table) decodes and persists as expected |
| P02-T15 | Loaded account addresses resolve correctly | PASS | The same fixture resolves writable and readonly loaded addresses in runtime order (`reconstruct::account_order_and_flags_follow_the_runtime_rules`); missing loaded addresses fail (`reconstruct::bad_indices_and_missing_loaded_addresses_fail`) |
| P02-T16 | Inner instructions retain appropriate context | PASS | `reconstruct::stack_heights_build_the_cpi_tree`; `reconstruct::missing_stack_height_and_inner_are_reported_not_guessed`; fixture `no_stack_height`; Jupiter subtree claim (`fixtures::jupiter_route_has_two_legs_and_no_nested_swaps`) |
| P02-T17 | Invalid account indexes are rejected safely | PASS | `reconstruct::bad_indices_and_missing_loaded_addresses_fail`; `evidence::arbitrary_text_never_panics` (property test) |
| P02-T18 | Native SOL balance changes are accurate | PASS | `balances::delta_is_exact` (property test, `i128`); `engine_db::extreme_amounts_round_trip_exactly_through_the_schema` (`u64::MAX` pre-balance, exact −5000 delta) |
| P02-T19 | SPL token balance changes are accurate | PASS | `fixtures::token_2022_fee_is_netted_and_reported`; `engine_db::extreme_amounts_round_trip_exactly_through_the_schema` (±`u64::MAX` token deltas) |
| P02-T20 | Token account creation and closure are handled correctly | PASS | Fixture `wrap_and_close_wsol` and `fixtures::wrap_flow_is_classified` (`ACCOUNT_CREATION`, `WRAP_SOL`, `UNWRAP_SOL`); absent-side balances are deltas from zero |
| P02-T21 | WSOL wrapping and unwrapping avoid double-counting | PASS | `fixtures::wrap_flow_is_classified`; native-side rule in [SWAP_NORMALIZATION.md](../architecture/SWAP_NORMALIZATION.md); Pump.fun and PumpSwap fixtures report one SOL side |
| P02-T22 | Network fees and rent movements are distinguished | PASS | `fixtures::failed_transactions_produce_no_swaps_but_a_fee` (`FEE` activity); `fixtures::pump_fees_are_explicit`; rent via `ACCOUNT_CREATION` / closure ([TRANSACTION_RECONSTRUCTION.md](../architecture/TRANSACTION_RECONSTRUCTION.md)) |
| P02-T23 | Token amounts preserve exact base units | PASS | Raw `u64` end to end, strings on the wire, `BigInt` in the UI. Every fixture's expected raw amounts match after persistence (`engine_db::ingest_persists_once_and_reprocessing_is_idempotent`) |
| P02-T24 | Database numeric representations support required ranges | PASS | Domains `u64_amount NUMERIC(20,0)` and `i65_delta NUMERIC(21,0)` with range checks; `engine_db::extreme_amounts_round_trip_exactly_through_the_schema` |
| P02-T25 | Financial overflow and invalid conversion tests pass | PASS | `balances::delta_is_exact`, `normalize::ratio_never_panics_and_is_positive`, `borsh::never_panics_on_arbitrary_input` (property tests); `solprint-core` amount overflow tests |

### Protocol decoding and attribution

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P02-T26 | Decoder registry is deterministic and versioned | PASS | `Registry::standard()` is a fixed order and decoder version `de-2026.11.1` is stored on every record. Decoding twice gives identical ids (`fixtures::every_fixture_decodes_to_its_expected_outcome`). `programs::anchor_discriminators_match_their_preimages`; `programs::pump_discriminators_match_the_official_idl_arrays` |
| P02-T27 | Supported Jupiter swaps decode correctly | PASS | Fixture `jupiter_route_two_legs`; `fixtures::jupiter_route_has_two_legs_and_no_nested_swaps` |
| P02-T28 | Supported Pump.fun trades decode correctly | PASS | Fixture `pump_fun_buy`; `pump::parses_old_and_new_trade_events`; `fixtures::pump_fees_are_explicit` |
| P02-T29 | Supported PumpSwap trades decode correctly | PASS | Fixture `pumpswap_sell` |
| P02-T30 | Supported Raydium swaps decode correctly | PASS | Fixtures `raydium_amm_v4_swap`, `raydium_cpmm_swap`, `raydium_clmm_swap_v2_alt` |
| P02-T31 | Supported Orca swaps decode correctly | PASS | Fixtures `orca_whirlpool_swap`, `orca_two_hop_swap` |
| P02-T32 | Supported multi-hop routes are represented correctly | PASS | `orca_two_hop_swap` is one swap with two legs, and the Jupiter route is one swap with two legs in execution order |
| P02-T33 | Aggregator legs are not double-counted as wallet trades | PASS | `fixtures::jupiter_route_has_two_legs_and_no_nested_swaps` (exactly one swap; legs are not transfers or swaps) |
| P02-T34 | Verified user-level swap amounts reconcile with evidence | PASS | Every fixture's input and output match its expected raw amounts. Fixture `amount_mismatch` (transfer ≠ balance delta) gives `PARTIALLY_DECODED` and `DECODER_AMOUNT_MISMATCH` |
| P02-T35 | Wallet attribution uses verified account and authority evidence | PASS | [WALLET_ATTRIBUTION.md](../architecture/WALLET_ATTRIBUTION.md): authority by verified account position and only if it signed; ownership from the transaction's own evidence |
| P02-T36 | Fee payer is not incorrectly assumed to be trader | PASS | Fixture `relayed_swap_fee_payer_differs`; `fixtures::relayer_fee_payer_is_not_the_wallet` |
| P02-T37 | Multiple possible actors are handled explicitly | PASS | Several signers (relayer and user) are resolved by the protocol authority, never the fee payer. A non-signing authority becomes `PROGRAM_CONTROLLED` with no wallet (`fixtures::program_controlled_swaps_have_no_wallet`). A differing beneficiary is recorded separately with `BENEFICIARY_DIFFERS`. Limitation: `MULTIPLE_POSSIBLE_ACTORS` is never produced, because attribution reads one authority account |
| P02-T38 | Ordinary transfers are not misclassified as swaps | PASS | `fixtures::plain_transfers_are_not_swaps`; fixtures `token_2022_transfer_fee` and `wrap_and_close_wsol` produce no swaps |
| P02-T39 | Liquidity operations are not misclassified as swaps | PASS | `fixtures::liquidity_operations_are_not_swaps` (Orca `increase_liquidity` → `LIQUIDITY_DEPOSIT`, no swap) |
| P02-T40 | Failed transactions cannot produce successful swaps | PASS | Fixture `failed_swap`; `fixtures::failed_transactions_produce_no_swaps_but_a_fee`; `engine_db::corrected_evidence_retracts_swaps_under_a_new_revision` (a transaction found to have failed retracts its swap) |
| P02-T41 | Unsupported protocols retain evidence and explicit status | PASS | Fixture `unsupported_program` is `UNSUPPORTED` with an issue, and its evidence is persisted (`engine_db::every_fixture_persists_within_schema_constraints_and_reads_back`); the UI states it |
| P02-T42 | Ambiguous activities are excluded from verified trade queries | PASS | `swaps?verified_only=true` and the verified total use the same rule. `engine_db::every_fixture_persists_within_schema_constraints_and_reads_back` checks that the query excludes `program_controlled_swap`, `missing_inner_instructions` and `amount_mismatch`, and that it matches the verified count |
| P02-T43 | Token-2022 transfer fees are handled accurately for supported cases | PASS | Fixture `token_2022_transfer_fee`; `fixtures::token_2022_fee_is_netted_and_reported` (1,000,000 sent, 990,000 received) |
| P02-T44 | Unsupported token extensions are explicitly identified | PASS | `fixtures::unsupported_token_2022_extensions_are_flagged` (`UNSUPPORTED_TOKEN_EXTENSION`) |

### Finality, history and pricing

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P02-T45 | Commitment and execution status remain separate | PASS | Separate `execution_status` and `finality` columns, contracts and UI columns. Fixture `failed_swap` is `FAILED` + finalized ([FINALITY_AND_RECONCILIATION.md](../architecture/FINALITY_AND_RECONCILIATION.md)) |
| P02-T46 | Confirmed transactions can be reconciled to finalized | PASS | `engine_db::finality_advances_and_unknown_status_is_never_orphaned` (`TransactionFinalized` event) |
| P02-T47 | Late evidence does not regress finality | PASS | The same test (a later `null` status leaves it `FINALIZED`); `engine_db::out_of_order_and_late_observations_never_regress_or_refetch` |
| P02-T48 | Missing signature status is not treated as definitive orphaning | PASS | `engine_db::finality_advances_and_unknown_status_is_never_orphaned` (`FINALITY_UNKNOWN`, finalized refetch queued); `engine_db::invalid_evidence_is_quarantined_and_slot_mismatch_is_not_orphaned` |
| P02-T49 | Authoritative corrections and retractions are supported | PASS | `engine_db::corrected_evidence_retracts_swaps_under_a_new_revision`: revision 2, superseded evidence kept, swap retracted with `EVIDENCE_REVISION` and a `SwapRetracted` event at revision 2 |
| P02-T50 | Out-of-order observations are processed correctly | PASS | `engine_db::out_of_order_and_late_observations_never_regress_or_refetch` (priority only rises, one retrieval, no regression) |
| P02-T51 | Historical pagination is bounded and correct | PASS | `engine_db::backfill_is_bounded_resumable_and_deduplicated` (cursor walk, slot range, exhaustion); configured page, page-count and signature bounds; the CLI prints the credit upper bound before creating |
| P02-T52 | Backfill checkpoints survive restart | PASS | `engine_db::backfill_resumes_from_its_checkpoint_after_shutdown_and_budget_pause`: a new engine resumes from the stored cursor without refetching page one |
| P02-T53 | Live and historical observations deduplicate correctly | PASS | The same test (a signature seen live is counted as a duplicate, not queued again); `engine_db::backfill_is_bounded_resumable_and_deduplicated` |
| P02-T54 | Backfills pause safely when API budgets require it | PASS | The same test (`PAUSED_BY_BUDGET`, cursor kept, resumed later) |
| P02-T55 | Unresolved historical gaps remain visible | PASS | `engine_db::stream_gaps_stay_visible_and_live_logs_become_jobs` (`STREAM_GAP` and `CONTINUITY_UNKNOWN` stay open on the target); `engine_db::a_page_limited_backfill_reports_partial_history` (open `BACKFILL_INCOMPLETE`) |
| P02-T56 | Historical coverage limitations are represented accurately | PASS | `ADDRESS_HISTORY_EXHAUSTED` only at the provider's end of history; `PARTIAL_HISTORY` at a limit (`engine_db::a_page_limited_backfill_reports_partial_history`); shown in the Targets and Backfills panes |
| P02-T57 | Observed trade exchange ratios are calculated correctly | PASS | `normalize::ratio_accounts_for_decimals`, `fixtures::exchange_ratio_uses_decimals`; exact `Decimal`, rounded toward zero to 18 places |
| P02-T58 | Historical USD values do not use future price observations | NOT RUN | USD valuation is not implemented, and no USD value is computed or shown. The schema allows only `CONTEMPORANEOUS_REFERENCE_PRICE` with `price_age_ms >= 0`, but no code writes valuations, so nothing could be tested |
| P02-T59 | Missing prices remain unknown rather than fabricated | PASS | Every swap's valuation is `NOT_ENRICHED` (or `PRICE_UNAVAILABLE`) with null values (`data_routes.rs`); the UI states "No USD values are shown" (`tests/ui/shell.spec.ts`) |
| P02-T60 | Point-in-time knowledge timestamps are preserved | PASS | `block_time` (source), `first_observed_at`, `decoded_at`, `finalized_at`, `corrected_at` on swaps; observation and evidence fetch times; event source, observed and persisted times ([DATA_CONVENTIONS.md](../architecture/DATA_CONVENTIONS.md#chain-data-data-engine)) |

### Persistence and event delivery

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P02-T61 | Fresh database migrations succeed | PASS | Every `engine_db` test and storage test migrates a fresh database; the isolated Compose stack in `verify.ps1` migrates from empty |
| P02-T62 | Transaction evidence and normalized activities persist atomically | PASS | `de_persist_transaction` writes evidence, the transaction, balances, activities, swaps, legs, fees, issues, outbox events and the job outcome in one database transaction, guarded by revision. `engine_db::every_fixture_persists_within_schema_constraints_and_reads_back` |
| P02-T63 | Stable identities prevent duplicate economic records | PASS | `swap:<signature>:<instruction_ref>` ids; `engine_db::ingest_persists_once_and_reprocessing_is_idempotent` (reprocessing writes nothing new) |
| P02-T64 | Required historical and real-time queries work | PASS | `engine_db::every_fixture_persists_within_schema_constraints_and_reads_back` runs every read model the API uses. `tests/http.rs` covers the API. The e2e test `data engine workspace reads the real, persisted state` runs against the real stack |
| P02-T65 | Decoder revisions support safe reprocessing | PASS | Reprocessing uses stored evidence only (no fetch: `engine_db::ingest_persists_once_and_reprocessing_is_idempotent`). A new decoder version writes revision n+1 with `DECODER_REVISION` retractions (`mapping::retracted_swaps_emit_events`), through the same persist path that `engine_db::corrected_evidence_retracts_swaps_under_a_new_revision` exercises |
| P02-T66 | Raw evidence retention is bounded and documented | PASS | `DATA_ENGINE_EVIDENCE_MAX_BYTES` (hard cap, quarantine above it); retention of finalized raw JSON, jobs and delivered events ([DATA_ENGINE.md](../architecture/DATA_ENGINE.md#retention), [CONFIGURATION.md](../development/CONFIGURATION.md)); retention runs in `engine_db::every_fixture_persists_within_schema_constraints_and_reads_back` |
| P02-T67 | Transactional outbox publication is atomic | PASS | Events are inserted in the same database transaction as the change they describe (P02-T62). `engine_db::ingest_persists_once_and_reprocessing_is_idempotent` checks one `SwapRecorded` per swap and none on reprocessing |
| P02-T68 | Duplicate event delivery is handled idempotently | PASS | `engine_db::crashed_leases_and_undelivered_events_survive_a_restart` (a duplicate or late acknowledgement never undoes a delivery; delivery is per consumer). Events carry id and aggregate revision for consumer deduplication |
| P02-T69 | Event delivery survives application restart | PASS | The same test (a new engine delivers the pending events, each once) |
| P02-T70 | Corrections invalidate affected canonical projections | PASS | `engine_db::corrected_evidence_retracts_swaps_under_a_new_revision` (the retracted swap is non-canonical and excluded from default and verified listings) |
| P02-T71 | Database and queue failure scenarios recover safely | PASS | Lease recovery, budget denial, `JOB_ERROR` hand-back, queue saturation (`engine_db::a_full_queue_refuses_intake_and_pauses_backfills_visibly`); with the database down the API returns a 503 envelope and degrades `status` truthfully (`tests/http.rs`); the UI database-down state is tested |
| P02-T72 | Performance and capacity results are documented | PASS | [DATA_ENGINE_CAPACITY_MODEL.md](../architecture/DATA_ENGINE_CAPACITY_MODEL.md): measured decode 31.8 µs and write set 12.8 µs per transaction (catalogue mix, one machine); derived credit arithmetic; unmeasured items listed |

### Interface and visual quality

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P02-T73 | Data Engine monitoring workspace is operational | PASS | `/data` against mocks (`tests/ui/shell.spec.ts` › data engine workspace) and against the real stack (e2e `data engine workspace reads the real, persisted state`) |
| P02-T74 | Transaction explorer displays actual persisted evidence | PASS | The inspector shows evidence revisions with SHA-256, balances and activities from `/api/v1/data/transactions/{signature}` (UI test `inspector opens by keyboard, shows evidence and returns focus on Esc`) |
| P02-T75 | Swap inspector displays normalized amounts and attribution | PASS | UI test `swaps render exact amounts, attribution and finality as text` (`BigInt` amounts, attribution words, unknown as `—`) |
| P02-T76 | Route information accurately represents available evidence | PASS | The inspector shows route completeness and legs with raw amounts and evidence kind, and nothing when no legs were proven ([UI_DESIGN_SYSTEM.md](../design/UI_DESIGN_SYSTEM.md#data-engine-workspace-data-phase-02)) |
| P02-T77 | Backfill progress and limitations are visible | PASS | Backfills pane: status, stop reason, coverage, pages of maximum, queued and duplicate counts, credits spent against the estimate (inspected at 390, 1440 and 1920 px) |
| P02-T78 | Decoder coverage and quality issues are visible | PASS | Decoder coverage pane (UI test `decoder coverage lists every decoded program, even with the database down`); Data quality pane by severity |
| P02-T79 | Unknown, provisional and ambiguous states render correctly | PASS | UI tests: insufficient evidence, program-controlled, finality unknown, possibly orphaned and retracted render as words plus glyphs; empty, loading, offline and database-down states |
| P02-T80 | Approved all-black visual identity is preserved | PASS | Only shared tokens and components are used (no new colours, gradients or local styles); colour only with meaning, always paired with text. Visual candidates inspected |
| P02-T81 | No generic AI-dashboard aesthetics are introduced | PASS | Dense tables and definition lists in the existing TERMINAL style; no decorative charts, cards or hero metrics. Owner judgement is still pending under P02-T85 |
| P02-T82 | Required responsive viewports pass | PASS | No horizontal overflow on `/data` at all five standard viewports (UI tests); visual baselines at 1920/1440/1280/1024/390 |
| P02-T83 | Accessibility and keyboard checks pass | PASS | axe clean in data states; keyboard inspector with focus return (`c5bf345`); `tests/ui` 56/56 |
| P02-T84 | Screenshot candidates are captured and inspected | PASS | Candidates generated in the pinned container and inspected, including the decoder pane and the Backfills fix at 390, 1440 and 1920 px. The desktop first-viewport limitation is recorded above |
| P02-T85 | Project owner approves the new visual implementation | PENDING OWNER REVIEW | See Owner actions 1 |

### Security and delivery

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| P02-T86 | Existing `.env` and database data are preserved | PASS | `.env` was never read or modified; only key names were added to `.env.example`. Every database test uses a disposable container. The schema change is a new forward-only migration |
| P02-T87 | Secret scanning and dependency audits pass | NOT RUN | `verify.ps1`: secret scan, cargo deny (advisories, bans, licences, sources), npm audit ([Verification log](#verification-log)) |
| P02-T88 | Malformed transaction data cannot crash normal processing | PASS | Property tests (`evidence::arbitrary_text_never_panics`, `borsh::never_panics_on_arbitrary_input`); `engine_db::invalid_evidence_is_quarantined_and_slot_mismatch_is_not_orphaned` (quarantined, workers continue) |
| P02-T89 | External API consumption remains controlled by GATEWAY | PASS | Budget admission and priorities for every fetch, finality and backfill call. The smoke check `data engine endpoints answer from local state without provider traffic` passes. The CLI and diagnostics contact a provider only with `--live` / `-Live` |
| P02-T90 | Live execution remains impossible | PASS | Live trading is rejected by configuration. GATEWAY refuses transaction-sending methods. The engine has no signing or wallet code, and the `deny.toml` bans pass. `/api/v1/data/status` reports `execution_capability: NONE` (smoke, e2e) |
| P02-T91 | Real-provider diagnostics are accurately reported | NOT RUN | No live diagnostic was run; it needs the owner's opt-in (Owner actions 2). Nothing in this document is a live measurement |
| P02-T92 | Complete local verification passes | NOT RUN | `check.ps1`, `test.ps1`, Playwright ui and visual ([Verification log](#verification-log)) |
| P02-T93 | Isolated clean-checkout verification passes | NOT RUN | `verify.ps1` full mode on the candidate commit ([Verification log](#verification-log)) |
| P02-T94 | Documentation is complete and accurate | PASS | See "What was built" → Docs |
| P02-T95 | No hosted CI/CD is introduced | PASS | No `.github/` directory; verification is local only |
| P02-T96 | No future-phase intelligence functionality is prematurely implemented | PASS | No ranking, scoring, clustering, signals, token analysis or execution. Attribution is per swap only. Outbox events are published for later consumers, but no consumer module exists |

## Verification log

Entries are added when a run completes, with the exact commit it verified.
