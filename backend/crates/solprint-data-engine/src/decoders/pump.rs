//! Pump.fun bonding curve. Source: pump-fun/pump-public-docs `idl/pump.json`.
//!
//! Trade amounts come from the program's own `TradeEvent`, emitted through an
//! Anchor self-CPI (`emit_cpi!`) inside the trade instruction — a program
//! log line is never used as evidence. The token side is cross-checked
//! against the token transfer into / out of the user's token account.
//! Amounts are the curve's gross quote amounts; protocol and creator fees
//! are reported as explicit fee rows.

use super::{Ctx, Decoded, ProtocolDecoder, SwapDraft, received_by, sent_from, side};
use crate::borsh::Reader;
use crate::model::{
    ActivityKind, AssetKind, DecodeStatus, FeeKind, FeeProvenance, LegEvidence, RouteLeg, SwapFee,
    SwapSide, VenueKind,
};
use crate::programs::{DEFAULT_PUBKEY, PUMP_FUN, WSOL_MINT, pump as p};
use crate::quality::{Issue, IssueCode};

#[derive(Debug, Clone, Copy)]
pub struct PumpFun;

/// The fields of `TradeEvent` SOLPRINT uses. Older program versions emit a
/// shorter event; fields after `creator_fee` are optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeEvent {
    pub mint: String,
    pub sol_amount: u64,
    pub token_amount: u64,
    pub is_buy: bool,
    pub user: String,
    pub fee_recipient: String,
    pub fee: u64,
    pub creator: String,
    pub creator_fee: u64,
    /// `None` on older events or for SOL-quoted curves.
    pub quote: Option<(String, u64)>,
}

/// Parse a `TradeEvent` body (after the 16-byte tag + discriminator).
#[must_use]
pub fn parse_trade_event(body: &[u8]) -> Option<TradeEvent> {
    let mut r = Reader::new(body);
    let mint = r.pubkey()?;
    let sol_amount = r.u64()?;
    let token_amount = r.u64()?;
    let is_buy = r.bool()?;
    let user = r.pubkey()?;
    r.skip(8 + 8 * 4)?; // timestamp, virtual/real reserves
    let fee_recipient = r.pubkey()?;
    r.skip(8)?; // fee_basis_points
    let fee = r.u64()?;
    let creator = r.pubkey()?;
    r.skip(8)?; // creator_fee_basis_points
    let creator_fee = r.u64()?;
    let quote = (|| {
        r.skip(1 + 8 * 3 + 8)?; // track_volume, unclaimed, claimed, current volume, last update
        r.string()?; // ix_name
        r.skip(1 + 8 * 4)?; // mayhem_mode, cashback bps/amount, buyback bps/amount
        let holders = usize::try_from(r.u32()?).ok()?;
        r.skip(holders.checked_mul(34)?)?; // Shareholder { address, share_bps u16 }
        let qm = r.pubkey()?;
        let qa = r.u64()?;
        (qm != DEFAULT_PUBKEY).then_some((qm, qa))
    })();
    Some(TradeEvent {
        mint,
        sol_amount,
        token_amount,
        is_buy,
        user,
        fee_recipient,
        fee,
        creator,
        creator_fee,
        quote,
    })
}

/// (base mint, bonding curve, user, user base account, user quote account, quote mint)
type Layout = (usize, usize, usize, usize, Option<usize>, Option<usize>);

fn layout(disc: [u8; 8]) -> Option<(Layout, bool)> {
    const V1: Layout = (2, 3, 6, 5, None, None);
    const V2: Layout = (1, 10, 13, 14, Some(15), Some(2));
    const V3: Layout = (1, 5, 8, 9, Some(10), Some(2));
    Some(match disc {
        d if d == p::BUY || d == p::BUY_EXACT_SOL_IN => (V1, true),
        d if d == p::SELL => (V1, false),
        d if d == p::BUY_V2 || d == p::BUY_EXACT_QUOTE_IN_V2 => (V2, true),
        d if d == p::SELL_V2 => (V2, false),
        d if d == p::BUY_V3 || d == p::BUY_EXACT_QUOTE_IN_V3 => (V3, true),
        d if d == p::SELL_V3 => (V3, false),
        _ => return None,
    })
}

