# RADAR — autonomous wallet discovery (Phase 03)

RADAR finds Solana memecoin markets on its own, samples their recent transactions
**through the DATA ENGINE**, records the wallets that the DATA ENGINE verified as swap
owners, and keeps a durable, explainable registry of research candidates. It investigates
those candidates progressively under a bounded provider budget.

RADAR is read-only research bookkeeping. It never builds, signs or submits a transaction.
It holds no keys, does not identify the people behind wallets, and produces no trading
signal. A candidate's priority and qualification only order RADAR's own investigation
work. They are not predictions, recommendations or measures of skill.

This document consolidates the topics the phase plan listed separately: discovery
sources, market universe, wallet registry, historical investigation, preliminary
screening and scheduler. Measured and derived cost figures are in
[RADAR_CAPACITY_MODEL.md](RADAR_CAPACITY_MODEL.md).

## Placement and boundaries

| Crate | Role |
|---|---|
| `solprint-radar` | engine, policies (`classify`, `market`, `screening`, `scoring`, `lifecycle`, `monitoring`, `budget`), DATA ENGINE event consumer, `RadarQuery` |
| `solprint-gateway` | the only provider client: Jupiter Tokens V2 category/recent listings, DEX Screener pairs and boosts, Helius `getMultipleAccounts` for pool owner checks |
| `solprint-data-engine` | fetches, decodes and verifies every transaction RADAR learns from (sampling and history are DATA ENGINE backfills) |
| `solprint-storage` | `radar_*` tables and queries (migration `20261127000004_radar.sql`) |
| `solprint-api` | read-only `GET /api/v1/radar/*` |
| `solprint-app` | composition (RADAR starts after the DATA ENGINE and stops before it) and the `solprint radar` CLI |

The following rules are enforced in code:

- **No direct provider access.** RADAR calls GATEWAY's typed traits through a
  `MarketFeed` and has no HTTP client.
- **No transaction fetching.** RADAR never fetches a transaction itself. It requests a
  bounded DATA ENGINE backfill (a pool's signatures for sampling, a wallet's signatures
  for history).
- **Wallets come only from DATA ENGINE events.** RADAR learns about wallets from the
  DATA ENGINE's outbox events (`SwapRecorded`, `SwapRetracted`, `TransactionFinalized`),
  consumed as the registered consumer `radar`. It never reads provider responses or
  decoder internals.
- **Later phases read through `RadarQuery`.** It is read-only:
  - `wallet` and `evidence` for one address;
  - `ready_for_analysis(after, limit)`, keyset-paginated;
  - `monitored(limit)`.

## Discovery sources

Each source is a `Listing` with a fixed source code. Every market observation records
its source, rank, observed time and whether the listing is promotional.

| Source | Provider (via GATEWAY) | Bucket fed | Notes |
|---|---|---|---|
| `JUPITER_TOP_TRADED`, `JUPITER_TOP_ORGANIC` | Jupiter Tokens V2 category listings (`toptraded`, `toporganicscore`) | established | provider-ranked; Jupiter excludes generic tokens |
| `JUPITER_RECENT` | Jupiter Tokens V2 `/recent` | emerging | mints whose first pool was created recently |
| `DEXSCREENER_BOOSTS` | DEX Screener boosts | — | **promotional**: recorded with `promotional_only`; never counted as demand; off unless `DEXSCREENER_DISCOVERY_ENABLED=true` |
| `LOCAL_ACTIVITY` | none (local DATA ENGINE swaps) | local activity | mints the DATA ENGINE already observed |
| `HISTORICAL` | none | historical | markets that earlier yielded verified wallets |
| `EXPLORATION` | none | exploration | a deterministic, seed-rotated pick from every screened market, so low-ranked markets are not starved |

Every source run is recorded in `radar_source_runs` with one of these statuses: `OK`,
`EMPTY`, `DISABLED`, `BUDGET_DEFERRED`, `PROVIDER_COOLDOWN` or `FAILED`.

