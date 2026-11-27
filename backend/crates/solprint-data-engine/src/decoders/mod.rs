//! Protocol decoder registry and the shared evidence helpers decoders use.
//!
//! A decoder recognises instructions of exactly one program by discriminator
//! and account position (both verified against official sources; see
//! `docs/architecture/DECODER_CAPABILITIES.md`). It never guesses: amounts
//! come from token/system movements executed inside the instruction, from an
//! authentic self-CPI program event, or are reported missing.

pub mod jupiter;
pub mod orca;
pub mod pump;
pub mod pumpswap;
pub mod raydium;

use crate::balances::Balances;
use crate::model::{
    ActivityKind, AssetKind, Attribution, DecodeStatus, RouteLeg, SwapFee, SwapSide, VenueKind,
};
use crate::movements::{MovementKind, Movements};
use crate::programs::{TOKEN_2022_PROGRAM, WSOL_MINT};
use crate::quality::{Issue, IssueCode};
use crate::reconstruct::Tx;

/// Read-only decoding context for one transaction.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    pub tx: &'a Tx,
    pub balances: &'a Balances,
    pub movements: &'a Movements,
}

/// A swap as a decoder sees it, before attribution and normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapDraft {
    pub protocol: &'static str,
    pub venue_kind: VenueKind,
    pub activity_kind: ActivityKind,
    pub pool: Option<String>,
    /// The account the program requires as transfer authority / user.
    pub authority: Option<String>,
    pub input: SwapSide,
    pub output: SwapSide,
    pub legs: Vec<RouteLeg>,
    pub fees: Vec<SwapFee>,
    pub route_complete: bool,
    pub status: DecodeStatus,
    pub reasons: Vec<String>,
    pub issues: Vec<Issue>,
}

/// A non-swap protocol activity (liquidity, pool creation, migration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDraft {
    pub kind: ActivityKind,
    pub protocol: &'static str,
    pub actor: Option<String>,
    pub attribution: Option<Attribution>,
    pub status: DecodeStatus,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    Swap(Box<SwapDraft>),
    Activity(ActivityDraft),
    /// A known program instruction this decoder does not model.
    Unsupported {
        protocol: &'static str,
        name: &'static str,
    },
    /// Not an economic instruction (e.g. a program's own event CPI).
    Ignore,
}

pub trait ProtocolDecoder: Send + Sync + std::fmt::Debug {
    /// Stable lowercase id stored with every record (`^[a-z0-9_]{1,32}$`).
    fn id(&self) -> &'static str;
    fn protocol(&self) -> &'static str;
    fn program_id(&self) -> &'static str;
    /// `None` when the instruction is not recognised at all.
    fn decode(&self, cx: &Ctx<'_>, ix: usize) -> Option<Decoded>;
}

/// The registered decoders. Program ids are unique within the registry.
#[derive(Debug)]
pub struct Registry {
    decoders: Vec<Box<dyn ProtocolDecoder>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::standard()
    }
}

impl Registry {
    #[must_use]
    pub fn standard() -> Self {
        Self {
            decoders: vec![
                Box::new(jupiter::JupiterV6),
                Box::new(pump::PumpFun),
                Box::new(pumpswap::PumpSwap),
                Box::new(raydium::AmmV4),
                Box::new(raydium::Cpmm),
                Box::new(raydium::Clmm),
                Box::new(orca::Whirlpool),
            ],
        }
    }

