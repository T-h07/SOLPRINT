# Data conventions

These conventions are implemented in `solprint-core` and `frontend/src/lib/format` and
apply to every future module.

## Amounts — never floating point

| Type | Representation | Wire format |
|---|---|---|
| `Lamports` | `u64`, checked arithmetic (overflow is an error, never a wrap) | decimal **string**, canonical (`"0"`, no leading zeros, no sign) |
| `TokenAmount` | raw `u64` + `decimals` (≤ 28) | `{ "raw": "<string>", "decimals": n }` |
| `BasisPoints` | `u16`, 0..=10 000 | integer |
| `Price` | `rust_decimal::Decimal`, non-negative | decimal **string** |

* u64 values are strings on the wire because JSON numbers lose precision above 2^53.
* `TokenAmount::from_ui_str` rejects input with more fractional digits than the mint's
  decimals instead of rounding silently.
* `BasisPoints::apply_to_lamports` computes in `u128` with an explicit `Rounding::Down`
  or `Rounding::Up`; the caller must choose (fees round against the user, never in their
  favour by accident).
* The frontend formats base-unit strings with `BigInt` (`formatBaseUnits`,
  `formatLamportsAsSol`); amounts are never converted to a JS `number`.

## Identifiers

* `WalletAddress`, `TokenMint`: base58, exactly 32 decoded bytes.
* `TransactionSignature`: base58, exactly 64 decoded bytes.
* `EventId`, `CorrelationId`: UUIDv7 (time-ordered).
* Request ids: client-supplied `x-request-id` accepted only if `[A-Za-z0-9_-]{8,64}`,
  otherwise a UUIDv7 is generated.

## Time

* All timestamps are UTC, RFC 3339, millisecond precision (truncated), `Z` suffix
  (`2026-11-26T12:00:00.000Z`), via `solprint_core::UtcTimestamp` (built on the `time`
  crate; `chrono` is not used because it is flagged unmaintained, RUSTSEC-2026-0543).
* Events carry three timestamps: `source_at` (when the source says it happened),
  `observed_at` (when SOLPRINT received it), `processed_at` (when it was processed).
  They are never collapsed into one.
* Latency is measured with a monotonic clock, reported in microseconds (`latency_us`).
* The UI shows absolute times in UTC with an explicit `UTC` suffix, and ages relative to a
  clock it controls (`formatAge(then, now)`).

## Freshness, quality and reasons

* `DataFreshness` classifies data as fresh / stale / expired against explicit thresholds;
  stale data is shown as stale, never as current.
* `DataQuality` and `DataSource` travel with every `EventEnvelope<T>` together with a
  `SchemaVersion`.
* `ReasonCode` is `UPPER_SNAKE_CASE`, 1–64 characters, and explains every non-obvious
  decision or rejection (e.g. `LIVE_TRADING_NOT_PERMITTED`).

## Provider data (GATEWAY)

Every value GATEWAY returns is a `GatewayResponse<T>` carrying `provider`, `provenance`
(`DIRECT`, `FRESH_CACHE`, `STALE_CACHE`, `FALLBACK`, `RECOVERY`, `FIXTURE`), `freshness`
(`FRESH`, `STALE`, `EXPIRED`, `UNAVAILABLE`, `UNKNOWN_SOURCE_AGE`), `observed_at`,
`source_timestamp` (only when the provider supplies one), `context_slot`, `age` and
`credits_charged`. Details: [GATEWAY.md](GATEWAY.md).

* Provider numbers are parsed from text, never via floats: lamports and raw token
  amounts as `u64`, prices as `Decimal` keeping every digit sent. Float fields such as
  `uiAmount` are ignored.
* RPC responses keep their commitment and context slot. `processed`/`confirmed` data can
  be rolled back; only `finalized` is final.
* Stream events carry slot, commitment, connection generation and provenance; continuity
  is reported explicitly (`CONTINUOUS`, `RECOVERED`, `GAP`, `UNKNOWN`), never assumed.
* Monitoring counters that are `u64` are sent as JSON numbers (`#[ts(type = "number")]`):
  they are operational counts far below 2^53, unlike amounts, which stay strings.

## Chain data (DATA ENGINE)

Details: [DATA_ENGINE.md](DATA_ENGINE.md),
[TRANSACTION_RECONSTRUCTION.md](TRANSACTION_RECONSTRUCTION.md),
[SWAP_NORMALIZATION.md](SWAP_NORMALIZATION.md).

* **Raw amounts stay exact.** Swap, leg and fee amounts are raw `u64` (`NUMERIC(20,0)`
  in PostgreSQL, strings on the wire). UI amounts are derived only when decimals are
  known from the transaction's own evidence; otherwise they are null, never guessed.
* **Balance deltas are signed `i128`** (`post − pre` of two `u64` cannot overflow) and are
  stored as `NUMERIC(21,0)`. They are sent as signed decimal strings (`"-5000"`).
* **Exchange ratios** are exact `Decimal` division rounded **toward zero** to 18 decimal
  places (`NUMERIC(48,18)`), and null when either side is zero or missing.
* **Stable ids.** Records are keyed by evidence, not by insertion order:
  `swap:<signature>:<instruction_ref>`. An instruction reference is `<outer>` for a
  top-level instruction and `<outer>.<inner>` for an inner one (zero-based
  indices). Reprocessing rewrites the same ids under a new revision and never creates
  duplicates.
* **Revisions and evidence hashes.** Each stored transaction has a `current_revision` and
  the SHA-256 of the canonical evidence it was decoded from. Every outbox event carries
  `aggregate_revision`, so consumers can ignore stale or duplicate deliveries.
* **Finality is separate from execution status** (`SUCCESS` / `FAILED`) and never
  regresses ([FINALITY_AND_RECONCILIATION.md](FINALITY_AND_RECONCILIATION.md)).
* **Block time is the source time.** `block_time` is the chain's own (second-precision)
  time and may be null. `observed_at` is when SOLPRINT received the evidence. They are
  never substituted for each other.

## Honesty rules

* A value that was not measured is `null` and rendered as `—`, never `0`.
* Provider health is `UNKNOWN` (shown as "Not measured") with `last_checked_at: null`
  until GATEWAY has sent real traffic to it; there are no background probes. Historical
  diagnostics (e.g. earlier manual latency checks) are never reported as current.
* Cached provider data keeps its original `observed_at` and is labelled `FRESH_CACHE` or
  `STALE_CACHE`; it is never presented as newly observed.
* Local credit accounting is labelled as SOLPRINT's own estimate; the provider's
  dashboard remains the authority for billing.
* An unverified swap is not a trade, and an aggregator's route leg is not a separate
  trade. Only `VERIFIED` swaps with a verified wallet count as wallet trades.
* USD values are not computed in Phase 02. Swaps report `NOT_ENRICHED` (or
  `PRICE_UNAVAILABLE`), never an estimated or current price applied to past swaps.
* Fixtures and demo data are visibly labelled `FIXTURE` / `DEMO DATA` and never appear in
  production bundles.

## Number display

* Tabular numerals (`.num`) for every financial and operational number.
* Fixed precision per column so widths do not jump while values update.
* Negative values use U+2212 (−); signed deltas carry an explicit `+`.