- **A failing source does not stop the cycle.** Local sources still run when a provider
  fails.
- **A provider cooldown is respected.** When GATEWAY reports a rate limit or an
  unavailable provider, the source run is recorded as `PROVIDER_COOLDOWN`. RADAR does not
  retry inside the cycle. GATEWAY itself honours `Retry-After`.

## Market universe

**Identity.** Tokens are identified by mint address. Symbols and names are display hints
only. One token can have several pools.

**Pools.**
- Pools come from DEX Screener pairs. Each pool keeps its own provider-reported
  liquidity, 24 h volume, 1 h and 24 h transaction counts, observed time, and sampling
  history.
- RADAR samples a market through its best *supported* pool, the one with the highest
  reported liquidity.
- **Pool validation.**
  - `getMultipleAccounts` (up to 100 accounts per call, budget-admitted) confirms that
    the pool account exists and is owned by a program the DATA ENGINE decodes.
  - The result is `SUPPORTED`, `UNSUPPORTED_PROGRAM` or `ACCOUNT_NOT_FOUND`. Until it is
    checked, the pool is `UNVERIFIED`.
  - Unsupported pools are never sampled.

**Classification** (`classify`, ruleset `radar-class-1`). Rules apply in order; the first
match wins, and its reason is stored.

1. Known mints: stablecoins (USDC, USDT and others), wrapped SOL, known liquid-staking
   tokens, and established assets. These are **excluded from sampling**.
2. Provider tags for liquid-staking, stablecoin and LP tokens. These are also excluded.
3. Launchpad evidence reported by the provider, or a pool on a bonding-curve protocol
   (`PUMP_FUN`, `PUMPSWAP`), marks a `MEME_CANDIDATE`.
4. A young token with no verification marks a `GENERAL_SPECULATIVE_TOKEN`.
5. Anything else is `UNKNOWN`.

Classification is explainable bookkeeping, not a quality rating.

**Screening** (`market::screen`) applies to the market's bucket and returns `ELIGIBLE`,
`DEFERRED` (with reasons) or `EXCLUDED`:

| Bucket | Requirements |
|---|---|
| established | supported pool; observation no older than `RADAR_MAX_MARKET_OBSERVATION_AGE_SECS` (300 s); liquidity ≥ `RADAR_MIN_POOL_LIQUIDITY_USD` (5,000); 24 h volume ≥ `RADAR_MIN_VOLUME_24H_USD` (10,000); 1 h transactions ≥ `RADAR_MIN_TXNS_1H` (20) |
| emerging | supported pool; fresh observation; a fifth of the liquidity floor and half of the 1 h activity floor (documented policy for young markets) |
| local activity, historical, exploration | supported pool only; their purpose is to look where provider rankings do not |

- **Missing values are reasons, never zeros.** A missing figure is recorded as
  `MISSING_LIQUIDITY`, `MISSING_VOLUME_24H` or `MISSING_TXNS_1H`, never as a passing or
  failing zero.
- **No immediate resampling.** A pool sampled within six discovery intervals is deferred
  as `RECENTLY_SAMPLED`.
- **Provider figures are point-in-time.** Liquidity, volume and activity are
  provider-reported at the observation time. They are shown with that time and are never
  presented as current.

**Selection** (`market::select`) fills up to `RADAR_MAX_MARKET_SAMPLES_PER_CYCLE` slots
(5) across the five buckets by share:

| Bucket | Share (bps) |
|---|---|
| established | 3,500 |
| emerging | 2,500 |
| local activity | 2,000 |
| historical | 1,000 |
| exploration | 1,000 |

- **Unused slots roll over.** A bucket's unused slots pass to the others.
- **One pick per market.** A market is picked at most once per cycle.
- **Every decision is recorded.** Each market gets a selection state with reasons.
- **Within a bucket**, markets are ordered by a scheduling weight. The weight is not a
  market-quality score.

