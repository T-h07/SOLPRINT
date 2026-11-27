//! The pure decode pipeline: raw evidence JSON → [`DecodedTransaction`].
//! Deterministic: the same evidence and decoder version always produce the
//! same records with the same ids.

use std::collections::BTreeMap;

use crate::attribution::attribute;
use crate::balances::{self, Balances};
use crate::decoders::{Ctx, Decoded, Registry, SwapDraft};
use crate::evidence::{self, EvidenceError};
use crate::model::{
    Activity, ActivityKind, Attribution, DECODER_VERSION, DecodeStatus, DecodedTransaction, Swap,
};
use crate::movements::{self, MovementKind};
use crate::normalize::exchange_ratio;
use crate::programs::{
    ASSOCIATED_TOKEN_PROGRAM, SYSTEM_PROGRAM, WSOL_MINT, is_infrastructure, is_token_program,
};
use crate::quality::{Issue, IssueCode};
use crate::reconstruct::{self, ReconstructError, Tx};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error(transparent)]
    Evidence(#[from] EvidenceError),
    #[error(transparent)]
    Reconstruct(#[from] ReconstructError),
}

impl DecodeError {
    #[must_use]
    pub fn issue_code(&self) -> IssueCode {
        match self {
            Self::Evidence(_) => IssueCode::EvidenceParseFailed,
            Self::Reconstruct(ReconstructError::AccountIndex(_)) => {
                IssueCode::AccountIndexOutOfRange
            }
            Self::Reconstruct(ReconstructError::LoadedAddressesMissing) => {
                IssueCode::LoadedAddressesMissing
            }
            Self::Reconstruct(_) => IssueCode::EvidenceParseFailed,
        }
    }
}

const GENERIC_DECODER: &str = "spl_system";
const TX_DECODER: &str = "transaction";

/// Decode raw `getTransaction` JSON.
pub fn decode(raw_json: &str, registry: &Registry) -> Result<DecodedTransaction, DecodeError> {
    let ev = evidence::parse(raw_json)?;
    let tx = reconstruct::reconstruct(ev)?;
    Ok(decode_tx(&tx, registry))
}

fn actor_attribution(tx: &Tx, actor: Option<&str>) -> Option<Attribution> {
    actor.map(|a| {
        if tx.is_signer(a) {
            Attribution::VerifiedAuthority
        } else {
            Attribution::ProgramControlled
        }
    })
}

struct Builder<'a> {
    tx: &'a Tx,
    activities: Vec<Activity>,
    swaps: Vec<Swap>,
    issues: Vec<Issue>,
    protocols: Vec<&'static str>,
}

impl Builder<'_> {
    fn next_index(&self) -> u32 {
        u32::try_from(self.activities.len()).unwrap_or(u32::MAX)
    }

    fn push(&mut self, mut a: Activity) -> u32 {
        a.index = self.next_index();
        let i = a.index;
        self.activities.push(a);
        i
    }

    fn protocol(&mut self, p: &'static str) {
        if !self.protocols.contains(&p) {
            self.protocols.push(p);
        }
    }

    fn blank(&self, kind: ActivityKind, status: DecodeStatus, decoder: &'static str) -> Activity {
        let _ = self.tx;
        Activity {
            index: 0,
            kind,
            status,
            protocol: None,
            program_id: None,
            instruction_ref: None,
            actor: None,
            attribution: None,
            mint: None,
            amount: None,
            source_account: None,
            destination_account: None,
            counterparty: None,
            reasons: Vec::new(),
            decoder,
        }
    }
}

