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

* All timestamps are UTC, RFC 3339, millisecond precision, `Z` suffix
  (`2026-11-26T12:00:00.000Z`).
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

## Honesty rules

* A value that was not measured is `null` and rendered as `—`, never `0`.
* Provider state is `NOT_INITIALIZED` with `last_checked_at: null` until GATEWAY measures
  it. Historical diagnostics (e.g. earlier manual latency checks) are never reported as
  current.
* Fixtures and demo data are visibly labelled `FIXTURE` / `DEMO DATA` and never appear in
  production bundles.

## Number display

* Tabular numerals (`.num`) for every financial and operational number.
* Fixed precision per column so widths do not jump while values update.
* Negative values use U+2212 (−); signed deltas carry an explicit `+`.