## Sampling

A selected pool becomes a `TRANSACTION_SAMPLING` job.

- **Size and cost.** The job requests `RADAR_INITIAL_SAMPLE_SIGNATURES` (25), or
  `RADAR_EXPANDED_SAMPLE_SIGNATURES` (100) once the pool has yielded wallets. Its
  estimated cost is 1 + N credits.
- **Execution.** The DATA ENGINE runs it as a bounded backfill. Transactions it already
  stores are reused, not refetched.
- **Duplicate prevention.** Duplicate jobs are blocked by a unique partial index on the
  open subject and work class.

## Wallet extraction and eligibility

For every verified, canonical swap the DATA ENGINE records, RADAR applies these rules:

- **Ownership must be verified.** The swap must carry `VERIFIED_OWNER` or
  `VERIFIED_AUTHORITY` attribution. **The fee payer is never a substitute.** A swap with
  no verified owner yields no wallet.
- **Quote-for-quote swaps are not memecoin activity.** SOL for USDC, for example, is
  ignored.
- **Each swap becomes one evidence row.**
  - The row is keyed (wallet, swap) and records:
    - the swap revision, finality and economic role (`BUYER`, `SELLER` or `SWAPPER`);
    - the token, quote and pool;
    - the discovery source, inherited from the job that fetched the signature.
  - Aggregator route legs are not separate evidence. The DATA ENGINE publishes one swap
    per route.
- **Corrections and retractions invalidate evidence.**
  - A corrected swap updates its evidence to the new revision. Revisions never go
    backwards.
  - A retracted, orphaned, unverified or de-attributed swap invalidates its evidence,
    with a reason and a time.
  - In every case the wallet is re-evaluated.

Eligibility (`scoring::eligibility`; the first match wins):

| Condition | Eligibility |
|---|---|
| operator excluded | `EXCLUDED_BY_POLICY` |
| a program, or a known pool account | `KNOWN_INFRASTRUCTURE` |
| a token account (for example a pool vault) | `UNSUPPORTED_ACCOUNT_ROLE` |
| no valid evidence, and every piece was de-attributed | `AMBIGUOUS_ATTRIBUTION` |
| no valid evidence otherwise | `IRRELEVANT_ACTIVITY` |
| only token-for-token evidence | `NEEDS_MORE_EVIDENCE` |
| a finalized verified swap | `ELIGIBLE_CANDIDATE` |
| only confirmed verified swaps | `PROVISIONAL_CANDIDATE` |

## Wallet registry and lifecycle

**Registry rows.** `radar_wallets` holds one row per (network, address). Each row keeps:

- the first discovery source and time, and the last discovery time;
- the lifecycle, eligibility and qualification, each with its reasons;
- the priority and its policy version;
- the history stage and its coverage;
- the monitoring mode;
- a `revision`.

**Atomic writes.** Upserts, evidence and transitions are written in one transaction.
Evaluations are guarded by the revision they read, so a concurrent change aborts the
stale write.

**Provenance.** Every source that produced evidence for a wallet is kept. A wallet found
by several sources lists them all.

Lifecycle (`radar-lifecycle-1`, `lifecycle::decide`). Every change is a row in
`radar_wallet_transitions` with trigger and reasons:

```text
DISCOVERED ─stage A queued─▶ HISTORY_QUEUED ─job running─▶ INVESTIGATING
   stage A: few swaps ─▶ INSUFFICIENT_EVIDENCE
   stage A: enough   ─▶ stage B
   stage B: negative ─▶ REJECTED     promising ─▶ PRELIMINARY_QUALIFIED ─▶ stage C
   stage C: promising ─▶ PRIORITIZED ─monitoring slot─▶ MONITORED
any non-terminal state ─inactivity established by coverage─▶ DORMANT ─▶ ARCHIVED
DORMANT / ARCHIVED / REJECTED ─new verified activity─▶ DISCOVERED (reactivated)
not investigable eligibility ─▶ REJECTED
```