    #[must_use]
    pub fn for_program(&self, program: &str) -> Option<&dyn ProtocolDecoder> {
        self.decoders
            .iter()
            .find(|d| d.program_id() == program)
            .map(AsRef::as_ref)
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn ProtocolDecoder> {
        self.decoders.iter().map(AsRef::as_ref)
    }
}

// ---- shared helpers ----------------------------------------------------------

/// Sum of token transferred *out of* `account` inside `root`'s subtree.
/// `None` when no such transfer exists.
#[must_use]
pub fn sent_from(cx: &Ctx<'_>, root: usize, account: &str) -> Option<u64> {
    sum(cx, root, |m| {
        m.kind == MovementKind::TokenTransfer && m.from.as_deref() == Some(account)
    })
}

/// Sum of token transferred *into* `account` inside `root`'s subtree, net of
/// any explicit Token-2022 transfer fee.
#[must_use]
pub fn received_by(cx: &Ctx<'_>, root: usize, account: &str) -> Option<u64> {
    let mut total: Option<u64> = None;
    for m in cx.movements.within(cx.tx, root, false) {
        if m.kind == MovementKind::TokenTransfer && m.to.as_deref() == Some(account) {
            let net = m.amount.checked_sub(m.transfer_fee.unwrap_or(0))?;
            total = Some(total.unwrap_or(0).checked_add(net)?);
        }
    }
    total
}

/// Native lamports sent by `wallet` via System transfers inside the subtree.
#[must_use]
pub fn lamports_from(cx: &Ctx<'_>, root: usize, wallet: &str) -> Option<u64> {
    sum(cx, root, |m| {
        m.kind == MovementKind::SolTransfer && m.from.as_deref() == Some(wallet)
    })
}

fn sum(
    cx: &Ctx<'_>,
    root: usize,
    pred: impl Fn(&crate::movements::Movement) -> bool,
) -> Option<u64> {
    let mut total: Option<u64> = None;
    for m in cx.movements.within(cx.tx, root, false) {
        if pred(m) {
            total = Some(total.unwrap_or(0).checked_add(m.amount)?);
        }
    }
    total
}

/// Movements touching `account` outside `root`'s subtree.
#[must_use]
pub fn touched_outside(cx: &Ctx<'_>, root: usize, account: &str) -> bool {
    let range = cx.tx.descendants(root);
    cx.movements.items.iter().any(|m| {
        !range.contains(&m.ix)
            && (m.from.as_deref() == Some(account) || m.to.as_deref() == Some(account))
    })
}

/// Mint of a token account, from balances metadata or initialisation.
#[must_use]
pub fn mint_of(cx: &Ctx<'_>, account: &str) -> Option<String> {
    cx.movements
        .accounts
        .mint(account)
        .or_else(|| cx.balances.mint_of(account))
        .map(str::to_owned)
}

#[must_use]
pub fn owner_of(cx: &Ctx<'_>, account: &str) -> Option<String> {
    cx.movements.accounts.owner(account).map(str::to_owned)
}

fn closed_in_tx(cx: &Ctx<'_>, account: &str) -> bool {
    cx.movements
        .items
        .iter()
        .any(|m| m.kind == MovementKind::TokenAccountClose && m.from.as_deref() == Some(account))
}

/// Asset kind of a side: WSOL in an account the transaction closes (or a
/// native lamport side) is native SOL; otherwise by token program.
#[must_use]
pub fn asset_kind(cx: &Ctx<'_>, mint: Option<&str>, account: Option<&str>) -> AssetKind {
    let Some(mint) = mint else {
        return AssetKind::Unknown;
    };
    if mint == WSOL_MINT {
        return match account {
            Some(a) if closed_in_tx(cx, a) || cx.balances.token_account(a).is_none() => {
                AssetKind::NativeSol
            }
            _ => AssetKind::WrappedSol,
        };
    }
    let program = account
        .and_then(|a| cx.movements.accounts.0.get(a))
        .and_then(|i| i.program.clone())
        .or_else(|| {
            cx.balances
                .token
                .iter()
                .find(|c| c.mint == mint)
                .and_then(|c| c.program_id.clone())
        });
    match program.as_deref() {
        Some(TOKEN_2022_PROGRAM) => AssetKind::Token2022,
        Some(_) => AssetKind::SplToken,
        None => AssetKind::Unknown,
    }
}

#[must_use]
pub fn decimals(cx: &Ctx<'_>, mint: Option<&str>) -> Option<u8> {
    let mint = mint?;
    cx.balances.decimals_of(mint).or_else(|| {
        cx.movements
            .items
            .iter()
            .find(|m| m.mint.as_deref() == Some(mint) && m.decimals.is_some())
            .and_then(|m| m.decimals)
    })
}

/// Build a side from a user token account and an observed amount.
#[must_use]
pub fn side(
    cx: &Ctx<'_>,
    account: Option<&str>,
    mint: Option<String>,
    amount: Option<u64>,
) -> SwapSide {
    let mint = mint.or_else(|| account.and_then(|a| mint_of(cx, a)));
    SwapSide {
        asset_kind: asset_kind(cx, mint.as_deref(), account),
        decimals: decimals(cx, mint.as_deref()),
        mint,
        amount,
        account: account.map(str::to_owned),
    }
}

/// Cross-check a token-account-derived amount against that account's
/// balance delta, when the account is touched only by this instruction.
/// Returns an issue on mismatch.
#[must_use]
pub fn cross_check(
    cx: &Ctx<'_>,
    root: usize,
    account: &str,
    expected_delta: i128,
) -> Option<Issue> {
    if touched_outside(cx, root, account) {
        return None;
    }
    let change = cx.balances.token_account(account)?;
    if change.delta == expected_delta {
        return None;
    }
    Some(Issue::new(
        IssueCode::DecoderAmountMismatch,
        Some(
            cx.tx
                .ixs
                .get(root)
                .map(|i| i.id.to_string())
                .unwrap_or_default(),
        ),
        format!(
            "account balance delta {} does not match decoded {}",
            change.delta, expected_delta
        ),
    ))
}

/// Standard direct-DEX swap draft from the user's source and destination
/// token accounts named by the instruction layout.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn direct_swap(
    cx: &Ctx<'_>,
    root: usize,
    protocol: &'static str,
    venue_kind: VenueKind,
    pool: Option<&str>,
    authority: Option<&str>,
    src: Option<&str>,
    dst: Option<&str>,
    mints: (Option<String>, Option<String>),
) -> SwapDraft {
    let input_amount = src.and_then(|a| sent_from(cx, root, a));
    let output_amount = dst.and_then(|a| received_by(cx, root, a));
    let mut issues = Vec::new();
    let mut reasons = Vec::new();
    let mut status = DecodeStatus::Verified;
    let ix_ref = cx.tx.ixs.get(root).map(|i| i.id.to_string());
    if !cx.tx.inner_complete {
        status = DecodeStatus::InsufficientEvidence;
        reasons.push("INNER_INSTRUCTIONS_MISSING".to_owned());
    } else if input_amount.is_none() || output_amount.is_none() {
        status = DecodeStatus::PartiallyDecoded;
        reasons.push("USER_TRANSFER_NOT_FOUND".to_owned());
        issues.push(Issue::new(
            IssueCode::DecoderAmountMismatch,
            ix_ref.clone(),
            "no token transfer found for the user's source or destination account",
        ));
    }
    if let (Some(a), Some(v)) = (src, input_amount)
        && let Some(i) = cross_check(cx, root, a, -i128::from(v))
    {
        status = status.weaker(DecodeStatus::PartiallyDecoded);
        reasons.push("SOURCE_DELTA_MISMATCH".to_owned());
        issues.push(i);
    }
    if let (Some(a), Some(v)) = (dst, output_amount)
        && let Some(i) = cross_check(cx, root, a, i128::from(v))
    {
        status = status.weaker(DecodeStatus::PartiallyDecoded);
        reasons.push("DESTINATION_DELTA_MISMATCH".to_owned());
        issues.push(i);
    }
    let input = side(cx, src, mints.0, input_amount);
    let output = side(cx, dst, mints.1, output_amount);
    let legs = match (&input.mint, input.amount, &output.mint, output.amount) {
        (Some(im), Some(ia), Some(om), Some(oa)) => vec![RouteLeg {
            protocol: protocol.to_owned(),
            program_id: cx.tx.ixs.get(root).map(|i| i.program.clone()),
            pool: pool.map(str::to_owned),
            input_mint: im.clone(),
            input_amount: ia,
            output_mint: om.clone(),
            output_amount: oa,
            instruction_ref: ix_ref,
            evidence: crate::model::LegEvidence::TokenTransfers,
        }],
        _ => Vec::new(),
    };
    SwapDraft {
        protocol,
        venue_kind,
        activity_kind: ActivityKind::Swap,
        pool: pool.map(str::to_owned),
        authority: authority.map(str::to_owned),
        route_complete: !legs.is_empty(),
        input,
        output,
        legs,
        fees: Vec::new(),
        status,
        reasons,
        issues,
    }
}

