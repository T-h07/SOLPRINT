# DATA ENGINE (Phase 02)

The DATA ENGINE turns Solana transactions into durable, evidence-backed records:
reconstructed transactions, exact balance changes, normalized activities and user-level
swaps. Each record carries a quality status, finality and provenance. It is **read-only by
construction**: nothing in it builds, signs or submits transactions, and it holds no
wallets or keys. Every provider request goes through GATEWAY.

It is experimental software. It decodes a limited set of protocols
([DECODER_CAPABILITIES.md](DECODER_CAPABILITIES.md)) and has been verified against
synthetic fixtures and a disposable PostgreSQL. It has **not** been run against live
mainnet traffic.

## Documents

| Topic | Document |
|---|---|
| Overview, jobs, evidence, domain events, data quality, API, CLI, retention | this file |
| Supported protocols and their sources | [DECODER_CAPABILITIES.md](DECODER_CAPABILITIES.md) |
| Account list, instruction tree, balances, WSOL, fees, rent | [TRANSACTION_RECONSTRUCTION.md](TRANSACTION_RECONSTRUCTION.md) |
| User-level swaps, legs, fees, ratios, valuation | [SWAP_NORMALIZATION.md](SWAP_NORMALIZATION.md) |
| Who a swap belongs to | [WALLET_ATTRIBUTION.md](WALLET_ATTRIBUTION.md) |
| Finality, corrections, live intake, historical backfill, gaps | [FINALITY_AND_RECONCILIATION.md](FINALITY_AND_RECONCILIATION.md) |
| Throughput, storage and credit estimates | [DATA_ENGINE_CAPACITY_MODEL.md](DATA_ENGINE_CAPACITY_MODEL.md) |
| Amount, time and id conventions | [DATA_CONVENTIONS.md](DATA_CONVENTIONS.md) |

The topics listed for Phase 02 as `HISTORICAL_BACKFILL`, `DOMAIN_EVENTS` and
`DATA_QUALITY` are covered in the documents above. They were consolidated to avoid
repeating the same rules in several places.

## Module layout

```text
solprint-data-engine      (no HTTP client, no SQL)
  evidence ─ reconstruct ─ balances ─ movements      pure, deterministic
  decoders/{jupiter,pump,pumpswap,raydium,orca} ─ attribution ─ normalize ─ pipeline
  mapping      decoded transaction → storage write set + outbox events
  source       ChainSource trait; GatewaySource (production) uses GATEWAY's typed API
  engine       queue workers, finality reconciler, backfills, stream intake, outbox
solprint-storage::data_engine   the only SQL (migration 20261127000003_data_engine.sql)
solprint-api::data_routes       read-only /api/v1/data/* (wire contracts, ts-rs)
solprint-app                    composes and runs it; `solprint data-engine` CLI
```

Dependencies point inward, as described in
[MODULE_BOUNDARIES.md](MODULE_BOUNDARIES.md). The decode pipeline is a pure function of
the evidence and the decoder version. The same input always gives the same records, with
the same ids.

## Lifecycle

When `DATA_ENGINE_ENABLED=true` (the default), `solprint-app` builds the engine after
GATEWAY and starts these tasks:

- `DATA_ENGINE_DECODE_CONCURRENCY` job workers;
- the finality reconciler;
- the outbox dispatcher;
- maintenance: lease recovery, cached counts, and hourly retention;
- the backfill runner, when `DATA_ENGINE_BACKFILL_ENABLED=true`;
- live stream intake, when `DATA_ENGINE_AUTO_INGEST_ENABLED=true`.

On shutdown, the engine stops before GATEWAY, within the shutdown grace period, and
releases its job leases.

## Jobs

Every signature to retrieve is one row in `de_jobs`. Rows are leased with
`FOR UPDATE SKIP LOCKED`, so a crash can neither lose nor duplicate work. Outcomes:

| Condition | Result | Attempt consumed |
|---|---|---|
| Persisted (new revision) or unchanged | `COMPLETED` | yes |
| Budget admission denied | released, retried in 60 s | **no** |
| Provider disabled / no credential | released, retried in 5 min | **no** |
| Transient provider failure | `FETCH_RETRY`, backoff 2 s·2ⁿ (max 300 s) | yes |
| Not returned at the required commitment | retried until `DATA_ENGINE_JOB_MAX_ATTEMPTS`, then `UNAVAILABLE` + `TRANSACTION_UNAVAILABLE` | yes |
| Evidence larger than `DATA_ENGINE_EVIDENCE_MAX_BYTES` | `QUARANTINED` + `EVIDENCE_TOO_LARGE` | — |
| Unparseable or inconsistent evidence | `QUARANTINED` + `EVIDENCE_PARSE_FAILED` (or the specific reconstruction code) | — |
| Unsupported transaction version | `UNSUPPORTED` | — |
| Unexpected processing error (for example a storage failure) | handed back as `FETCH_RETRY` with backoff and `JOB_ERROR`; ends `QUARANTINED` when attempts run out | yes |