impl ProtocolDecoder for PumpFun {
    fn id(&self) -> &'static str {
        "pump_fun"
    }
    fn protocol(&self) -> &'static str {
        "PUMP_FUN"
    }
    fn program_id(&self) -> &'static str {
        PUMP_FUN
    }

    fn decode(&self, cx: &Ctx<'_>, i: usize) -> Option<Decoded> {
        let ix = cx.tx.ixs.get(i)?;
        if ix.is_anchor_event() {
            return Some(Decoded::Ignore);
        }
        let disc = ix.disc()?;
        let Some(((mint_i, curve_i, user_i, base_i, quote_i, qmint_i), is_buy)) = layout(disc)
        else {
            return Some(match disc {
                d if d == p::CREATE || d == p::CREATE_V2 => {
                    let actor = if d == p::CREATE { 7 } else { 5 };
                    with_reason(
                        super::lifecycle(cx, i, ActivityKind::PoolCreation, self.protocol(), actor),
                        "BONDING_CURVE_CREATED",
                    )
                }
                d if d == p::MIGRATE || d == p::MIGRATE_V2 => {
                    let actor = if d == p::MIGRATE { 5 } else { 7 };
                    super::lifecycle(
                        cx,
                        i,
                        ActivityKind::LiquidityMigration,
                        self.protocol(),
                        actor,
                    )
                }
                _ => Decoded::Unsupported {
                    protocol: self.protocol(),
                    name: "instruction",
                },
            });
        };
        if ix.accounts.len() <= base_i.max(user_i).max(quote_i.unwrap_or(0)) {
            return Some(Decoded::Unsupported {
                protocol: self.protocol(),
                name: "trade(unexpected layout)",
            });
        }
        let ix_ref = ix.id.to_string();
        let user = ix.account(user_i);
        let base_acct = ix.account(base_i);
        let base_mint = ix.account(mint_i).map(str::to_owned);
        let mut issues = Vec::new();
        let mut reasons = vec!["GROSS_OF_EXPLICIT_FEES".to_owned()];
        let mut status = DecodeStatus::Verified;

        let mut event = None;
        let mut post_complete = false;
        for d in cx.tx.descendants(i) {
            let Some(child) = cx.tx.ixs.get(d) else {
                continue;
            };
            if child.program != PUMP_FUN || child.parent != Some(i) {
                continue;
            }
            match child.anchor_event() {
                Some((ed, body)) if ed == p::EVENT_TRADE && event.is_none() => {
                    event = parse_trade_event(body)
                }
                Some((ed, _)) if ed == p::EVENT_POST_COMPLETE_BUY => post_complete = true,
                _ => {}
            }
        }
        let Some(ev) = event else {
            let (s, why) = if cx.tx.inner_complete {
                (DecodeStatus::PartiallyDecoded, "TRADE_EVENT_MISSING")
            } else {
                (
                    DecodeStatus::InsufficientEvidence,
                    "INNER_INSTRUCTIONS_MISSING",
                )
            };
            issues.push(Issue::new(
                IssueCode::EventMissing,
                Some(ix_ref.clone()),
                "Pump.fun TradeEvent self-CPI not found",
            ));
            let base_amount = base_acct.and_then(|a| {
                if is_buy {
                    received_by(cx, i, a)
                } else {
                    sent_from(cx, i, a)
                }
            });
            let base = side(cx, base_acct, base_mint, base_amount);
            let quote = SwapSide {
                mint: None,
                amount: None,
                decimals: None,
                asset_kind: AssetKind::Unknown,
                account: user.map(str::to_owned),
            };
            let (input, output) = if is_buy { (quote, base) } else { (base, quote) };
            return Some(Decoded::Swap(Box::new(SwapDraft {
                protocol: self.protocol(),
                venue_kind: VenueKind::BondingCurve,
                activity_kind: ActivityKind::BondingCurveTrade,
                pool: ix.account(curve_i).map(str::to_owned),
                authority: user.map(str::to_owned),
                input,
                output,
                legs: Vec::new(),
                fees: Vec::new(),
                route_complete: false,
                status: s,
                reasons: vec![why.to_owned()],
                issues,
            })));
        };

        if ev.is_buy != is_buy
            || Some(ev.user.as_str()) != user
            || base_mint.as_deref() != Some(ev.mint.as_str())
        {
            status = DecodeStatus::Ambiguous;
            reasons.push("EVENT_INSTRUCTION_MISMATCH".to_owned());
            issues.push(Issue::new(
                IssueCode::DecoderAmountMismatch,
                Some(ix_ref.clone()),
                "TradeEvent user/mint/direction differs from the instruction accounts",
            ));
        }
        if post_complete {
            status = status.weaker(DecodeStatus::PartiallyDecoded);
            reasons.push("POST_COMPLETE_BUY_SPLIT".to_owned());
        }
        // Token side cross-check.
        if let Some(a) = base_acct {
            let moved = if is_buy {
                received_by(cx, i, a)
            } else {
                sent_from(cx, i, a)
            };
            if moved != Some(ev.token_amount) {
                status = status.weaker(DecodeStatus::PartiallyDecoded);
                reasons.push("TOKEN_TRANSFER_MISMATCH".to_owned());
                issues.push(Issue::new(
                    IssueCode::DecoderAmountMismatch,
                    Some(ix_ref.clone()),
                    format!(
                        "TradeEvent token_amount {} vs token transfer {:?}",
                        ev.token_amount, moved
                    ),
                ));
            }
        }
        let base = side(cx, base_acct, base_mint, Some(ev.token_amount));
        let quote_mint_acct = qmint_i.and_then(|q| ix.account(q)).map(str::to_owned);
        let quote = match (&ev.quote, quote_mint_acct.as_deref()) {
            (Some((qm, qa)), _) => side(
                cx,
                quote_i.and_then(|q| ix.account(q)),
                Some(qm.clone()),
                Some(*qa),
            ),
            (None, Some(qm)) if qm != WSOL_MINT && qm != DEFAULT_PUBKEY => {
                // Non-SOL quote with an event that predates quote fields.
                status = status.weaker(DecodeStatus::PartiallyDecoded);
                reasons.push("QUOTE_AMOUNT_UNKNOWN".to_owned());
                side(
                    cx,
                    quote_i.and_then(|q| ix.account(q)),
                    Some(qm.to_owned()),
                    None,
                )
            }
            _ => SwapSide {
                mint: Some(WSOL_MINT.to_owned()),
                amount: Some(ev.sol_amount),
                decimals: Some(9),
                asset_kind: AssetKind::NativeSol,
                account: user.map(str::to_owned),
            },
        };
        let fee_mint = quote.mint.clone();
        let fees = vec![
            SwapFee {
                kind: FeeKind::Protocol,
                mint: fee_mint.clone(),
                amount: Some(ev.fee),
                provenance: FeeProvenance::Explicit,
                recipient: Some(ev.fee_recipient.clone()),
            },
            SwapFee {
                kind: FeeKind::Creator,
                mint: fee_mint,
                amount: Some(ev.creator_fee),
                provenance: FeeProvenance::Explicit,
                recipient: Some(ev.creator.clone()),
            },
        ];
        let (input, output) = if is_buy { (quote, base) } else { (base, quote) };
        let legs = match (&input.mint, input.amount, &output.mint, output.amount) {
            (Some(im), Some(ia), Some(om), Some(oa)) => vec![RouteLeg {
                protocol: self.protocol().to_owned(),
                program_id: Some(PUMP_FUN.to_owned()),
                pool: ix.account(curve_i).map(str::to_owned),
                input_mint: im.clone(),
                input_amount: ia,
                output_mint: om.clone(),
                output_amount: oa,
                instruction_ref: Some(ix_ref),
                evidence: LegEvidence::ProgramEvent,
            }],
            _ => Vec::new(),
        };
        Some(Decoded::Swap(Box::new(SwapDraft {
            protocol: self.protocol(),
            venue_kind: VenueKind::BondingCurve,
            activity_kind: ActivityKind::BondingCurveTrade,
            pool: ix.account(curve_i).map(str::to_owned),
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

fn with_reason(d: Decoded, reason: &str) -> Decoded {
    match d {
        Decoded::Activity(mut a) => {
            a.reasons.push(reason.to_owned());
            Decoded::Activity(a)
        }
        other => other,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn key(b: u8) -> [u8; 32] {
        [b; 32]
    }

    pub(crate) fn trade_event_body(
        sol: u64,
        tokens: u64,
        is_buy: bool,
        user: [u8; 32],
        with_quote: bool,
    ) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&key(7)); // mint
        d.extend_from_slice(&sol.to_le_bytes());
        d.extend_from_slice(&tokens.to_le_bytes());
        d.push(u8::from(is_buy));
        d.extend_from_slice(&user);
        d.extend_from_slice(&[0; 8 + 32]); // timestamp + reserves
        d.extend_from_slice(&key(3)); // fee recipient
        d.extend_from_slice(&95_u64.to_le_bytes());
        d.extend_from_slice(&1_000_u64.to_le_bytes()); // fee
        d.extend_from_slice(&key(4)); // creator
        d.extend_from_slice(&5_u64.to_le_bytes());
        d.extend_from_slice(&50_u64.to_le_bytes()); // creator fee
        if with_quote {
            d.extend_from_slice(&[0; 1 + 32]);
            d.extend_from_slice(&4_u32.to_le_bytes());
            d.extend_from_slice(b"sell");
            d.extend_from_slice(&[0; 33]);
            d.extend_from_slice(&1_u32.to_le_bytes());
            d.extend_from_slice(&[9; 34]);
            d.extend_from_slice(&[0; 32]); // default quote mint = SOL
            d.extend_from_slice(&sol.to_le_bytes());
        }
        d
    }

    #[test]
    fn parses_old_and_new_trade_events() {
        let user = key(1);
        let old = parse_trade_event(&trade_event_body(10, 20, true, user, false)).unwrap();
        assert_eq!(
            (old.sol_amount, old.token_amount, old.is_buy),
            (10, 20, true)
        );
        assert_eq!(old.fee, 1_000);
        assert_eq!(old.creator_fee, 50);
        assert_eq!(old.quote, None);
        let new = parse_trade_event(&trade_event_body(10, 20, false, user, true)).unwrap();
        assert_eq!(new.quote, None, "default quote mint means SOL");
        assert!(parse_trade_event(&[0; 40]).is_none());
    }
}
