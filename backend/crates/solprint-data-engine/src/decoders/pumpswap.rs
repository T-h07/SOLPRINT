//! PumpSwap (pump AMM). Source: pump-fun/pump-public-docs `idl/pump_amm.json`.
//!
//! `BuyEvent` / `SellEvent` carry explicit user-level quote amounts
//! (`user_quote_amount_in` / `user_quote_amount_out`, fees included), which
//! are used directly and cross-checked against the user's token transfers.

use super::{Ctx, Decoded, ProtocolDecoder, direct_swap, lifecycle, received_by, sent_from, side};
use crate::borsh::{pubkey_at, u64_at};
use crate::model::{
    ActivityKind, DecodeStatus, FeeKind, FeeProvenance, LegEvidence, RouteLeg, SwapFee, VenueKind,
};
use crate::programs::{PUMPSWAP, pumpswap as ps};
use crate::quality::{Issue, IssueCode};

#[derive(Debug, Clone, Copy)]
pub struct PumpSwap;

/// Byte offsets inside the event body (after tag + discriminator); both
/// events share the first 23 fields' layout.
mod off {
    pub const BASE_AMOUNT: usize = 8;
    pub const LP_FEE: usize = 72;
    pub const PROTOCOL_FEE: usize = 88;
    pub const USER_QUOTE_AMOUNT: usize = 104;
    pub const POOL: usize = 112;
    pub const USER: usize = 144;
    pub const PROTOCOL_FEE_RECIPIENT: usize = 240;
    pub const COIN_CREATOR: usize = 304;
    pub const COIN_CREATOR_FEE: usize = 344;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmmEvent {
    pub base_amount: u64,
    pub user_quote_amount: u64,
    pub lp_fee: u64,
    pub protocol_fee: u64,
    pub pool: String,
    pub user: String,
    pub protocol_fee_recipient: String,
    pub coin_creator: Option<String>,
    pub coin_creator_fee: Option<u64>,
}

#[must_use]
pub fn parse_event(body: &[u8]) -> Option<AmmEvent> {
    Some(AmmEvent {
        base_amount: u64_at(body, off::BASE_AMOUNT)?,
        user_quote_amount: u64_at(body, off::USER_QUOTE_AMOUNT)?,
        lp_fee: u64_at(body, off::LP_FEE)?,
        protocol_fee: u64_at(body, off::PROTOCOL_FEE)?,
        pool: pubkey_at(body, off::POOL)?,
        user: pubkey_at(body, off::USER)?,
        protocol_fee_recipient: pubkey_at(body, off::PROTOCOL_FEE_RECIPIENT)?,
        coin_creator: pubkey_at(body, off::COIN_CREATOR),
        coin_creator_fee: u64_at(body, off::COIN_CREATOR_FEE),
    })
}

impl ProtocolDecoder for PumpSwap {
    fn id(&self) -> &'static str {
        "pumpswap"
    }
    fn protocol(&self) -> &'static str {
        "PUMPSWAP"
    }
    fn program_id(&self) -> &'static str {
        PUMPSWAP
    }

    fn decode(&self, cx: &Ctx<'_>, i: usize) -> Option<Decoded> {
        let ix = cx.tx.ixs.get(i)?;
        if ix.is_anchor_event() {
            return Some(Decoded::Ignore);
        }
        let disc = ix.disc()?;
        let is_buy = if disc == ps::BUY
            || disc == ps::BUY_V2
            || disc == ps::BUY_EXACT_QUOTE_IN
            || disc == ps::BUY_EXACT_QUOTE_IN_V2
        {
            true
        } else if disc == ps::SELL || disc == ps::SELL_V2 {
            false
        } else {
            return Some(match disc {
                d if d == ps::DEPOSIT => {
                    lifecycle(cx, i, ActivityKind::LiquidityDeposit, self.protocol(), 2)
                }
                d if d == ps::WITHDRAW => {
                    lifecycle(cx, i, ActivityKind::LiquidityWithdrawal, self.protocol(), 2)
                }
                d if d == ps::CREATE_POOL => {
                    lifecycle(cx, i, ActivityKind::PoolCreation, self.protocol(), 2)
                }
                _ => Decoded::Unsupported {
                    protocol: self.protocol(),
                    name: "instruction",
                },
            });
        };
        // pool(0), user(1), global_config(2), base_mint(3), quote_mint(4),
        // user_base_token_account(5), user_quote_token_account(6), ...
        if ix.accounts.len() < 9 {
            return Some(Decoded::Unsupported {
                protocol: self.protocol(),
                name: "trade(unexpected layout)",
            });
        }
        let (pool, user, base_mint, quote_mint) =
            (ix.account(0), ix.account(1), ix.account(3), ix.account(4));
        let (user_base, user_quote) = (ix.account(5), ix.account(6));
        let want = if is_buy {
            ps::EVENT_BUY
        } else {
            ps::EVENT_SELL
        };
        let event = cx.tx.descendants(i).find_map(|d| {
            let c = cx.tx.ixs.get(d)?;
            if c.program != PUMPSWAP || c.parent != Some(i) {
                return None;
            }
            let (ed, body) = c.anchor_event()?;
            (ed == want).then(|| parse_event(body)).flatten()
        });
        let (src, dst) = if is_buy {
            (user_quote, user_base)
        } else {
            (user_base, user_quote)
        };
        let (in_mint, out_mint) = if is_buy {
            (quote_mint, base_mint)
        } else {
            (base_mint, quote_mint)
        };
        let Some(ev) = event else {
            // No event: fall back to the user's token transfers.
            let mut d = direct_swap(
                cx,
                i,
                self.protocol(),
                VenueKind::ConstantProductAmm,
                pool,
                user,
                src,
                dst,
                (in_mint.map(str::to_owned), out_mint.map(str::to_owned)),
            );
            d.status = d.status.weaker(DecodeStatus::PartiallyDecoded);
            d.reasons.push("SWAP_EVENT_MISSING".to_owned());
            d.issues.push(Issue::new(
                IssueCode::EventMissing,
                Some(ix.id.to_string()),
                "PumpSwap Buy/SellEvent self-CPI not found",
            ));
            return Some(Decoded::Swap(Box::new(d)));
        };
        let ix_ref = ix.id.to_string();
        let (in_amt, out_amt) = if is_buy {
            (ev.user_quote_amount, ev.base_amount)
        } else {
            (ev.base_amount, ev.user_quote_amount)
        };
        let mut status = DecodeStatus::Verified;
        let mut reasons = Vec::new();
        let mut issues = Vec::new();
        if Some(ev.user.as_str()) != user || Some(ev.pool.as_str()) != pool {
            status = DecodeStatus::Ambiguous;
            reasons.push("EVENT_INSTRUCTION_MISMATCH".to_owned());
        }
        // Cross-check the base side against the user's base transfer (the
        // quote side may be split between pool and fee recipients).
        let base_moved = if is_buy {
            user_base.and_then(|a| received_by(cx, i, a))
        } else {
            user_base.and_then(|a| sent_from(cx, i, a))
        };
        if base_moved != Some(ev.base_amount) {
            status = status.weaker(DecodeStatus::PartiallyDecoded);
            reasons.push("TOKEN_TRANSFER_MISMATCH".to_owned());
            issues.push(Issue::new(
                IssueCode::DecoderAmountMismatch,
                Some(ix_ref.clone()),
                format!(
                    "event base amount {} vs transfer {:?}",
                    ev.base_amount, base_moved
                ),
            ));
        }
        let input = side(cx, src, in_mint.map(str::to_owned), Some(in_amt));
        let output = side(cx, dst, out_mint.map(str::to_owned), Some(out_amt));
        let qm = quote_mint.map(str::to_owned);
        let mut fees = vec![
            SwapFee {
                kind: FeeKind::Other,
                mint: qm.clone(),
                amount: Some(ev.lp_fee),
                provenance: FeeProvenance::Explicit,
                recipient: None,
            },
            SwapFee {
                kind: FeeKind::Protocol,
                mint: qm.clone(),
                amount: Some(ev.protocol_fee),
                provenance: FeeProvenance::Explicit,
                recipient: Some(ev.protocol_fee_recipient.clone()),
            },
        ];
        if let Some(cf) = ev.coin_creator_fee {
            fees.push(SwapFee {
                kind: FeeKind::Creator,
                mint: qm,
                amount: Some(cf),
                provenance: FeeProvenance::Explicit,
                recipient: ev.coin_creator.clone(),
            });
        }
        let legs = match (&input.mint, &output.mint) {
            (Some(im), Some(om)) => vec![RouteLeg {
                protocol: self.protocol().to_owned(),
                program_id: Some(PUMPSWAP.to_owned()),
                pool: pool.map(str::to_owned),
                input_mint: im.clone(),
                input_amount: in_amt,
                output_mint: om.clone(),
                output_amount: out_amt,
                instruction_ref: Some(ix_ref),
                evidence: LegEvidence::ProgramEvent,
            }],
            _ => Vec::new(),
        };
        Some(Decoded::Swap(Box::new(super::SwapDraft {
            protocol: self.protocol(),
            venue_kind: VenueKind::ConstantProductAmm,
            activity_kind: ActivityKind::Swap,
            pool: pool.map(str::to_owned),
            authority: user.map(str::to_owned),
            route_complete: !legs.is_empty(),
            input,
            output,
            legs,
            fees,
            status,
            reasons,
            issues,
        })))
    }
}
