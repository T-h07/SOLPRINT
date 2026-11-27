//! Raydium AMM v4, CPMM and CLMM.
//!
//! Sources: raydium-io/raydium-amm `program/src/instruction.rs`,
//! raydium-io/raydium-cp-swap `programs/cp-swap/src/instructions/swap_base_input.rs`,
//! raydium-io/raydium-clmm `programs/amm/src/instructions/swap.rs`, `swap_v2.rs`
//! (commits recorded in DECODER_CAPABILITIES.md).

use super::{Ctx, Decoded, ProtocolDecoder, direct_swap, lifecycle, lifecycle_by_signer};
use crate::model::{ActivityKind, VenueKind};
use crate::programs::{RAYDIUM_AMM_V4, RAYDIUM_CLMM, RAYDIUM_CPMM, raydium as r};

#[derive(Debug, Clone, Copy)]
pub struct AmmV4;

impl ProtocolDecoder for AmmV4 {
    fn id(&self) -> &'static str {
        "raydium_amm_v4"
    }
    fn protocol(&self) -> &'static str {
        "RAYDIUM_AMM_V4"
    }
    fn program_id(&self) -> &'static str {
        RAYDIUM_AMM_V4
    }
    fn decode(&self, cx: &Ctx<'_>, i: usize) -> Option<Decoded> {
        let ix = cx.tx.ixs.get(i)?;
        let tag = *ix.data.first()?;
        match tag {
            // SwapBaseIn/Out: 17 or 18 accounts; V2: 8 accounts. In every
            // layout the last three are user_source, user_destination,
            // user_source_owner and account 1 is the AMM.
            r::AMM_SWAP_BASE_IN
            | r::AMM_SWAP_BASE_OUT
            | r::AMM_SWAP_BASE_IN_V2
            | r::AMM_SWAP_BASE_OUT_V2 => {
                let n = ix.accounts.len();
                let ok = match tag {
                    r::AMM_SWAP_BASE_IN | r::AMM_SWAP_BASE_OUT => n == 17 || n == 18,
                    _ => n == 8,
                };
                if !ok || ix.data.len() != 17 {
                    return Some(Decoded::Unsupported {
                        protocol: self.protocol(),
                        name: "swap(unexpected layout)",
                    });
                }
                let src = ix.account(n - 3);
                let dst = ix.account(n - 2);
                let owner = ix.account(n - 1);
                Some(Decoded::Swap(Box::new(direct_swap(
                    cx,
                    i,
                    self.protocol(),
                    VenueKind::ConstantProductAmm,
                    ix.account(1),
                    owner,
                    src,
                    dst,
                    (None, None),
                ))))
            }
            // Deposit / Withdraw / Initialize2 user-owner positions differ
            // between layouts; attribute only to a transaction signer.
            r::AMM_DEPOSIT => Some(lifecycle_by_signer(
                cx,
                i,
                ActivityKind::LiquidityDeposit,
                self.protocol(),
            )),
            r::AMM_WITHDRAW => Some(lifecycle_by_signer(
                cx,
                i,
                ActivityKind::LiquidityWithdrawal,
                self.protocol(),
            )),
            r::AMM_INITIALIZE2 => Some(lifecycle_by_signer(
                cx,
                i,
                ActivityKind::PoolCreation,
                self.protocol(),
            )),
            _ => Some(Decoded::Unsupported {
                protocol: self.protocol(),
                name: "instruction",
            }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Cpmm;

impl ProtocolDecoder for Cpmm {
    fn id(&self) -> &'static str {
        "raydium_cpmm"
    }
    fn protocol(&self) -> &'static str {
        "RAYDIUM_CPMM"
    }
    fn program_id(&self) -> &'static str {
        RAYDIUM_CPMM
    }
    fn decode(&self, cx: &Ctx<'_>, i: usize) -> Option<Decoded> {
        let ix = cx.tx.ixs.get(i)?;
        let disc = ix.disc()?;
        if disc == r::CPMM_SWAP_BASE_INPUT || disc == r::CPMM_SWAP_BASE_OUTPUT {
            // payer, authority, amm_config, pool_state, input_token_account,
            // output_token_account, input_vault, output_vault,
            // input_token_program, output_token_program, input_token_mint,
            // output_token_mint, observation_state
            if ix.accounts.len() < 13 {
                return Some(Decoded::Unsupported {
                    protocol: self.protocol(),
                    name: "swap(unexpected layout)",
                });
            }
            return Some(Decoded::Swap(Box::new(direct_swap(
                cx,
                i,
                self.protocol(),
                VenueKind::ConstantProductAmm,
                ix.account(3),
                ix.account(0),
                ix.account(4),
                ix.account(5),
                (
                    ix.account(10).map(str::to_owned),
                    ix.account(11).map(str::to_owned),
                ),
            ))));
        }
        Some(match disc {
            d if d == r::CPMM_DEPOSIT => {
                lifecycle(cx, i, ActivityKind::LiquidityDeposit, self.protocol(), 0)
            }
            d if d == r::CPMM_WITHDRAW => {
                lifecycle(cx, i, ActivityKind::LiquidityWithdrawal, self.protocol(), 0)
            }
            d if d == r::CPMM_INITIALIZE => {
                lifecycle(cx, i, ActivityKind::PoolCreation, self.protocol(), 0)
            }
            _ if ix.is_anchor_event() => Decoded::Ignore,
            _ => Decoded::Unsupported {
                protocol: self.protocol(),
                name: "instruction",
            },
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Clmm;

impl ProtocolDecoder for Clmm {
    fn id(&self) -> &'static str {
        "raydium_clmm"
    }
    fn protocol(&self) -> &'static str {
        "RAYDIUM_CLMM"
    }
    fn program_id(&self) -> &'static str {
        RAYDIUM_CLMM
    }
    fn decode(&self, cx: &Ctx<'_>, i: usize) -> Option<Decoded> {
        let ix = cx.tx.ixs.get(i)?;
        let disc = ix.disc()?;
        if disc == r::CLMM_SWAP || disc == r::CLMM_SWAP_V2 {
            // payer, amm_config, pool_state, input_token_account,
            // output_token_account, input_vault, output_vault,
            // observation_state, ... ; v2 adds input_vault_mint (11) and
            // output_vault_mint (12).
            let v2 = disc == r::CLMM_SWAP_V2;
            if ix.accounts.len() < if v2 { 13 } else { 10 } {
                return Some(Decoded::Unsupported {
                    protocol: self.protocol(),
                    name: "swap(unexpected layout)",
                });
            }
            let mints = if v2 {
                (
                    ix.account(11).map(str::to_owned),
                    ix.account(12).map(str::to_owned),
                )
            } else {
                (None, None)
            };
            return Some(Decoded::Swap(Box::new(direct_swap(
                cx,
                i,
                self.protocol(),
                VenueKind::ConcentratedLiquidity,
                ix.account(2),
                ix.account(0),
                ix.account(3),
                ix.account(4),
                mints,
            ))));
        }
        Some(match disc {
            d if d == r::CLMM_SWAP_ROUTER_BASE_IN => Decoded::Unsupported {
                protocol: self.protocol(),
                name: "swap_router_base_in",
            },
            d if d == r::CLMM_INCREASE_LIQUIDITY || d == r::CLMM_INCREASE_LIQUIDITY_V2 => {
                lifecycle(cx, i, ActivityKind::LiquidityDeposit, self.protocol(), 0)
            }
            d if d == r::CLMM_DECREASE_LIQUIDITY || d == r::CLMM_DECREASE_LIQUIDITY_V2 => {
                lifecycle(cx, i, ActivityKind::LiquidityWithdrawal, self.protocol(), 0)
            }
            d if d == r::CLMM_OPEN_POSITION
                || d == r::CLMM_OPEN_POSITION_V2
                || d == r::CLMM_OPEN_POSITION_WITH_TOKEN22_NFT =>
            {
                lifecycle(cx, i, ActivityKind::LiquidityDeposit, self.protocol(), 0)
            }
            d if d == r::CLMM_CREATE_POOL => {
                lifecycle(cx, i, ActivityKind::PoolCreation, self.protocol(), 0)
            }
            _ if ix.is_anchor_event() => Decoded::Ignore,
            _ => Decoded::Unsupported {
                protocol: self.protocol(),
                name: "instruction",
            },
        })
    }
}