**Dormancy and archival are coverage-aware.**

- **Coverage is required.** A wallet becomes `DORMANT` after `RADAR_DORMANT_AFTER_DAYS`
  (14), or `ARCHIVED` after `RADAR_ARCHIVE_AFTER_DAYS` (60), only if RADAR investigated
  its history *after* that much time had passed since its last verified trade. If not,
  RADAR schedules a bounded `DORMANT_RECHECK` instead. Silence in data RADAR never looked
  at is not evidence of inactivity.
- **Archiving keeps the record.** Evidence, transitions and snapshots stay; archival
  only removes the wallet from active work.
- **Reactivation reuses history.** New verified activity reactivates the wallet, and
  history stages already completed are reused.

## Historical investigation

History is fetched in stages, each a DATA ENGINE backfill of the wallet's signatures:

| Stage | Look-back | Signature cap | Estimate | Runs when |
|---|---|---|---|---|
| A (`INITIAL_HISTORY`) | 7 d | 50 | 51 | every eligible or provisional candidate |
| B (`EXPANDED_HISTORY`) | 30 d | 200 | 202 | stage A found at least `RADAR_MIN_QUALIFYING_SWAPS` (10) verified swaps |
| C (`EXTENDED_HISTORY`) | 90 d | 500 | 505 | stage B qualification is promising |

- **Concurrency and capacity.** At most `RADAR_MAX_CONCURRENT_HISTORY_JOBS` (2) history
  jobs run at once, and at most `RADAR_MAX_INVESTIGATING_CANDIDATES` (25) wallets are
  under investigation. Further work is deferred as `INVESTIGATION_CAPACITY`.
- **Duplicates and restarts.** Open jobs are unique per subject and class. Leases are
  recovered on start, and cancellation (`solprint radar cancel`) keeps every stored
  transaction and all evidence.
- **Coverage.** Each wallet's coverage is one of the following, and the UI shows it as
  text:
  - `NOT_INVESTIGATED`
  - `INVESTIGATION_PENDING`
  - `PARTIAL_HISTORY`
  - `BOUNDED_SAMPLE_COMPLETE`
  - `REQUESTED_SCOPE_COMPLETE`
  - `PROVIDER_HISTORY_UNAVAILABLE`
  - `DECODER_COVERAGE_LIMITED`
  - `UNRESOLVED_GAPS`
  - `UNKNOWN`
- **Never more complete than it is.** A capped stage is "requested scope complete", never
  "complete history". Retrieval counts are stored per wallet: signatures discovered,
  transactions retrieved, unavailable, verified and unsupported.
- **Knowledge timestamps.** Every observation, evidence row, transition and snapshot
  records when RADAR learned it, separately from the chain time.

## Preliminary screening

`screening::screen` runs over at most the newest 5,000 verified swaps. All arithmetic is
exact: raw `u64` amounts with checked operations, and `Decimal` for ratios.

**Activity.** For the 24 h, 7 d, 30 d, 90 d and all-time windows it counts swaps, buys,
sells, distinct tokens and trading days. Undated swaps are counted separately, not
placed in a window.

**FIFO matching** is per token and quote denomination:

- **Acquisitions open lots.** Each buy opens a lot at its quote cost, including the
  wallet's share of the fee when the wallet paid it. A fee is split across the swaps of
  one transaction and never counted twice.
- **Disposals close lots.** Each sell consumes lots first in, first out. Partial
  disposals allocate cost exactly.
- **An outcome is one closed position**, with its cost, proceeds and win flag.
- **Unknown cost stays unknown.**
  - If the wallet's balance before its first observed buy is unknown, or was not zero,
    the inventory has unknown cost. Sells against it produce no outcome and are counted
    as `unknown_cost_tokens` or `ambiguous_tokens`.
  - Unknown cost is **never treated as zero**, so pre-existing inventory cannot create a
    false profit.