fn finalize_swap(
    b: &mut Builder<'_>,
    cx: &Ctx<'_>,
    ix: usize,
    decoder: &'static str,
    d: SwapDraft,
) {
    let Some(node) = cx.tx.ixs.get(ix) else {
        return;
    };
    let ix_ref = node.id.to_string();
    let SwapDraft {
        protocol,
        venue_kind,
        activity_kind,
        pool,
        authority,
        input,
        output,
        legs,
        fees,
        route_complete,
        mut status,
        mut reasons,
        issues,
    } = d;
    b.issues.extend(issues);
    let att = attribute(
        cx.tx,
        &cx.movements.accounts,
        authority.as_deref(),
        &input,
        &output,
    );
    reasons.extend(att.reasons.iter().map(|r| (*r).to_owned()));
    if !att.attribution.is_verified() {
        b.issues.push(Issue::new(
            IssueCode::AttributionAmbiguous,
            Some(ix_ref.clone()),
            format!("attribution {}", att.attribution.as_str()),
        ));
    }
    if input.mint.is_some() && input.mint == output.mint {
        status = status.weaker(DecodeStatus::Ambiguous);
        reasons.push("SAME_INPUT_AND_OUTPUT_MINT".to_owned());
    }
    if input.amount == Some(0) || output.amount == Some(0) {
        status = status.weaker(DecodeStatus::PartiallyDecoded);
        reasons.push("ZERO_AMOUNT".to_owned());
    }
    let ratio = match (input.amount, input.decimals, output.amount, output.decimals) {
        (Some(ia), Some(id), Some(oa), Some(od)) => exchange_ratio(ia, id, oa, od),
        (Some(_), None, _, _) | (_, _, Some(_), None) => {
            reasons.push("DECIMALS_UNKNOWN".to_owned());
            b.issues.push(Issue::new(
                IssueCode::UnknownMintDecimals,
                Some(ix_ref.clone()),
                "mint decimals not present in transaction metadata",
            ));
            None
        }
        _ => None,
    };
    let aggregator = (venue_kind == crate::model::VenueKind::Aggregator).then_some(protocol);
    let mut activity = b.blank(activity_kind, status, decoder);
    activity.protocol = Some(protocol);
    activity.program_id = Some(node.program.clone());
    activity.instruction_ref = Some(ix_ref.clone());
    activity.actor.clone_from(&att.wallet);
    activity.attribution = Some(att.attribution);
    activity.mint.clone_from(&input.mint);
    activity.amount = input.amount;
    activity.source_account.clone_from(&input.account);
    activity.destination_account.clone_from(&output.account);
    activity.counterparty.clone_from(&pool);
    activity.reasons.clone_from(&reasons);
    let activity_index = b.push(activity);
    b.protocol(protocol);
    b.swaps.push(Swap {
        instruction_ref: ix_ref,
        protocol,
        program_id: node.program.clone(),
        venue_kind,
        aggregator,
        pool,
        wallet: att.wallet,
        attribution: att.attribution,
        authority,
        beneficiary: att.beneficiary,
        input,
        output,
        exchange_ratio: ratio,
        route_complete,
        status,
        reasons,
        legs,
        fees,
        decoder,
        activity_index,
    });
}

