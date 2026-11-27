# Wallet attribution

Which wallet a swap belongs to, and how sure the DATA ENGINE is. Code:
`backend/crates/solprint-data-engine/src/attribution.rs`.

## Rule

The wallet of a swap is **the account the protocol requires as the user / transfer
authority** for that instruction (read by verified account position; see
[DECODER_CAPABILITIES.md](DECODER_CAPABILITIES.md)), **and only when that account signed
the transaction**.

- **The fee payer is never used as a substitute.** Relayers, bots and sponsored
  transactions often pay fees for someone else. Fixture: `relayed_swap_fee_payer_differs`.
- **An authority that did not sign** can only have authorised the instruction through
  `invoke_signed`, which means it is a program address. The swap is `PROGRAM_CONTROLLED`
  and gets no wallet. Fixture: `program_controlled_swap`.
- **The beneficiary** (the owner of the output account) is recorded separately. When it
  differs from the authority, the reason `BENEFICIARY_DIFFERS` is added; the beneficiary
  is not promoted to the wallet.

## States

| Attribution | When | Wallet set |
|---|---|---|
| `VERIFIED_OWNER` | The authority signed and owns the input token account | yes |
| `VERIFIED_AUTHORITY` | The authority signed but is not the input account's owner (`AUTHORITY_IS_DELEGATE`), or the owner is not in the evidence (`SOURCE_OWNER_UNKNOWN`) | yes |
| `PROGRAM_CONTROLLED` | The authority did not sign (`AUTHORITY_IS_PROGRAM_SIGNER`) | no |
| `UNKNOWN_OWNER` | The decoder could not identify an authority (`AUTHORITY_UNKNOWN`) | no |
| `VERIFIED_BENEFICIARY`, `MULTIPLE_POSSIBLE_ACTORS`, `AMBIGUOUS` | Part of the contract for later decoders; **not produced by Phase 02 rules** | — |

Every non-verified attribution raises an `ATTRIBUTION_AMBIGUOUS` info issue on the swap.
Only verified states set `wallet` and count toward verified trades
([SWAP_NORMALIZATION.md](SWAP_NORMALIZATION.md#status)).

## Ownership evidence

An account's owner comes only from the transaction's own evidence, in this order:
token-balance metadata (`owner`), then `initializeAccount*` and ATA-creation
instructions. A native SOL side names the wallet itself. Nothing is looked up from
outside the transaction, and nothing is inferred from naming or history.

## What attribution is not

Attribution says who executed a swap. It does not cluster wallets into entities, does not
rank or score wallets, and does not judge intent. Those are out of scope for Phase 02.