/// A liquidity / pool lifecycle activity attributed to the signer at
/// `actor_index` of the instruction (when it signed the transaction).
#[must_use]
pub fn lifecycle(
    cx: &Ctx<'_>,
    root: usize,
    kind: ActivityKind,
    protocol: &'static str,
    actor_index: usize,
) -> Decoded {
    let actor = cx
        .tx
        .ixs
        .get(root)
        .and_then(|i| i.account(actor_index))
        .map(str::to_owned);
    let attribution = actor.as_deref().map(|a| {
        if cx.tx.is_signer(a) {
            Attribution::VerifiedAuthority
        } else {
            Attribution::ProgramControlled
        }
    });
    Decoded::Activity(ActivityDraft {
        kind,
        protocol,
        actor,
        attribution,
        // Amounts of liquidity operations are not normalized in Phase 02.
        status: DecodeStatus::PartiallyDecoded,
        reasons: vec!["AMOUNTS_NOT_NORMALIZED".to_owned()],
    })
}

/// Lifecycle activity attributed to the first transaction signer among the
/// instruction's accounts (for layouts without a fixed user position).
#[must_use]
pub fn lifecycle_by_signer(
    cx: &Ctx<'_>,
    root: usize,
    kind: ActivityKind,
    protocol: &'static str,
) -> Decoded {
    let actor = cx
        .tx
        .ixs
        .get(root)
        .and_then(|i| i.accounts.iter().find(|k| cx.tx.is_signer(k)))
        .cloned();
    Decoded::Activity(ActivityDraft {
        kind,
        protocol,
        attribution: Some(if actor.is_some() {
            Attribution::VerifiedAuthority
        } else {
            Attribution::UnknownOwner
        }),
        actor,
        status: DecodeStatus::PartiallyDecoded,
        reasons: vec!["AMOUNTS_NOT_NORMALIZED".to_owned()],
    })
}
