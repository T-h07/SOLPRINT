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

## Honesty rules

* A value that was not measured is `null` and rendered as `—`, never `0`.
* Provider health is `UNKNOWN` (shown as "Not measured") with `last_checked_at: null`
  until GATEWAY has sent real traffic to it; there are no background probes. Historical
  diagnostics (e.g. earlier manual latency checks) are never reported as current.
* Cached provider data keeps its original `observed_at` and is labelled `FRESH_CACHE` or
  `STALE_CACHE`; it is never presented as newly observed.
* Local credit accounting is labelled as SOLPRINT's own estimate; the provider's
  dashboard remains the authority for billing.
* Fixtures and demo data are visibly labelled `FIXTURE` / `DEMO DATA` and never appear in
  production bundles.

## Number display

* Tabular numerals (`.num`) for every financial and operational number.
* Fixed precision per column so widths do not jump while values update.
* Negative values use U+2212 (−); signed deltas carry an explicit `+`.
