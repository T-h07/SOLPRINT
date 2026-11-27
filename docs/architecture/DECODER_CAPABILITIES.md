# Decoder capabilities

What the DATA ENGINE decodes in Phase 02, what it does not, and the sources each decoder
was built from. Decoder implementation version: **`de-2026.11.1`**
(`solprint_data_engine::DECODER_VERSION`). Every stored record carries this version, so
records can be reprocessed when it changes (see [DATA_ENGINE.md](DATA_ENGINE.md#reprocessing)).

**This is not complete DEX coverage.** Seven programs are decoded. Anything else is
recorded with its evidence and an explicit `UNSUPPORTED` / `UNKNOWN_ACTIVITY` status,
never guessed.

## How to read this document

- **Verified behaviour** means the program id, instruction discriminators, account
  positions and argument layouts were read from the official source at the commit below.
  The Anchor discriminators are also recomputed by unit tests
  (`sha256("global:<name>")[..8]`, `sha256("event:<Name>")[..8]`).
- **Inferred behaviour** is marked as such. It covers how an instruction's economic effect is
  read from the movements executed inside it (for example, "the transfer out of the user's
  source account is the input"). Decoders verify these inferences against balance deltas
  and flag mismatches; they are never assumed silently.
- **Fixture coverage** names the synthetic fixtures in
  `backend/crates/solprint-data-engine/src/fixtures.rs`. They are built from the verified
  layouts; they are **not** captured mainnet transactions. No real provider response is
  committed. Decoding against real mainnet evidence is the owner's optional live
  diagnostic ([DATA_ENGINE.md](DATA_ENGINE.md#diagnostics)) and has not been performed.

Sources verified on **2026-11-27**, at these commits:

| Source repository | Commit | Commit date |
|---|---|---|
| `jup-ag/instruction-parser` | `e6f77951377847c579112e6a16d8c17c5c092485` | 2024-11-06 |
| `pump-fun/pump-public-docs` | `bc35796152027d2744ca02066d305438e34b67e7` | 2026-11-21 |
| `raydium-io/raydium-amm` | `9c54069c6bd86d52cd6622705b98c274ee33ba4a` | 2026-11-18 |
| `raydium-io/raydium-cp-swap` | `3d69122790e97cd6aff24e8ba95940793773f58c` | 2026-11-01 |
| `raydium-io/raydium-clmm` | `6d7a0f82d2f293c553c69aaf943377367c6c4cc6` | 2026-10-27 |
| `orca-so/whirlpools` | `cc683ec92acb5e7606caa3d8e514d9383e9c7c11` | 2026-10-12 |

The Jupiter parser repository is the newest public source for the v6 route layout; its
last commit predates the others. That is a known limitation: route variants added to the
program after that commit are reported as unsupported.

## Shared rules (all decoders)

- Amounts come from token/system movements executed **inside** the instruction's subtree,
  or from the program's own authentic self-CPI event (Anchor `emit_cpi!`, identified by
  the `EVENT_IX_TAG` prefix, invoked by the program itself). Program **log lines are never
  used as evidence**.
- Instructions executed inside an aggregator route are **legs** of that route, not separate
  swaps (the pipeline claims the route's subtree).
- A failed transaction never produces a swap (`FAILED_TRANSACTION` + `FEE` activities).
- Missing inner instructions give `INSUFFICIENT_EVIDENCE`. Amounts are reported missing,
  never inferred.
- Token-2022 `TransferCheckedWithFee` is decoded: the withheld fee is reported as a
  `TRANSFER_FEE` row, and the net amount is what arrived. Confidential transfers,
  confidential mint/burn, `UnwrapLamports`, permissioned burn and `Batch` raise
  `UNSUPPORTED_TOKEN_EXTENSION` and are not decoded.
- Wallet attribution follows [WALLET_ATTRIBUTION.md](WALLET_ATTRIBUTION.md).

## Jupiter Aggregator v6

| | |
|---|---|
| Program | `JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4` |
| Source | `jup-ag/instruction-parser`: `src/idl/jupiter.ts`, `src/lib/instruction-parser.ts`, `src/constants.ts` |
| Supported instructions | `route`, `route_with_token_ledger`, `shared_accounts_route`, `shared_accounts_route_with_token_ledger`, `exact_out_route`, `shared_accounts_exact_out_route` |
| Events used | `SwapEvent` (route legs), `FeeEvent` (platform fee) |
| Activity | `SWAP`, venue `AGGREGATOR`. One user-level swap per route instruction; legs are listed in order with their AMM. |
| Inferred | The user-level input is the transfer out of the user's source token account; the output is what arrived in the destination account. Both are cross-checked against balance deltas. |
| Limitations | Routes whose legs cannot be reconciled to `SwapEvent`s are kept, with `route_complete = false` and `ROUTE_INCOMPLETE`. Route instructions newer than the source commit are unsupported. Jupiter limit orders, DCA and perpetuals are separate programs and are not decoded. |
| Fixtures | `jupiter_route_two_legs` |

## Pump.fun (bonding curve)

| | |
|---|---|
| Program | `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` |
| Source | `pump-fun/pump-public-docs` `idl/pump.json` |
| Supported instructions | `buy`, `buy_exact_sol_in`, `buy_v2`, `buy_v3`, `buy_exact_quote_in_v2`, `buy_exact_quote_in_v3`, `sell`, `sell_v2`, `sell_v3`; `create`, `create_v2` (pool creation); `migrate`, `migrate_v2` (liquidity migration) |
| Events used | `TradeEvent`, `PostCompleteBuyEvent` |
| Activity | `BONDING_CURVE_TRADE` (a user-level swap, venue `BONDING_CURVE`), `POOL_CREATION`, `LIQUIDITY_MIGRATION` |
| Inferred | The token side is cross-checked against the transfer into or out of the user's token account. Amounts are the curve's gross quote amounts. Protocol and creator fees are explicit fee rows taken from the event. |
| Limitations | Trades without an authentic `TradeEvent` self-CPI are `INSUFFICIENT_EVIDENCE` (`EVENT_MISSING`). Admin and configuration instructions are unsupported. |
| Fixtures | `pump_fun_buy` |

## PumpSwap (pump AMM)

| | |
|---|---|
| Program | `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA` |
| Source | `pump-fun/pump-public-docs` `idl/pump_amm.json` |
| Supported instructions | `buy`, `buy_v2`, `buy_exact_quote_in`, `buy_exact_quote_in_v2`, `sell`, `sell_v2`; `create_pool`, `deposit`, `withdraw` |
| Events used | `BuyEvent`, `SellEvent`: `user_quote_amount_in` / `user_quote_amount_out` include fees and are used directly |
| Activity | `SWAP` (venue `CONSTANT_PRODUCT_AMM`), `POOL_CREATION`, `LIQUIDITY_DEPOSIT`, `LIQUIDITY_WITHDRAWAL` |
| Inferred | Event amounts are cross-checked against the user's token transfers. LP, protocol and creator fees are explicit rows. |
| Limitations | Without the event, amounts are not inferred (`EVENT_MISSING`). Admin instructions are unsupported. |
| Fixtures | `pumpswap_sell` |

## Raydium AMM v4

| | |
|---|---|
| Program | `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8` |
| Source | `raydium-io/raydium-amm` `program/src/instruction.rs`, `program/src/lib.rs` |
| Supported instructions | one-byte tags `SwapBaseIn` (9), `SwapBaseOut` (11), `SwapBaseInV2` (16), `SwapBaseOutV2` (17); `Initialize2` (1), `Deposit` (3), `Withdraw` (4) |
| Activity | `SWAP` (venue `CONSTANT_PRODUCT_AMM`), `POOL_CREATION`, `LIQUIDITY_DEPOSIT`, `LIQUIDITY_WITHDRAWAL` |
| Inferred | The user's source and destination token accounts and the user authority are read by account position. Swaps whose account count does not match a known layout are `UNSUPPORTED` ("unexpected layout"), never forced. |
| Limitations | No AMM v4 event exists, so amounts come from the two transfers inside the instruction. Other instructions (admin, OpenBook market operations) are unsupported. |
| Fixtures | `raydium_amm_v4_swap` |

## Raydium CPMM

| | |
|---|---|
| Program | `CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C` |
| Source | `raydium-io/raydium-cp-swap` `programs/cp-swap/src/lib.rs`, `instructions/swap_base_input.rs` |
| Supported instructions | `swap_base_input`, `swap_base_output`; `initialize`, `deposit`, `withdraw` |
| Activity | `SWAP` (venue `CONSTANT_PRODUCT_AMM`), `POOL_CREATION`, `LIQUIDITY_DEPOSIT`, `LIQUIDITY_WITHDRAWAL` |
| Limitations | Amounts come from transfers inside the instruction. A disagreement with balance deltas gives `PARTIALLY_DECODED` with `DECODER_AMOUNT_MISMATCH`. |
| Fixtures | `raydium_cpmm_swap`, `relayed_swap_fee_payer_differs`, `program_controlled_swap`, `missing_inner_instructions`, `amount_mismatch` |

## Raydium CLMM

| | |
|---|---|
| Program | `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK` |
| Source | `raydium-io/raydium-clmm` `programs/amm/src/instructions/swap.rs`, `swap_v2.rs` |
| Supported instructions | `swap`, `swap_v2`; `create_pool`, `open_position`, `open_position_v2`, `open_position_with_token22_nft`, `increase_liquidity(_v2)`, `decrease_liquidity(_v2)` |
| Activity | `SWAP` (venue `CONCENTRATED_LIQUIDITY`), `POOL_CREATION`, `LIQUIDITY_DEPOSIT`, `LIQUIDITY_WITHDRAWAL` |
| Unsupported | `swap_router_base_in` (multi-pool router) is explicitly reported as unsupported |
| Fixtures | `raydium_clmm_swap_v2_alt` (vaults loaded from an address lookup table) |

## Orca Whirlpool

| | |
|---|---|
| Program | `whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc` |
| Source | `orca-so/whirlpools` `programs/whirlpool/src/instructions/` (`swap.rs`, `v2/swap.rs`, `two_hop_swap.rs`, `v2/two_hop_swap.rs`) |
| Supported instructions | `swap`, `swap_v2`, `two_hop_swap`, `two_hop_swap_v2`; `initialize_pool(_v2)`, `increase_liquidity(_v2)`, `decrease_liquidity(_v2)` |
| Activity | `SWAP` (venue `CONCENTRATED_LIQUIDITY`); a two-hop swap is one user swap with two legs |
| Verified layout | `a_to_b` is byte 41 of swap data (after the 8-byte discriminator); two-hop flags are bytes 24–26 |
| Fixtures | `orca_whirlpool_swap`, `orca_two_hop_swap` |

## Non-protocol activity (runtime and SPL programs)

System Program transfers and account creation, SPL Token and Token-2022 transfers,
`transferChecked`, mint/burn, account initialisation and closure, ATA creation, and WSOL
`syncNative` are decoded as `NATIVE_TRANSFER`, `TOKEN_TRANSFER`, `TOKEN_MINT`,
`TOKEN_BURN`, `ACCOUNT_CREATION`, `ACCOUNT_CLOSURE`, `WRAP_SOL` and `UNWRAP_SOL`. These
are never classified as swaps. Compute Budget and Memo instructions are infrastructure.

| Fixture | What it checks |
|---|---|
| `token_2022_transfer_fee` | `TransferCheckedWithFee`: 1.0 sent, 0.01 withheld, net received |
| `wrap_and_close_wsol` | ATA create, wrap, sync, close: no double-counted SOL |
| `failed_swap` | `FAILED_TRANSACTION` + `FEE`, never a swap |
| `unsupported_program` | `UNKNOWN_ACTIVITY` / `UNSUPPORTED` with evidence kept |
| `no_stack_height` | legacy metadata without `stackHeight`: decoded, with an info issue |
