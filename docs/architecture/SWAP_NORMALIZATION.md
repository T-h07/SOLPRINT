# Swap normalization

How a decoded protocol instruction becomes one normalized, user-level swap record. Code:
`backend/crates/solprint-data-engine/src/{pipeline,normalize}.rs` and
`src/decoders/`. Supported protocols are listed in
[DECODER_CAPABILITIES.md](DECODER_CAPABILITIES.md). Wallet attribution is described in
[WALLET_ATTRIBUTION.md](WALLET_ATTRIBUTION.md).

## What a swap is

A swap is what one user gave up and received in a **single protocol instruction**:
an input side and an output side, each with a mint, an exact raw `u64` amount, decimals
(when known), the token account involved, and an asset kind (`NATIVE_SOL`,
`WRAPPED_SOL`, `SPL_TOKEN`, `TOKEN_2022`, `UNKNOWN`).

- **Identity.** `swap_id = "swap:<signature>:<instruction_ref>"`. It is stable across
  reprocessing, so the same economic event never becomes two records.
- **Aggregators.** A Jupiter route is one swap. The DEX instructions executed inside it
  are **route legs** (`legs[]`, in execution order, each with protocol, pool, raw in/out
  and evidence `PROGRAM_EVENT` or `TOKEN_TRANSFERS`), never separate wallet trades. The
  pipeline claims the route instruction's whole subtree before running other decoders.
- **Multi-hop on one venue** (Orca `two_hop_swap`) is one swap with two legs.
- **Not swaps.** Liquidity deposits and withdrawals, pool creation, migrations, ordinary
  transfers, wraps and unwraps are other activity kinds and never appear as swaps. A
  failed transaction never yields a swap.

## Status

Each swap has a `classification_status`. The transaction's status is the weakest of its
records.

| Status | Meaning |
|---|---|
| `VERIFIED` | Both sides observed and cross-checked against balance deltas |
| `PARTIALLY_DECODED` | Decoded, but some evidence disagrees or is zero (for example `DECODER_AMOUNT_MISMATCH`, `ZERO_AMOUNT`) |
| `AMBIGUOUS` | The economic meaning is unclear (for example the same mint on both sides) |
| `UNSUPPORTED` | The program or instruction is not decoded; evidence is kept |
| `INSUFFICIENT_EVIDENCE` | Required evidence is absent (inner instructions, program event): amounts are missing, not inferred |
| `INVALID` | The evidence is inconsistent |

`reasons[]` lists machine-readable reasons, for example `SAME_INPUT_AND_OUTPUT_MINT`,
`ZERO_AMOUNT`, `DECIMALS_UNKNOWN`, `AUTHORITY_IS_DELEGATE`.

**A verified trade** is a canonical swap with status `VERIFIED` and a verified attribution
(`VERIFIED_OWNER`, `VERIFIED_AUTHORITY` or `VERIFIED_BENEFICIARY`). The `swaps_verified`
count and the `verifiedTrade` flag on `SwapRecorded` events use exactly this definition.
The `wallet` column is set **only** for verified attribution, so a wallet query never
returns an unattributed or ambiguous swap.

## Fees

Fees are explicit rows (`fees[]`), never netted silently into amounts:

| Kind | Source |
|---|---|
| `NETWORK` | `meta.fee` (on the transaction, not the swap) |
| `PROTOCOL`, `CREATOR`, `PLATFORM`, `OTHER` | program events (Pump.fun, PumpSwap, Jupiter `FeeEvent`) |
| `TRANSFER_FEE` | Token-2022 `TransferCheckedWithFee` |

Each fee row has a provenance: `EXPLICIT` (stated by the program or instruction),
`DERIVED` (computed from observed values) or `UNKNOWN`.

## Exchange ratio

`exchange_ratio` is the output received per one unit of input, in UI units:

```text
(output_raw / 10^output_decimals) / (input_raw / 10^input_decimals)
```

It is computed in exact `Decimal` arithmetic and **rounded toward zero to 18 decimal
places** (`NUMERIC(48,18)`). It is null when either amount is zero or missing, when
decimals are unknown, or when the result does not fit. No floating point is used anywhere
(see [DATA_CONVENTIONS.md](DATA_CONVENTIONS.md)).

## USD valuation

Phase 02 does **not** compute USD values. Price enrichment
(`DATA_ENGINE_PRICE_ENRICHMENT_ENABLED`) is off by default, and no price retrieval is
implemented. The API reports each swap's valuation state honestly:

- `NOT_ENRICHED` when enrichment is off;
- `PRICE_UNAVAILABLE` when it is on, because no point-in-time price exists.

Prices and values stay null. They are never estimated, never taken from a later
observation, and never zero-filled. The schema has a `de_price_observations` table, with
source, source slot, observed time and freshness, for a later phase. Any future valuation
must use the newest price observed **at or before** the swap's block time, within
`DATA_ENGINE_PRICE_MAX_AGE_SECS`, and must record which observation it used.

## Corrections

When a transaction is decoded again from new evidence (a finalized refetch) or by a new
decoder version, its swaps are rewritten under a new revision. A swap id that no longer
exists is marked non-canonical, with `retracted_at` and a `retraction_reason`, and a
`SwapRetracted` event is emitted. Retracted swaps are excluded from listings by default.
See [FINALITY_AND_RECONCILIATION.md](FINALITY_AND_RECONCILIATION.md).