/// Decode a reconstructed transaction.
#[must_use]
pub fn decode_tx(tx: &Tx, registry: &Registry) -> DecodedTransaction {
    let bal: Balances = balances::compute(tx);
    let mv = movements::collect(tx, &bal);
    let cx = Ctx {
        tx,
        balances: &bal,
        movements: &mv,
    };
    let mut b = Builder {
        tx,
        activities: Vec::new(),
        swaps: Vec::new(),
        issues: tx.issues.clone(),
        protocols: Vec::new(),
    };
    b.issues.extend(mv.issues.iter().cloned());

    // The network fee is always an activity (failed transactions pay it too).
    let mut fee = b.blank(ActivityKind::Fee, DecodeStatus::Verified, TX_DECODER);
    fee.actor = Some(tx.fee_payer().to_owned());
    fee.attribution = Some(Attribution::VerifiedOwner);
    fee.mint = Some("SOL".to_owned());
    fee.amount = Some(tx.evidence.fee);
    fee.source_account = Some(tx.fee_payer().to_owned());
    b.push(fee);

    if tx.evidence.failed {
        let mut f = b.blank(
            ActivityKind::FailedTransaction,
            DecodeStatus::Verified,
            TX_DECODER,
        );
        f.actor = Some(tx.fee_payer().to_owned());
        f.reasons.push("EXECUTION_FAILED".to_owned());
        b.push(f);
    } else {
        let mut claimed = vec![false; tx.ixs.len()];
        for (i, ix) in tx.ixs.iter().enumerate() {
            if claimed[i] {
                continue;
            }
            if let Some(dec) = registry.for_program(&ix.program) {
                match dec.decode(&cx, i) {
                    Some(Decoded::Swap(d)) => {
                        finalize_swap(&mut b, &cx, i, dec.id(), *d);
                        claimed[i..ix.subtree_end]
                            .iter_mut()
                            .for_each(|c| *c = true);
                    }
                    Some(Decoded::Activity(a)) => {
                        let mut act = b.blank(a.kind, a.status, dec.id());
                        act.protocol = Some(a.protocol);
                        act.program_id = Some(ix.program.clone());
                        act.instruction_ref = Some(ix.id.to_string());
                        act.actor = a.actor;
                        act.attribution = a.attribution;
                        act.reasons = a.reasons;
                        b.push(act);
                        b.protocol(a.protocol);
                        // Movements inside a lifecycle operation stay visible
                        // as transfers; only the instruction itself is claimed.
                        claimed[i] = true;
                    }
                    Some(Decoded::Unsupported { protocol, name }) => {
                        b.protocol(protocol);
                        b.issues.push(Issue::new(
                            IssueCode::UnsupportedInstruction,
                            Some(ix.id.to_string()),
                            format!("{protocol} {name} not decoded"),
                        ));
                        if ix.parent.is_none() {
                            let mut act = b.blank(
                                ActivityKind::UnknownActivity,
                                DecodeStatus::Unsupported,
                                dec.id(),
                            );
                            act.protocol = Some(protocol);
                            act.program_id = Some(ix.program.clone());
                            act.instruction_ref = Some(ix.id.to_string());
                            act.reasons.push("UNSUPPORTED_INSTRUCTION".to_owned());
                            b.push(act);
                        }
                    }
                    Some(Decoded::Ignore) | None => {}
                }
                continue;
            }
            let generic = ix.program == SYSTEM_PROGRAM
                || is_token_program(&ix.program)
                || ix.program == ASSOCIATED_TOKEN_PROGRAM;
            if ix.parent.is_none() && !generic && !is_infrastructure(&ix.program) {
                b.issues.push(Issue::new(
                    IssueCode::UnsupportedProgram,
                    Some(ix.id.to_string()),
                    format!("program {} has no decoder", ix.program),
                ));
                let mut act = b.blank(
                    ActivityKind::UnknownActivity,
                    DecodeStatus::Unsupported,
                    GENERIC_DECODER,
                );
                act.program_id = Some(ix.program.clone());
                act.instruction_ref = Some(ix.id.to_string());
                act.reasons.push("UNSUPPORTED_PROGRAM".to_owned());
                b.push(act);
            }
        }

        // Value movements not consumed by a decoded swap.
        for m in &mv.items {
            // Movements inside a decoded swap are that swap's legs.
            if claimed.get(m.ix).copied().unwrap_or(false) {
                continue;
            }
            let Some(node) = tx.ixs.get(m.ix) else {
                continue;
            };
            let wsol_dest =
                m.to.as_deref()
                    .is_some_and(|t| mv.accounts.mint(t) == Some(WSOL_MINT));
            let (kind, mint, amount) = match m.kind {
                MovementKind::SolTransfer if wsol_dest => (
                    ActivityKind::WrapSol,
                    Some("SOL".to_owned()),
                    Some(m.amount),
                ),
                MovementKind::SolTransfer => (
                    ActivityKind::NativeTransfer,
                    Some("SOL".to_owned()),
                    Some(m.amount),
                ),
                MovementKind::SolCreateAccount => (
                    ActivityKind::AccountCreation,
                    Some("SOL".to_owned()),
                    Some(m.amount),
                ),
                MovementKind::TokenTransfer => {
                    (ActivityKind::TokenTransfer, m.mint.clone(), Some(m.amount))
                }
                MovementKind::TokenMint => {
                    (ActivityKind::TokenMint, m.mint.clone(), Some(m.amount))
                }
                MovementKind::TokenBurn => {
                    (ActivityKind::TokenBurn, m.mint.clone(), Some(m.amount))
                }
                MovementKind::TokenAccountClose => {
                    let lamports = m
                        .from
                        .as_deref()
                        .and_then(|a| tx.account_index(a))
                        .and_then(|k| tx.evidence.pre_balances.get(k).copied());
                    if m.mint.as_deref() == Some(WSOL_MINT) {
                        (ActivityKind::UnwrapSol, Some("SOL".to_owned()), lamports)
                    } else {
                        (
                            ActivityKind::AccountClosure,
                            Some("SOL".to_owned()),
                            lamports,
                        )
                    }
                }
                MovementKind::WsolSync => continue,
            };
            let status = if mint.is_some() {
                DecodeStatus::Verified
            } else {
                DecodeStatus::PartiallyDecoded
            };
            let mut a = b.blank(kind, status, GENERIC_DECODER);
            a.program_id = Some(m.program.clone());
            a.instruction_ref = Some(node.id.to_string());
            a.actor.clone_from(&m.authority);
            a.attribution = actor_attribution(tx, m.authority.as_deref());
            a.mint = mint;
            a.amount = amount;
            a.source_account.clone_from(&m.from);
            a.destination_account.clone_from(&m.to);
            if let Some(f) = m.transfer_fee {
                a.reasons.push(format!("TOKEN_2022_TRANSFER_FEE_{f}"));
            }
            if status != DecodeStatus::Verified {
                a.reasons.push("MINT_UNKNOWN".to_owned());
            }
            b.push(a);
        }
    }

    // Overall status: the weakest decoded activity, with unsupported
    // programs lowering a decoded transaction to partially decoded.
    let mut status = DecodeStatus::Verified;
    let mut unknown = false;
    let mut decoded_other = false;
    for a in &b.activities {
        match a.kind {
            ActivityKind::Fee | ActivityKind::FailedTransaction => {}
            ActivityKind::UnknownActivity => unknown = true,
            _ => {
                decoded_other = true;
                status = status.weaker(a.status);
            }
        }
    }
    if unknown {
        status = status.weaker(if decoded_other {
            DecodeStatus::PartiallyDecoded
        } else {
            DecodeStatus::Unsupported
        });
    }
    if !tx.inner_complete && !tx.evidence.failed {
        status = status.weaker(DecodeStatus::InsufficientEvidence);
    }
    if tx
        .issues
        .iter()
        .any(|i| i.code == IssueCode::BalanceLengthMismatch)
    {
        status = DecodeStatus::Invalid;
    }

    let mut ownership = BTreeMap::new();
    for c in &bal.native {
        ownership.insert(c.account.clone(), "NATIVE_ACCOUNT");
    }
    for c in &bal.token {
        let src = if c.owner.is_some() {
            "TOKEN_BALANCE_META"
        } else if mv.accounts.owner(&c.account).is_some() {
            "INITIALIZE_ACCOUNT"
        } else {
            "UNKNOWN"
        };
        ownership.insert(c.account.clone(), src);
    }
    let mut protocols = b.protocols;
    protocols.sort_unstable();
    b.issues
        .sort_by(|x, y| (x.code, &x.instruction_ref).cmp(&(y.code, &y.instruction_ref)));
    b.issues
        .dedup_by(|x, y| x.code == y.code && x.instruction_ref == y.instruction_ref);
    DecodedTransaction {
        signature: tx.signature.clone(),
        slot: tx.evidence.slot,
        block_time: tx.evidence.block_time,
        transaction_index: tx.evidence.transaction_index,
        version: tx.evidence.version.as_str(),
        failed: tx.evidence.failed,
        err_json: tx.evidence.err_json.clone(),
        fee: tx.evidence.fee,
        fee_payer: tx.fee_payer().to_owned(),
        signers: tx.signers().into_iter().map(str::to_owned).collect(),
        compute_units: tx.evidence.compute_units_consumed,
        balances: bal,
        ownership,
        activities: b.activities,
        swaps: b.swaps,
        issues: b.issues,
        status,
        protocols,
        decoder_version: DECODER_VERSION,
    }
}
