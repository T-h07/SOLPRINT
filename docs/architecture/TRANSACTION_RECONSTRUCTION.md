# Transaction reconstruction

How the DATA ENGINE turns one stored `getTransaction` result into an exact, ordered model
of what executed. This step is pure and deterministic: the same evidence and decoder
version always give the same records, with the same ids. Code:
`backend/crates/solprint-data-engine/src/{evidence,reconstruct,balances,movements}.rs`.

## Evidence

The input is the raw JSON of `getTransaction` with `encoding: "json"` and
`maxSupportedTransactionVersion: 0`, fetched through GATEWAY and stored unchanged as
evidence (hashed with SHA-256; see [DATA_ENGINE.md](DATA_ENGINE.md#evidence)).

Parsing (`evidence.rs`) is bounded and infers nothing. These limits reject the evidence
instead of growing memory:

| Limit | Value |
|---|---|
| accounts | 256 |
| instructions (top level + inner) | 4,096 |
| instruction data | 10,240 bytes |
| log lines | 2,048 |
| raw evidence | `DATA_ENGINE_EVIDENCE_MAX_BYTES` (default 1 MiB) |

Versions other than `legacy` and `0` raise `UNSUPPORTED_TX_VERSION`, and the job ends as
`UNSUPPORTED`. Malformed evidence is quarantined with `EVIDENCE_PARSE_FAILED`. Oversized
evidence is quarantined with `EVIDENCE_TOO_LARGE`. Neither stops the worker (see
[DATA_ENGINE.md](DATA_ENGINE.md#jobs)).

## Account list

The full account list follows the runtime order:

1. static `accountKeys`;
2. `meta.loadedAddresses.writable`;
3. `meta.loadedAddresses.readonly`.

Signer and writable flags come from the message header (`numRequiredSignatures`,
`numReadonlySignedAccounts`, `numReadonlyUnsignedAccounts`) for static keys. Loaded
writable addresses are writable and never signers.

The following are rejected, never guessed:

- A version-0 message with lookup tables whose `loadedAddresses` is missing:
  `LOADED_ADDRESSES_MISSING`.
- Any instruction account or program index outside the list: `ACCOUNT_INDEX_OUT_OF_RANGE`.
- An inconsistent header: the evidence fails as `EVIDENCE_PARSE_FAILED`.

## Instruction tree

Top-level instructions are numbered `0..n`. Inner instructions from
`meta.innerInstructions` are attached to their top-level instruction and numbered in
execution order (`"3.0"`, `"3.1"`, …). Parent links come from `stackHeight`: an inner
instruction's parent is the nearest earlier instruction one level shallower. Every
instruction keeps its program, accounts and data, so decoders see the full CPI context.

Degraded evidence is decoded with what is known and flagged:

| Condition | Issue | Effect |
|---|---|---|
| `innerInstructions` absent (node did not record them) | `INNER_INSTRUCTIONS_MISSING` (warning) | swap amounts are missing; status `INSUFFICIENT_EVIDENCE` |
| `stackHeight` absent (old metadata) | `STACK_HEIGHT_MISSING` (info) | inner instructions are treated as direct children of their top-level instruction |
| token balances absent | `TOKEN_BALANCES_MISSING` (warning) | token deltas unknown |
| `preBalances` / `postBalances` length differs from the account list | `BALANCE_LENGTH_MISMATCH` (invalid) | native deltas not trusted |

## Native SOL balances

`preBalances` and `postBalances` give one exact delta per account, in lamports. Deltas are
`i128`, so a `u64` minus a `u64` cannot overflow, and they are stored as `NUMERIC` (see
[DATA_CONVENTIONS.md](DATA_CONVENTIONS.md)).

The **network fee** is `meta.fee`, charged to the fee payer whether or not execution
succeeded. It is recorded as a `FEE` activity and kept apart from swap amounts.
**Rent** moves through account creation (System `CreateAccount` /
`CreateAccountWithSeed`, ATA creation) and comes back on closure (`CloseAccount`). These
are `ACCOUNT_CREATION` / `ACCOUNT_CLOSURE` activities and are never counted as trade
value.

## Token balances

`preTokenBalances` / `postTokenBalances` give the owner, mint, decimals and exact raw
`amount` string per token account. An account that is missing on one side was created or
closed in this transaction. Its missing side is zero, and the evidence says how that zero
is known:

| Evidence | Meaning |
|---|---|
| `REPORTED` | present in the metadata |
| `INFERRED_CREATED` | absent before: the account was created in this transaction |
| `INFERRED_CLOSED` | absent after: the account was closed in this transaction |

Mint decimals come only from the transaction's own metadata. When they are unknown, raw
amounts are kept, UI amounts and ratios are not computed, and the transaction gets
`UNKNOWN_MINT_DECIMALS`.

## Movements

System, SPL Token and Token-2022 instructions are decoded wherever they appear, at top
level or in a CPI. Token accounts are resolved to their mint and owner from the
transaction's own evidence: token balances, `initializeAccount*` and ATA creation. The
supported movements are transfer, `transferChecked`, mint, burn, close, `syncNative`,
Token-2022 `TransferCheckedWithFee`, and the System transfer and create variants.

### WSOL

Wrapping is a System transfer into a WSOL token account followed by `syncNative`.
Unwrapping is `CloseAccount` of a WSOL account. These are recorded as `WRAP_SOL` /
`UNWRAP_SOL`. A swap side in a WSOL account that the same transaction closes, or that
has no token-balance entry (a native lamport side), is reported as native SOL (asset kind
`NATIVE_SOL`). The lamports are counted once, on the swap, and never again as a separate
transfer.

### Token-2022

`TransferCheckedWithFee` carries the withheld fee. The sent amount, the fee (a
`TRANSFER_FEE` row) and the net amount received are all kept. Extensions that hide or
reroute amounts are flagged with `UNSUPPORTED_TOKEN_EXTENSION` and not decoded (see
[DECODER_CAPABILITIES.md](DECODER_CAPABILITIES.md#shared-rules-all-decoders)).

## Execution status

A failed transaction (`meta.err` not null) keeps its evidence, fee and balance changes.
It is classified `FAILED_TRANSACTION` + `FEE`, and no swap is produced from it. Execution
status (`SUCCESS` / `FAILED`) and finality are separate fields. A failed transaction can
be finalized.
