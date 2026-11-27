//! Jupiter Aggregator v6. Source: jup-ag/instruction-parser
//! (`src/idl/jupiter.ts`, `src/lib/instruction-parser.ts`).
//!
//! The user-level swap is what left the user's source token account and what
//! arrived in the destination account, both observed as token transfers
//! inside the route instruction. Route legs come from Jupiter's own
//! `SwapEvent` self-CPIs. DEX instructions executed inside the route are
//! legs, never separate swaps (the pipeline claims the subtree).

use super::{
    Ctx, Decoded, ProtocolDecoder, SwapDraft, cross_check, mint_of, received_by, sent_from, side,
};
use crate::borsh::Reader;
use crate::model::{
    ActivityKind, DecodeStatus, FeeKind, FeeProvenance, LegEvidence, RouteLeg, SwapFee, VenueKind,
};
use crate::programs::{JUPITER_V6, jupiter as j, protocol_label};
use crate::quality::{Issue, IssueCode};

#[derive(Debug, Clone, Copy)]
pub struct JupiterV6;

#[derive(Debug, Clone, Copy)]
struct Layout {
    authority: usize,
    source: usize,
    destination: usize,
    /// Optional explicit destination account (Anchor `Option<Account>`;
    /// the program id stands in for `None`).
    destination_override: Option<usize>,
    source_mint: Option<usize>,
    destination_mint: usize,
    min_accounts: usize,
    exact_out: bool,
}