- **Denominations are never mixed.** SOL, USDC and USDT outcomes are summarised
  separately. USDC and USDT cycles flag `incomplete_fees`, because the fee is paid in
  SOL.
- **No historical USD.** No historical price is fetched and no USD value is computed.
- **Return ratio** is `(proceeds − cost) / cost` per denomination, to 6 dp. The primary
  denomination is the one with the most outcomes.
- **Matched coverage** is the share of disposals that matched a known-cost lot, in bps.

These are **preliminary** figures over a bounded sample of history. They describe matched
round trips inside the investigated window and are never presented as the wallet's
profitability.

## Priority (investigation order)

`scoring::priority`, policy `radar-priority-1`, score 0–100 (4 dp, clamped):

```text
score = 100 × (0.25·A + 0.15·R + 0.25·E + 0.25·P + 0.10·D)

A  activity          min(1, verified swaps in 30 d / 20)
R  recency           2^(−days since last verified trade / RADAR_ACTIVITY_HALF_LIFE_DAYS (7)); unknown → 0
E  evidence quality  mean of 5 parts: retrieved/discovered, verified/retrieved,
                     valid/total evidence, finalized/valid, matched coverage
P  performance       0.5 + n/(n+10) · (clamp(0.5 + ratio/2, 0, 1) − 0.5)
                     (n = outcomes in the primary denomination; no outcomes → 0.5, UNKNOWN)
D  diversity         min(1, distinct tokens / 3)
```

- **Shrinkage keeps small samples near neutral.** n/(n+10) pulls performance toward 0.5
  when there are few outcomes. One lucky round trip moves P by at most 1/11 of its range.
- **Missing performance is neutral, not good.** When there are no outcomes, P is 0.5 with
  status `UNKNOWN`. Qualification never treats a missing performance as passing.
- **Weights are policy, not a statistical model.** They have not been fitted or validated
  against outcomes, so changing them changes only the order of investigation.
- **Each score is a snapshot.** It is saved in `radar_wallet_snapshots` with its
  components, input hash, policy version and time. History is point-in-time: a
  re-evaluation adds a snapshot and never rewrites one.

## Qualification (separate from priority)

`scoring::qualify` first checks activity:

- at least `RADAR_MIN_QUALIFYING_SWAPS` (10) verified swaps and
  `RADAR_MIN_DISTINCT_TRADING_DAYS` (3) trading days in the last 30 d;
- a last trade within `RADAR_QUALIFICATION_MAX_INACTIVE_DAYS` (7) days.

If any of these fail, the wallet is `INSUFFICIENT_ACTIVITY`. Then:

| Condition | Qualification |
|---|---|
| stage B not complete | `INSUFFICIENT_HISTORY` |
| fewer than `RADAR_MIN_MATCHED_OUTCOMES` (3) outcomes, or matched coverage below `RADAR_MIN_MATCHED_COVERAGE_BPS` (6,000) | `RECENT_ACTIVITY_QUALIFIED` with reason `INSUFFICIENT_FINANCIAL_EVIDENCE`; a missing figure never passes |
| primary return ratio > 0 | `PRELIMINARY_POSITIVE_RETURN` |
| primary return ratio ≤ 0 | `PRELIMINARY_NEGATIVE_RETURN` |
| any of the promising results above, with stage C complete | `READY_FOR_DEEPER_ANALYSIS` (keeps its basis as a reason) |

Qualification decides whether the investigation continues. Priority only orders work.
Both are stored with reasons; neither is a trading signal.

## Scheduler and budget

The engine runs four loops. Each is bounded and restart-safe, because all of its state
is in PostgreSQL:

| Loop | Interval | Work |
|---|---|---|
| discovery cycle | `RADAR_MARKET_DISCOVERY_INTERVAL_SECS` (300 s); also on operator request | listings, pool lookup and validation, classification, screening, selection, sample jobs |
| job dispatcher | continuous | claims due jobs under the concurrency caps, starts DATA ENGINE backfills, tracks progress, completes or defers |
| event consumer | continuous | DATA ENGINE outbox to evidence; marks wallets for evaluation |
| evaluation and maintenance | `RADAR_CANDIDATE_REEVALUATION_INTERVAL_SECS` (900 s) and `RADAR_REGISTRY_MAINTENANCE_INTERVAL_SECS` (3,600 s) | eligibility, screening, priority, qualification, lifecycle, history requests, monitoring rotation, dormancy, retention |

**Budget admission** (`budget::admission`) reads GATEWAY's local budget state. RADAR never
creates credits.

| GATEWAY state | RADAR mode |
|---|---|
| `NORMAL` | `FULL` |
| `CONSERVATION` | `CONSERVATION`: only stage A history and local-activity samples; every other job is deferred as `BUDGET_CONSERVATION` |
| `CRITICAL`, `NORMAL_BUDGET_EXHAUSTED`, `HARD_LIMIT_REACHED`, unknown usage, ledger not loaded | `PAUSED`: no provider work |

- **Allowance.** RADAR's allowance is `RADAR_NORMAL_BUDGET_SHARE_BPS` (40 %) of the
  *currently remaining* normal allowance. GATEWAY's reserve is never part of it.
- **The share bounds in-flight work.** A job is admitted only if its estimate fits in the
  allowance minus the estimates of work already outstanding.
- **Cumulative spend.** It is bounded by GATEWAY's budget states, not by the share (see
  the capacity model).
- **GATEWAY still decides each request.** It admits or denies every individual request.

**Deferral.** Deferred jobs keep their reason and are retried later: after 300 s for
budget reasons, or 3,600 s when discovery is off or DATA ENGINE backfills are disabled.

**Discovery off** (`RADAR_AUTO_DISCOVERY_ENABLED=false`, the default) means:

- no listing requests;
- no autonomous sampling or history requests;
- event consumption, evaluation and the API keep working on local data.

Operator-requested investigations (`solprint radar investigate`) still run, under the
same budget admission.

## Monitoring

Monitoring takes up to `RADAR_MAX_LIVE_MONITORED_WALLETS` (10) of the highest-priority
`PRIORITIZED` wallets:

- **Hysteresis.** A challenger replaces the weakest incumbent only if its priority is at
  least 5 points higher and the incumbent has held its slot for
  `RADAR_MONITORING_MIN_DURATION_SECS`.
- **Cooldown.** A released wallet waits `RADAR_MONITORING_COOLDOWN_SECS` before it can
  return.
- **Live streams are not used in this phase.** Live subscriptions would need GATEWAY
  streams; without them, the mode is `PERIODIC_HISTORY_REFRESH`. That is one bounded page
  (26 credits at most) since the newest covered signature, per reevaluation interval.
- **Reduced activity.** A wallet with no verified trade within
  `RADAR_REDUCE_MONITORING_AFTER_DAYS` (7) is released as `REDUCED_ACTIVITY` regardless
  of tenure, and is not started again until it trades. Inactive wallets therefore leave
  the expensive workload before they become dormant.

## Source feedback

`radar_source_yield` attributes each wallet to the source that first led to its evidence.
For each source it counts:

- unique wallets;
- eligible, investigated and qualified wallets;
- evidence rows.

Source runs over the window add runs, failures, items, new markets and provider requests.
This feedback is reported, not acted on automatically. Bucket shares stay as configured.

## Events

Events go to `radar_outbox` in the same transaction as the registry change they describe.
The event types are:

- `WalletCandidateDiscovered`
- `WalletLifecycleChanged`
- `WalletQualificationChanged`
- `WalletPriorityChanged`
- `WalletMonitoringChanged`
- `WalletReactivated`

Event properties:

- **Unique per change.** `(wallet, revision, type)` is unique, so a retried evaluation
  never emits twice.
- **Safe payloads.** Payloads carry identifiers, states and reasons. They never contain
  provider responses or secrets.
