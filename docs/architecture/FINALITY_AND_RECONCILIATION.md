# Finality, reconciliation, intake and backfill

How the DATA ENGINE learns about transactions (live streams, manual intake, bounded
history backfills), how it tracks their finality, and how it corrects what it stored.
This document also covers the topics listed for `HISTORICAL_BACKFILL.md`. Code:
`backend/crates/solprint-data-engine/src/engine.rs` and
`backend/crates/solprint-storage/src/data_engine.rs`.

## Finality is separate from execution

`execution_status` (`SUCCESS` / `FAILED`) is what the transaction did. `finality` is how
settled the record is:

| Finality | Meaning |
|---|---|
| `OBSERVED_PROCESSED` | Seen only at processed commitment (not used for retrieval; `getTransaction` rejects processed) |
| `OBSERVED_CONFIRMED` | Fetched at confirmed commitment |
| `FINALIZED` | Fetched at finalized commitment, or reported finalized by `getSignatureStatuses` at the stored slot |
| `FINALITY_UNKNOWN` | The status cache no longer knows the signature (`null`) |
| `POSSIBLY_ORPHANED` | A status reported a different slot than the stored evidence |
| `ORPHANED` | Not inferred automatically in Phase 02 (see below) |

Finality never regresses. Ranks: processed 1; confirmed, unknown and possibly-orphaned 2;
finalized and orphaned 3. Late or duplicated evidence cannot move a finalized record back
(`de_apply_finality` and the persist path both enforce this).

## Reconciler

Every `DATA_ENGINE_FINALITY_RECONCILE_INTERVAL_SECS` (default 15 s), the reconciler takes
up to `DATA_ENGINE_FINALITY_BATCH_SIZE` (default 64) non-final transactions whose check
is due. It asks GATEWAY for `getSignatureStatuses` (one request, at most 256 signatures,
P4 priority, without `searchTransactionHistory`) and applies one transition per
transaction in a single database transaction, together with its outbox event and issue:

| Status answer | Transition | Follow-up |
|---|---|---|
| same slot, `finalized` | → `FINALIZED` (`TransactionFinalized`) | no further checks |
| same slot, `confirmed`/`processed` | stays `OBSERVED_CONFIRMED` | next check later |
| `null` | → `FINALITY_UNKNOWN` (`FinalityUnknown`, issue `FINALITY_UNKNOWN`) | a **finalized refetch** is queued |
| different slot | → `POSSIBLY_ORPHANED` (`FinalityUnknown`, issue `SLOT_MISMATCH`) | a **finalized refetch** is queued |

Checks back off exponentially (interval × 2ⁿ, n ≤ 6) and stop after 20 checks. A
transaction that is still unresolved then stays visibly non-final.

**A missing status is never treated as orphaning.** The status cache only covers recent
slots, so `null` means "unknown here", not "dropped". The finalized refetch
(`getTransaction` at finalized commitment, cache bypassed) decides. If it returns the
transaction, it is persisted as `FINALIZED`. If it never does, the job ends `UNAVAILABLE`
with `TRANSACTION_UNAVAILABLE`, and the transaction keeps its non-final state. Phase 02
never marks a transaction `ORPHANED` automatically. The storage transition exists, and
would retract the transaction's swaps and activities and emit `TransactionOrphaned`, but
no automatic rule reaches it without definitive evidence. Swaps of a possibly-orphaned
transaction stay canonical and carry its finality.

## Corrections and retractions

A transaction is rewritten only through the persist path, guarded by its revision
(optimistic concurrency, up to three retries on conflict):

- **The same evidence hash and decoder version** rewrite nothing. Only finality may advance
  (`Unchanged`).
- **New evidence** (for example the finalized refetch) or **a new decoder version** writes
  revision n+1. The new evidence becomes canonical and the old one is kept with
  `superseded_reason`. Swaps whose ids vanished are retracted (`SwapRetracted`), and
  surviving swaps are updated in place under the same id.

## Intake