fn layout(disc: [u8; 8]) -> Option<Layout> {
    let route = Layout {
        authority: 1,
        source: 2,
        destination: 3,
        destination_override: Some(4),
        source_mint: None,
        destination_mint: 5,
        min_accounts: 7,
        exact_out: false,
    };
    let shared = Layout {
        authority: 2,
        source: 3,
        destination: 6,
        destination_override: None,
        source_mint: Some(7),
        destination_mint: 8,
        min_accounts: 10,
        exact_out: false,
    };
    Some(match disc {
        d if d == j::ROUTE || d == j::ROUTE_WITH_TOKEN_LEDGER => route,
        d if d == j::SHARED_ACCOUNTS_ROUTE || d == j::SHARED_ACCOUNTS_ROUTE_WITH_TOKEN_LEDGER => {
            shared
        }
        d if d == j::EXACT_OUT_ROUTE => Layout {
            source_mint: Some(5),
            destination_mint: 6,
            min_accounts: 8,
            exact_out: true,
            ..route
        },
        d if d == j::SHARED_ACCOUNTS_EXACT_OUT_ROUTE => Layout {
            exact_out: true,
            ..shared
        },
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapEvent {
    pub amm: String,
    pub input_mint: String,
    pub input_amount: u64,
    pub output_mint: String,
    pub output_amount: u64,
}

#[must_use]
pub fn parse_swap_event(body: &[u8]) -> Option<SwapEvent> {
    let mut r = Reader::new(body);
    Some(SwapEvent {
        amm: r.pubkey()?,
        input_mint: r.pubkey()?,
        input_amount: r.u64()?,
        output_mint: r.pubkey()?,
        output_amount: r.u64()?,
    })
}

#[must_use]
pub fn parse_fee_event(body: &[u8]) -> Option<(String, String, u64)> {
    let mut r = Reader::new(body);
    Some((r.pubkey()?, r.pubkey()?, r.u64()?))
}

impl ProtocolDecoder for JupiterV6 {
    fn id(&self) -> &'static str {
        "jupiter_v6"
    }
    fn protocol(&self) -> &'static str {
        "JUPITER"
    }
    fn program_id(&self) -> &'static str {
        JUPITER_V6
    }

    fn decode(&self, cx: &Ctx<'_>, i: usize) -> Option<Decoded> {
        let ix = cx.tx.ixs.get(i)?;
        if ix.is_anchor_event() {
            return Some(Decoded::Ignore);
        }
        let Some(l) = layout(ix.disc()?) else {
            return Some(Decoded::Unsupported {
                protocol: self.protocol(),
                name: "instruction",
            });
        };
        if ix.accounts.len() < l.min_accounts {
            return Some(Decoded::Unsupported {
                protocol: self.protocol(),
                name: "route(unexpected layout)",
            });
        }
        let ix_ref = ix.id.to_string();
        let authority = ix.account(l.authority);
        let src = ix.account(l.source);
        let dst = l
            .destination_override
            .and_then(|k| ix.account(k))
            .filter(|a| *a != JUPITER_V6)
            .or_else(|| ix.account(l.destination));
        let src_mint = l
            .source_mint
            .and_then(|k| ix.account(k))
            .map(str::to_owned)
            .or_else(|| src.and_then(|a| mint_of(cx, a)));
        let dst_mint = ix.account(l.destination_mint).map(str::to_owned);

        // Route legs and platform fees from Jupiter's own events.
        let mut legs = Vec::new();
        let mut fees = Vec::new();
        for d in cx.tx.descendants(i) {
            let Some(c) = cx.tx.ixs.get(d) else { continue };
            if c.program != JUPITER_V6 {
                continue;
            }
            match c.anchor_event() {
                Some((ed, body)) if ed == j::EVENT_SWAP => {
                    if let Some(e) = parse_swap_event(body) {
                        // The venue instruction executed just before the event.
                        let venue_ix = cx
                            .tx
                            .descendants(i)
                            .filter(|&k| k < d)
                            .rev()
                            .find(|&k| cx.tx.ixs.get(k).is_some_and(|x| x.program == e.amm));
                        legs.push(RouteLeg {
                            protocol: protocol_label(&e.amm).unwrap_or("UNKNOWN_AMM").to_owned(),
                            program_id: Some(e.amm),
                            pool: None,
                            input_mint: e.input_mint,
                            input_amount: e.input_amount,
                            output_mint: e.output_mint,
                            output_amount: e.output_amount,
                            instruction_ref: venue_ix
                                .and_then(|k| cx.tx.ixs.get(k))
                                .map(|x| x.id.to_string()),
                            evidence: LegEvidence::ProgramEvent,
                        });
                    }
                }
                Some((ed, body)) if ed == j::EVENT_FEE => {
                    if let Some((account, mint, amount)) = parse_fee_event(body) {
                        fees.push(SwapFee {
                            kind: FeeKind::Platform,
                            mint: Some(mint),
                            amount: Some(amount),
                            provenance: FeeProvenance::Explicit,
                            recipient: Some(account),
                        });
                    }
                }
                _ => {}
            }
        }

        let mut status = DecodeStatus::Verified;
        let mut reasons = Vec::new();
        let mut issues = Vec::new();
        let mut in_amt = src.and_then(|a| sent_from(cx, i, a));
        let mut out_amt = dst.and_then(|a| received_by(cx, i, a));
        let src_legs: u128 = legs
            .iter()
            .filter(|g| Some(&g.input_mint) == src_mint.as_ref())
            .map(|g| u128::from(g.input_amount))
            .sum();
        let sink_legs: u128 = legs
            .iter()
            .filter(|g| Some(&g.output_mint) == dst_mint.as_ref())
            .map(|g| u128::from(g.output_amount))
            .sum();
        if !cx.tx.inner_complete {
            status = DecodeStatus::InsufficientEvidence;
            reasons.push("INNER_INSTRUCTIONS_MISSING".to_owned());
        } else if in_amt.is_none() || out_amt.is_none() {
            // Fall back to route events, flagged.
            if in_amt.is_none() && src_legs > 0 {
                in_amt = u64::try_from(src_legs).ok();
            }
            if out_amt.is_none() && sink_legs > 0 {
                out_amt = u64::try_from(sink_legs).ok();
            }
            status = DecodeStatus::PartiallyDecoded;
            reasons.push("AMOUNTS_FROM_ROUTE_EVENTS".to_owned());
            issues.push(Issue::new(
                IssueCode::DecoderAmountMismatch,
                Some(ix_ref.clone()),
                "user source/destination transfer not observed; amounts from SwapEvents",
            ));
        }
        if let (Some(a), Some(v)) = (src, in_amt)
            && let Some(iss) = cross_check(cx, i, a, -i128::from(v))
        {
            status = status.weaker(DecodeStatus::PartiallyDecoded);
            reasons.push("SOURCE_DELTA_MISMATCH".to_owned());
            issues.push(iss);
        }
        let route_complete = !legs.is_empty()
            && in_amt.is_some_and(|v| u128::from(v) == src_legs)
            && out_amt.is_some_and(|v| sink_legs >= u128::from(v));
        if !route_complete && status == DecodeStatus::Verified {
            reasons.push("ROUTE_INCOMPLETE".to_owned());
            issues.push(Issue::new(
                IssueCode::RouteIncomplete,
                Some(ix_ref),
                format!(
                    "{} SwapEvent legs do not reconcile with the user amounts",
                    legs.len()
                ),
            ));
        }
        if l.exact_out {
            reasons.push("EXACT_OUT".to_owned());
        }
        Some(Decoded::Swap(Box::new(SwapDraft {
            protocol: self.protocol(),
            venue_kind: VenueKind::Aggregator,
            activity_kind: ActivityKind::Swap,
            pool: None,
            authority: authority.map(str::to_owned),
            input: side(cx, src, src_mint, in_amt),
            output: side(cx, dst, dst_mint, out_amt),
            legs,
            fees,
            route_complete,
            status,
            reasons,
            issues,
        })))
    }
}