Malformed evidence never stops a worker. Above `DATA_ENGINE_QUEUE_CAPACITY` non-terminal
jobs, intake is refused with `QUEUE_SATURATED` and backfills pause.

## Evidence

The raw `getTransaction` JSON is stored unchanged in `de_evidence`. Each row records its
SHA-256, provider (`HELIUS`, `SOLANA_RPC` or `FIXTURE`), commitment, slot, fetch time and
evidence schema version. Exactly one evidence row per transaction is canonical; earlier
ones are kept with a `superseded_reason`. Evidence that was served from GATEWAY's cache is
labelled as cached in metrics and is never presented as newly observed. Raw evidence is
never sent to the browser: the API returns its metadata and hash only.

The transaction, its evidence, balance changes, activities, swaps, legs, fees, quality
issues and outbox events are written in **one database transaction**, guarded by the
transaction's revision.

## Reprocessing

`solprint data-engine reprocess` re-queues stored transactions as `REPROCESS` jobs. By
default it takes those decoded by an older decoder version; `--all` takes every
transaction. Reprocessing decodes the **stored canonical evidence** and contacts no
provider. The same evidence hash and decoder version produce no write (`Unchanged`). A
new decoder version writes a new revision, and swaps that vanish are retracted
([FINALITY_AND_RECONCILIATION.md](FINALITY_AND_RECONCILIATION.md#corrections-and-retractions)).

## Domain events (transactional outbox)

Events are inserted into `de_outbox` in the same database transaction as the change they
describe, so an event exists if and only if its change was committed.

| Event | Aggregate | When |
|---|---|---|
| `TransactionDecoded` | transaction | a new revision was written |
| `SwapRecorded` | swap | a swap was written (payload includes `verifiedTrade`) |
| `SwapRetracted` | swap | a canonical swap id disappeared in a new revision |
| `TransactionFinalized` | transaction | finality became `FINALIZED` |
| `FinalityUnknown` | transaction | finality became `FINALITY_UNKNOWN` or `POSSIBLY_ORPHANED` |
| `TransactionOrphaned` | transaction | reserved; not emitted automatically in Phase 02 |

Each event has a UUID, a schema version (1), an aggregate type, id and revision, its
source, observed and source times, a correlation id and a quality state. The unique key
`(aggregate_type, aggregate_id, aggregate_revision, event_type)` means reprocessing never
publishes a second copy. Finality events carry the transaction's current revision.

**Delivery is at least once.** Consumers must be idempotent, keyed by `event_id`.
Delivery state is per consumer (`de_outbox_deliveries`) and survives restarts. Phase 02
has one built-in consumer, `diagnostic_log`, which records delivery counts per event type
for monitoring. Future modules subscribe as additional consumers. They must not read
decoder internals.

## Data quality

Problems are recorded as quality issues (`de_quality_issues`), not just logged. Each
issue has a code, a severity, the subject it concerns (transaction, target, backfill or
stream), a detail, the decoder version and an occurrence count. Open issues with the same
subject and code are deduplicated.

| Severity | Meaning | Codes |
|---|---|---|
| `CRITICAL` | data must not be used | `TRANSACTION_ORPHANED` |
| `INVALID` | evidence is inconsistent | `EVIDENCE_PARSE_FAILED`, `EVIDENCE_TOO_LARGE`, `ACCOUNT_INDEX_OUT_OF_RANGE`, `BALANCE_LENGTH_MISMATCH`, `LOADED_ADDRESSES_MISSING`, `SLOT_MISMATCH` |
| `WARNING` | decoded with a known gap | `INNER_INSTRUCTIONS_MISSING`, `TOKEN_BALANCES_MISSING`, `BALANCE_RECONCILIATION_FAILED`, `UNSUPPORTED_TOKEN_EXTENSION`, `DECODER_AMOUNT_MISMATCH`, `DECODER_MALFORMED_DATA`, `EVENT_MISSING`, `ROUTE_INCOMPLETE`, `UNKNOWN_MINT_DECIMALS` |
| `OPERATIONAL` | pipeline or coverage condition | `FINALITY_UNKNOWN`, `TRANSACTION_UNAVAILABLE`, `STREAM_GAP`, `CONTINUITY_UNKNOWN`, `BACKFILL_INCOMPLETE`, `QUEUE_SATURATED`, `BUDGET_EXHAUSTED` |
| `INFO` | expected limitation | `STACK_HEIGHT_MISSING`, `UNSUPPORTED_INSTRUCTION`, `UNSUPPORTED_PROGRAM`, `ATTRIBUTION_AMBIGUOUS`, `PRICE_UNAVAILABLE` |

Every decoded record also carries its own status (`VERIFIED` … `INVALID`; see
[SWAP_NORMALIZATION.md](SWAP_NORMALIZATION.md#status)). A record with an open issue is
still shown, with its status, so nothing degraded is presented as clean.

## Read-only API

All routes are `GET`, read local state only, and never trigger provider traffic. When the
database is unreachable they return a 503 envelope. Invalid parameters return 400. Amounts
are decimal strings; ids and enum values are fixed-format strings. Contracts are generated
from Rust into `frontend/src/types/generated` (`cargo test -p solprint-api`).

| Route | Query parameters |
|---|---|
| `/api/v1/data/status` | none. Engine state, queue, totals, process counters, settings; `counts_available: false` with null totals when the database is down |
| `/api/v1/data/decoders` | none |
| `/api/v1/data/transactions` | `protocol`, `status`, `finality`, `cursor` (`slot:signature`), `limit` (1–200, default 50) |
| `/api/v1/data/transactions/{signature}` | none. Evidence metadata, balances, activities, swaps, issues; 404 when not stored |
| `/api/v1/data/swaps` | `wallet`, `mint`, `protocol`, `status`, `include_retracted`, `verified_only` (only canonical `VERIFIED` swaps with a verified attribution, the same rule as the verified total), `cursor`, `limit` |
| `/api/v1/data/issues` | `severity`, `signature`, `include_resolved`, `limit` |
| `/api/v1/data/targets` | none |
| `/api/v1/data/backfills` | `limit` |
| `/api/v1/data/events` | `limit` |

The frontend workspace (`/data`) is described in
[UI_DESIGN_SYSTEM.md](../design/UI_DESIGN_SYSTEM.md#data-engine-workspace-data-phase-02).

## CLI

`solprint data-engine <command>` (from `backend/`:
`cargo run --bin solprint -- data-engine …`). All commands work on local state, except
`ingest --live`, which makes one GATEWAY request.

| Command | Effect |
|---|---|
| `status` | local counts |
| `decoders` | the decoder registry (no database needed) |
| `decode-fixture [NAME]` | decodes the synthetic fixtures and checks each against its expected result; exits non-zero on a mismatch (no database needed) |
| `jobs [STATE] [--limit N]` | recent jobs |
| `issues [--severity S] [--signature SIG] [--limit N]` | open quality issues |
| `targets list \| add <ADDRESS> [--label] [--priority P2..P5] [--commitment] \| enable <ID> \| disable <ID>` | explicit ingestion targets; none are created automatically |
| `ingest <SIGNATURE> [--finalized]` | queue one signature (the running service fetches it) |
| `ingest <SIGNATURE> --live` | queue it and fetch it now through GATEWAY (1 request) |
| `backfill create <ADDRESS> [--pages N] [--max-signatures N] [--min-slot S] [--until SIG] [--before SIG] [--target ID]` | prints the credit estimate only |
| `backfill create … --yes` | creates the bounded backfill |
| `backfill list \| cancel <ID>` | inspect or cancel |
| `reprocess [--all] [--limit N]` | re-decode stored evidence (no provider) |

## Diagnostics

`./scripts/data-engine-diagnostics.ps1` is offline by default. It lists the decoders,
checks every fixture against its expected result, and, if PostgreSQL is reachable, prints
local status, recent jobs and open issues.

`-Live -Signature <SIG>` additionally fetches that one transaction through GATEWAY. This
is the owner's explicit opt-in, costs at most 1 Helius credit, and requires the database.
No live diagnostic has been run for Phase 02. Real-provider results are reported as
**NOT RUN** in [PHASE_02.md](../phases/PHASE_02.md).

## Retention

The maintenance task applies these rules hourly:

| Data | Rule | Default |
|---|---|---|
| Raw evidence JSON | of `FINALIZED`/`ORPHANED` transactions older than `DATA_ENGINE_EVIDENCE_RETENTION_DAYS`, set to null (`pruned_at`). Hash, metadata and all normalized records are kept. `0` keeps evidence indefinitely | 90 days |
| Jobs | `COMPLETED`/`CANCELLED` jobs older than `DATA_ENGINE_JOB_RETENTION_DAYS` are deleted | 14 days |
| Outbox | events older than `DATA_ENGINE_OUTBOX_RETENTION_DAYS` that **every** registered consumer has delivered are deleted | 7 days |

Normalized transactions, swaps, issues and non-final evidence are never pruned
automatically. A pruned transaction can still be reprocessed only after its evidence is
fetched again.

## Interfaces for later phases

Later modules (wallet intelligence, CORTEX, GUARD) consume:

- the read models (verified swaps by wallet, mint or protocol, with finality and status);
- the outbox events.

They do not consume provider data or decoder internals. Phase 02 implements none of those
modules.