All intake goes into one durable queue (`de_jobs`). Every observation is recorded in
`de_observations` with its provenance: source, target, commitment, slot, stream
generation and observed time. The same signature from several sources becomes **one job
with several observations**. An exact repeat (same dedup key) is recorded once.

| Source | How |
|---|---|
| `MANUAL` | `solprint data-engine ingest <signature>` |
| `LIVE_STREAM` | GATEWAY `logsSubscribe` (`mentions` = target wallet), only with `DATA_ENGINE_AUTO_INGEST_ENABLED=true` and GATEWAY streams enabled; at most `DATA_ENGINE_MAX_ACTIVE_TARGETS` enabled wallet targets |
| `STREAM_RECOVERY` | signatures GATEWAY recovered after a reconnect ([STREAM_RECOVERY.md](STREAM_RECOVERY.md)) |
| `BACKFILL` | bounded history pages (below) |
| `FINALITY`, `REPROCESS` | internal re-queues |

Stream continuity is never assumed. A GATEWAY `GAP` or `CONTINUITY_UNKNOWN` event is
stored as a `STREAM_GAP` / `CONTINUITY_UNKNOWN` quality issue on the target, and stays
open and visible. Live intake does not fill gaps silently. Filling one is an explicit,
bounded backfill.

Out-of-order observations are safe. A job's required commitment only strengthens
(`confirmed` → `finalized`), and its priority only rises. An observation that arrives
after the transaction was stored adds provenance without new retrieval work.

## Historical backfill

A backfill walks one address's signature history backwards with
`getSignaturesForAddress` and queues each signature as a `BACKFILL` observation. It is
always bounded:

| Bound | Default (config key) |
|---|---|
| page size | 100 (`DATA_ENGINE_BACKFILL_PAGE_SIZE`, 1–1000) |
| pages per backfill | 10 (`DATA_ENGINE_BACKFILL_MAX_PAGES`) |
| signatures per backfill | 1,000 (`DATA_ENGINE_BACKFILL_MAX_SIGNATURES`) |
| concurrent backfills | 1 (`DATA_ENGINE_MAX_CONCURRENT_BACKFILLS`, max 2) |
| optional stops | `until` signature, `min_slot` |

Backfills are created only by the owner (`solprint data-engine backfill create`, which
prints the estimated credits first) and run only when `DATA_ENGINE_BACKFILL_ENABLED=true`.

**Checkpointing.** Each page's observations, the next `before` cursor, the slot range seen
and the credits spent are written in **one database transaction**. A restart resumes from
the stored cursor. Duplicates of signatures that are already known are counted, not
queued again.

**Pausing.** A backfill pauses with a stored reason and keeps its cursor:

- `PAUSED_BY_BUDGET` when GATEWAY budget admission denies P5;
- `PAUSED_BY_QUEUE` when the job queue reaches `DATA_ENGINE_QUEUE_CAPACITY`;
- `PAUSED_BY_PROVIDER` on a transient provider failure;
- `PAUSED` on shutdown.

Cancellation is a request (`backfill cancel`) honoured between pages. A permanent error
sets `FAILED` with its error code.

**Coverage is stated, never assumed.**

| Coverage | Meaning |
|---|---|
| `ADDRESS_HISTORY_EXHAUSTED` | the provider returned the end of the address's history |
| `BOUNDARY_REACHED` | stopped at `min_slot` |
| `PARTIAL_HISTORY` | stopped by a page or signature limit (also raises `BACKFILL_INCOMPLETE`) |
| `NONE` | nothing fetched yet |

Coverage is copied to the target. RPC history is only as complete as the node's ledger.
A backfill proves what it walked, not that nothing else exists.

## Jobs and restart

Workers lease jobs (`FOR UPDATE SKIP LOCKED`, lease `DATA_ENGINE_JOB_LEASE_SECONDS`). A
lease that expires, for example after a crash, returns its job to the queue, and shutdown
releases this process's leases. Outcomes are listed in
[DATA_ENGINE.md](DATA_ENGINE.md#jobs).
