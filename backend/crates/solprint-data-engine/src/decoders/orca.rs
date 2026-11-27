//! Orca Whirlpool. Source: orca-so/whirlpools `programs/whirlpool/src/instructions/`
//! (`swap.rs`, `v2/swap.rs`, `two_hop_swap.rs`, `v2/two_hop_swap.rs`).
//!
//! Swap args: amount u64, other_amount_threshold u64, sqrt_price_limit u128,
//! amount_specified_is_input bool, a_to_b bool — so `a_to_b` is at byte 41
//! of the instruction data (after the 8-byte discriminator).
//! Two-hop args: amount, threshold, amount_specified_is_input (24),
//! a_to_b_one (25), a_to_b_two (26), then the two price limits.

use super::{Ctx, Decoded, ProtocolDecoder, direct_swap, lifecycle, received_by, sent_from};
use crate::model::{DecodeStatus, LegEvidence, RouteLeg, VenueKind};
use crate::programs::{ORCA_WHIRLPOOL, orca as o};

#[derive(Debug, Clone, Copy)]
pub struct Whirlpool;

fn flag(data: &[u8], at: usize) -> Option<bool> {
    match data.get(at)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

impl ProtocolDecoder for Whirlpool {
    fn id(&self) -> &'static str {
        "orca_whirlpool"
    }
    fn protocol(&self) -> &'static str {
        "ORCA_WHIRLPOOL"
    }
    fn program_id(&self) -> &'static str {
        ORCA_WHIRLPOOL
    }

    fn decode(&self, cx: &Ctx<'_>, i: usize) -> Option<Decoded> {
        let ix = cx.tx.ixs.get(i)?;
        let disc = ix.disc()?;
        let bad = || Decoded::Unsupported {
            protocol: "ORCA_WHIRLPOOL",
            name: "swap(unexpected layout)",
        };
        if disc == o::SWAP || disc == o::SWAP_V2 {
            // swap:    token_program, token_authority(1), whirlpool(2),
            //          token_owner_account_a(3), vault_a(4), token_owner_account_b(5), vault_b(6), ...
            // swap_v2: token_program_a, token_program_b, memo, token_authority(3),
            //          whirlpool(4), mint_a(5), mint_b(6), owner_a(7), vault_a(8), owner_b(9), vault_b(10), ...
            let Some(a_to_b) = flag(&ix.data, 41) else {
                return Some(bad());
            };
            let v2 = disc == o::SWAP_V2;
            let (auth, pool, oa, ob, ma, mb) = if v2 {
                (
                    3,
                    4,
                    7,
                    9,
                    ix.account(5).map(str::to_owned),
                    ix.account(6).map(str::to_owned),
                )
            } else {
                (1, 2, 3, 5, None, None)
            };
            if ix.accounts.len() < if v2 { 15 } else { 11 } {
                return Some(bad());
            }
            let (src, dst, mints) = if a_to_b {
                (ix.account(oa), ix.account(ob), (ma, mb))
            } else {
                (ix.account(ob), ix.account(oa), (mb, ma))
            };
            return Some(Decoded::Swap(Box::new(direct_swap(
                cx,
                i,
                self.protocol(),
                VenueKind::ConcentratedLiquidity,
                ix.account(pool),
                ix.account(auth),
                src,
                dst,
                mints,
            ))));
        }
        if disc == o::TWO_HOP_SWAP || disc == o::TWO_HOP_SWAP_V2 {
            let v2 = disc == o::TWO_HOP_SWAP_V2;
            let (Some(one), Some(two)) = (flag(&ix.data, 25), flag(&ix.data, 26)) else {
                return Some(bad());
            };
            // v1: token_program, token_authority(1), whirlpool_one(2), whirlpool_two(3),
            //     owner_one_a(4), vault_one_a(5), owner_one_b(6), vault_one_b(7),
            //     owner_two_a(8), vault_two_a(9), owner_two_b(10), vault_two_b(11), ...
            // v2: whirlpool_one(0), whirlpool_two(1), mint_input(2), mint_intermediate(3),
            //     mint_output(4), programs(5..8), owner_input(8), vault_one_input(9),
            //     vault_one_intermediate(10), vault_two_intermediate(11),
            //     vault_two_output(12), owner_output(13), token_authority(14), ...
            let (auth, p1, p2, src, dst, mints, mid_vaults) = if v2 {
                if ix.accounts.len() < 15 {
                    return Some(bad());
                }
                (
                    14,
                    0,
                    1,
                    ix.account(8),
                    ix.account(13),
                    (
                        ix.account(2).map(str::to_owned),
                        ix.account(4).map(str::to_owned),
                    ),
                    (ix.account(10), ix.account(11)),
                )
            } else {
                if ix.accounts.len() < 16 {
                    return Some(bad());
                }
                let src = if one { ix.account(4) } else { ix.account(6) };
                let dst = if two { ix.account(10) } else { ix.account(8) };
                // Vault of the intermediate token in pool one (output of hop one).
                let v1_mid = if one { ix.account(7) } else { ix.account(5) };
                let v2_mid = if two { ix.account(9) } else { ix.account(11) };
                (1, 2, 3, src, dst, (None, None), (v1_mid, v2_mid))
            };
            let mut d = direct_swap(
                cx,
                i,
                self.protocol(),
                VenueKind::ConcentratedLiquidity,
                None,
                ix.account(auth),
                src,
                dst,
                mints,
            );
            // Two legs from the intermediate vault transfers.
            let mid_out = mid_vaults.0.and_then(|v| sent_from(cx, i, v));
            let mid_in = mid_vaults.1.and_then(|v| received_by(cx, i, v));
            let mid_mint = mid_vaults.0.and_then(|v| super::mint_of(cx, v));
            d.legs.clear();
            if let (Some(im), Some(ia), Some(mm), Some(mo), Some(mi), Some(om), Some(oa)) = (
                d.input.mint.clone(),
                d.input.amount,
                mid_mint,
                mid_out,
                mid_in,
                d.output.mint.clone(),
                d.output.amount,
            ) {
                let reff = Some(ix.id.to_string());
                d.legs.push(RouteLeg {
                    protocol: self.protocol().to_owned(),
                    program_id: Some(ORCA_WHIRLPOOL.to_owned()),
                    pool: ix.account(p1).map(str::to_owned),
                    input_mint: im,
                    input_amount: ia,
                    output_mint: mm.clone(),
                    output_amount: mo,
                    instruction_ref: reff.clone(),
                    evidence: LegEvidence::TokenTransfers,
                });
                d.legs.push(RouteLeg {
                    protocol: self.protocol().to_owned(),
                    program_id: Some(ORCA_WHIRLPOOL.to_owned()),
                    pool: ix.account(p2).map(str::to_owned),
                    input_mint: mm,
                    input_amount: mi,
                    output_mint: om,
                    output_amount: oa,
                    instruction_ref: reff,
                    evidence: LegEvidence::TokenTransfers,
                });
            }
            d.route_complete = d.legs.len() == 2;
            if !d.route_complete && d.status == DecodeStatus::Verified {
                d.reasons.push("ROUTE_LEGS_UNRESOLVED".to_owned());
            }
            return Some(Decoded::Swap(Box::new(d)));
        }
        Some(match disc {
            // position_authority: index 2 (v1) / 4 (v2: two token programs + memo first).
            d if d == o::INCREASE_LIQUIDITY => lifecycle(
                cx,
                i,
                crate::model::ActivityKind::LiquidityDeposit,
                self.protocol(),
                2,
            ),
            d if d == o::INCREASE_LIQUIDITY_V2 => lifecycle(
                cx,
                i,
                crate::model::ActivityKind::LiquidityDeposit,
                self.protocol(),
                4,
            ),
            d if d == o::DECREASE_LIQUIDITY => lifecycle(
                cx,
                i,
                crate::model::ActivityKind::LiquidityWithdrawal,
                self.protocol(),
                2,
            ),
            d if d == o::DECREASE_LIQUIDITY_V2 => lifecycle(
                cx,
                i,
                crate::model::ActivityKind::LiquidityWithdrawal,
                self.protocol(),
                4,
            ),
            d if d == o::INITIALIZE_POOL || d == o::INITIALIZE_POOL_V2 => {
                super::lifecycle_by_signer(
                    cx,
                    i,
                    crate::model::ActivityKind::PoolCreation,
                    self.protocol(),
                )
            }
            _ => Decoded::Unsupported {
                protocol: self.protocol(),
                name: "instruction",
            },
        })
    }
}