- **No consumer in this phase.** Later phases will read them; `GET /api/v1/radar/events`
  shows them.

## Operator controls (CLI)

`solprint radar <command>` writes to the database only. Every change is recorded in
`radar_operator_actions`.

| Command | Effect |
|---|---|
| `status` | totals and control state (JSON) |
| `pause [--reason CODE]` / `resume` | stop or allow provider work; local work continues |
| `cycle` | request a discovery cycle (the running service starts it within ~15 s, budget-admitted) |
| `investigate ADDRESS` | register a wallet as an operator source and evaluate it; history is still budget-admitted |
| `reevaluate ADDRESS` | mark for evaluation |
| `exclude ADDRESS` / `include ADDRESS` | operator exclusion (eligibility `EXCLUDED_BY_POLICY`) |
| `reactivate ADDRESS` | reopen a dormant, archived or rejected wallet |
| `cancel JOB_ID` | cancel an open job; stored evidence is kept |
| `wallets [--lifecycle CODE] [--limit N]`, `wallet ADDRESS`, `markets [--selection CODE]`, `jobs [STATE]` | read the registry |

## API

Read-only, local state only. No endpoint triggers provider traffic. Errors use the
standard envelope; a database outage returns `503`.

| Endpoint | Content |
|---|---|
| `GET /api/v1/radar/status` | discovery, process, budget admission, registry, market and job totals, settings |
| `GET /api/v1/radar/metrics`, `/budget` | process counters; budget share and admission |
| `GET /api/v1/radar/wallets` | registry; filters `lifecycle`, `eligibility`, `source`, `monitoring_mode`, `coverage_state`, `qualification`, `min_priority`, `max_priority`, `active_since`; `sort=PRIORITY\|LAST_TRADE\|DISCOVERED`; keyset `cursor`; `limit` ≤ 200 |
| `GET /api/v1/radar/wallets/{address}` | detail, latest metrics snapshot, retrieval, monitoring, recent jobs |
| `GET /api/v1/radar/wallets/{address}/evidence`, `/history`, `/snapshots` | evidence, lifecycle transitions, scoring snapshots |
| `GET /api/v1/radar/markets`, `/sources`, `/jobs`, `/monitoring`, `/cycles`, `/events` | market universe and pools, source yield, work queue, monitoring, cycles, outbox |

The `/radar` workspace renders these endpoints (see
[UI_DESIGN_SYSTEM.md](../design/UI_DESIGN_SYSTEM.md#radar-workspace-radar-phase-03)).

## Failure recovery

- **Restart.** All state is durable. Every job tick releases expired leases. Jobs that
  wait on a DATA ENGINE backfill resume by reading that backfill's progress, so nothing
  is requested twice. The DB test
  `restart_resumes_from_durable_state_without_duplicate_jobs` covers this.
- **Event redelivery is idempotent.** Events are deduplicated by event id and by
  (wallet, swap) evidence key with the revision check.
- **Provider outages.** They are recorded per source run and per job (`FAILED` with a
  result code, or deferred). Local sources continue.
- **Database outage.** The API returns `503`. The engine retries on its next tick.

## Retention

Evidence, transitions and snapshots are kept; they are the audit trail. The following
are pruned by maintenance:

| Data | Kept for |
|---|---|
| market observations | `RADAR_MARKET_OBSERVATION_RETENTION_DAYS` (30) |
| finished jobs | `RADAR_JOB_RETENTION_DAYS` (30) |
| superseded snapshots | `RADAR_SNAPSHOT_RETENTION_DAYS` (90); the latest snapshot per wallet is always kept |
| outbox events | `RADAR_JOB_RETENTION_DAYS`, and only once every registered consumer has acknowledged them |
| cycles | `RADAR_MARKET_OBSERVATION_RETENTION_DAYS`, and only once no observation or job refers to them |

Finished jobs that produced evidence are kept as that evidence's provenance.
