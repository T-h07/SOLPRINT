//! Value movements decoded from System, SPL Token and Token-2022
//! instructions (top level or CPI), with token accounts resolved to their
//! mint and owner from the transaction's own evidence.

use std::collections::BTreeMap;

use crate::balances::Balances;
use crate::borsh::Reader;
use crate::programs::{
    ASSOCIATED_TOKEN_PROGRAM, SYSTEM_PROGRAM, TOKEN_2022_PROGRAM, WSOL_MINT, is_token_program,
};
use crate::quality::{Issue, IssueCode};
use crate::reconstruct::{Ix, Tx};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementKind {
    SolTransfer,
    SolCreateAccount,
    TokenTransfer,
    TokenMint,
    TokenBurn,
    TokenAccountClose,
    WsolSync,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movement {
    /// Index into `Tx::ixs`.
    pub ix: usize,
    pub kind: MovementKind,
    /// Source account (system account for SOL, token account for tokens).
    pub from: Option<String>,
    pub to: Option<String>,
    pub authority: Option<String>,
    pub mint: Option<String>,
    pub amount: u64,
    /// Token-2022 transfer fee stated by `TransferCheckedWithFee`.
    pub transfer_fee: Option<u64>,
    pub decimals: Option<u8>,
    pub program: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenAccountInfo {
    pub mint: Option<String>,
    pub owner: Option<String>,
    pub program: Option<String>,
}

/// Token account → (mint, owner) knowledge collected from balances metadata
/// and from account-initialisation / ATA-creation instructions.
#[derive(Debug, Clone, Default)]
pub struct TokenAccounts(pub BTreeMap<String, TokenAccountInfo>);

impl TokenAccounts {
    #[must_use]
    pub fn mint(&self, account: &str) -> Option<&str> {
        self.0.get(account).and_then(|i| i.mint.as_deref())
    }

    #[must_use]
    pub fn owner(&self, account: &str) -> Option<&str> {
        self.0.get(account).and_then(|i| i.owner.as_deref())
    }

    fn learn(
        &mut self,
        account: &str,
        mint: Option<&str>,
        owner: Option<&str>,
        program: Option<&str>,
    ) {
        let e = self.0.entry(account.to_owned()).or_default();
        if e.mint.is_none() {
            e.mint = mint.map(str::to_owned);
        }
        if e.owner.is_none() {
            e.owner = owner.map(str::to_owned);
        }
        if e.program.is_none() {
            e.program = program.map(str::to_owned);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Movements {
    pub items: Vec<Movement>,
    pub accounts: TokenAccounts,
    pub issues: Vec<Issue>,
}

impl Movements {
    /// Movements produced inside `tx.ixs[root]`'s subtree (excluding the
    /// root itself unless `include_root`).
    pub fn within<'a>(
        &'a self,
        tx: &'a Tx,
        root: usize,
        include_root: bool,
    ) -> impl Iterator<Item = &'a Movement> + 'a {
        let range = tx.descendants(root);
        self.items
            .iter()
            .filter(move |m| range.contains(&m.ix) || (include_root && m.ix == root))
    }
}

fn system(ix: &Ix, idx: usize) -> Option<Movement> {
    let mut r = Reader::new(&ix.data);
    let tag = r.u32()?;
    let mk = |kind, from: Option<&str>, to: Option<&str>, amount| Movement {
        ix: idx,
        kind,
        from: from.map(str::to_owned),
        to: to.map(str::to_owned),
        authority: from.map(str::to_owned),
        mint: Some(WSOL_MINT.to_owned()),
        amount,
        transfer_fee: None,
        decimals: Some(9),
        program: SYSTEM_PROGRAM.to_owned(),
    };
    match tag {
        // CreateAccount { lamports, space, owner }
        0 => Some(mk(
            MovementKind::SolCreateAccount,
            ix.account(0),
            ix.account(1),
            r.u64()?,
        )),
        // Transfer { lamports }
        2 => Some(mk(
            MovementKind::SolTransfer,
            ix.account(0),
            ix.account(1),
            r.u64()?,
        )),
        // CreateAccountWithSeed { base, seed: String, lamports, space, owner }
        3 => {
            r.skip(32)?;
            let seed_len = usize::try_from(r.u64()?).ok()?;
            r.skip(seed_len)?;
            Some(mk(
                MovementKind::SolCreateAccount,
                ix.account(0),
                ix.account(1),
                r.u64()?,
            ))
        }
        // TransferWithSeed { lamports, seed, owner }: [from, base, to]
        11 => Some(mk(
            MovementKind::SolTransfer,
            ix.account(0),
            ix.account(2),
            r.u64()?,
        )),
        _ => None,
    }
}

/// Decode one token-program instruction. Unsupported extension
/// instructions that can move value are reported as issues.
fn token(
    ix: &Ix,
    idx: usize,
    accts: &mut TokenAccounts,
    issues: &mut Vec<Issue>,
) -> Option<Movement> {
    let mut r = Reader::new(&ix.data);
    let tag = r.u8()?;
    let prog = ix.program.as_str();
    let mv = |kind,
              from: Option<&str>,
              to: Option<&str>,
              auth: Option<&str>,
              mint: Option<&str>,
              amount,
              decimals| Movement {
        ix: idx,
        kind,
        from: from.map(str::to_owned),
        to: to.map(str::to_owned),
        authority: auth.map(str::to_owned),
        mint: mint.map(str::to_owned),
        amount,
        transfer_fee: None,
        decimals,
        program: prog.to_owned(),
    };
    match tag {
        // InitializeAccount [account, mint, owner, rent]
        1 => {
            accts.learn(ix.account(0)?, ix.account(1), ix.account(2), Some(prog));
            None
        }
        // Transfer { amount } [src, dst, authority]
        3 => {
            let amount = r.u64()?;
            let (src, dst) = (ix.account(0)?, ix.account(1)?);
            let mint = accts
                .mint(src)
                .or_else(|| accts.mint(dst))
                .map(str::to_owned);
            Some(mv(
                MovementKind::TokenTransfer,
                Some(src),
                Some(dst),
                ix.account(2),
                mint.as_deref(),
                amount,
                None,
            ))
        }
        // MintTo { amount } [mint, account, authority]
        7 => Some(mv(
            MovementKind::TokenMint,
            None,
            ix.account(1),
            ix.account(2),
            ix.account(0),
            r.u64()?,
            None,
        )),
        // Burn { amount } [account, mint, authority]
        8 => Some(mv(
            MovementKind::TokenBurn,
            ix.account(0),
            None,
            ix.account(2),
            ix.account(1),
            r.u64()?,
            None,
        )),
        // CloseAccount [account, destination, owner]: lamports returned are
        // visible in native balances; amount here is informational 0.
        9 => Some(mv(
            MovementKind::TokenAccountClose,
            ix.account(0),
            ix.account(1),
            ix.account(2),
            accts.mint(ix.account(0)?).map(str::to_owned).as_deref(),
            0,
            None,
        )),
        // TransferChecked { amount, decimals } [src, mint, dst, authority]
        12 => {
            let amount = r.u64()?;
            let decimals = r.u8()?;
            let (src, mint, dst) = (ix.account(0)?, ix.account(1)?, ix.account(2)?);
            accts.learn(src, Some(mint), None, Some(prog));
            accts.learn(dst, Some(mint), None, Some(prog));
            Some(mv(
                MovementKind::TokenTransfer,
                Some(src),
                Some(dst),
                ix.account(3),
                Some(mint),
                amount,
                Some(decimals),
            ))
        }
        // MintToChecked [mint, account, authority]
        14 => {
            let amount = r.u64()?;
            let d = r.u8()?;
            Some(mv(
                MovementKind::TokenMint,
                None,
                ix.account(1),
                ix.account(2),
                ix.account(0),
                amount,
                Some(d),
            ))
        }
        // BurnChecked [account, mint, authority]
        15 => {
            let amount = r.u64()?;
            let d = r.u8()?;
            Some(mv(
                MovementKind::TokenBurn,
                ix.account(0),
                None,
                ix.account(2),
                ix.account(1),
                amount,
                Some(d),
            ))
        }
        // InitializeAccount2 { owner } [account, mint, rent]
        // InitializeAccount3 { owner } [account, mint]
        16 | 18 => {
            let owner = r.pubkey()?;
            accts.learn(ix.account(0)?, ix.account(1), Some(&owner), Some(prog));
            None
        }
        // SyncNative [account]
        17 => Some(mv(
            MovementKind::WsolSync,
            None,
            ix.account(0),
            None,
            Some(WSOL_MINT),
            0,
            Some(9),
        )),
        // Token-2022 TransferFeeExtension
        26 if prog == TOKEN_2022_PROGRAM => match r.u8()? {
            // TransferCheckedWithFee { amount, decimals, fee } [src, mint, dst, authority]
            1 => {
                let amount = r.u64()?;
                let decimals = r.u8()?;
                let fee = r.u64()?;
                let (src, mint, dst) = (ix.account(0)?, ix.account(1)?, ix.account(2)?);
                accts.learn(src, Some(mint), None, Some(prog));
                accts.learn(dst, Some(mint), None, Some(prog));
                let mut m = mv(
                    MovementKind::TokenTransfer,
                    Some(src),
                    Some(dst),
                    ix.account(3),
                    Some(mint),
                    amount,
                    Some(decimals),
                );
                m.transfer_fee = Some(fee);
                Some(m)
            }
            _ => None,
        },
        // Confidential transfers / mint-burn hide amounts; UnwrapLamports
        // and PermissionedBurn move value through paths this decoder does
        // not model. All are flagged, never guessed.
        27 | 37 | 42 | 45 | 46 if prog == TOKEN_2022_PROGRAM => {
            issues.push(Issue::new(
                IssueCode::UnsupportedTokenExtension,
                Some(ix.id.to_string()),
                format!("Token-2022 instruction tag {tag} is not decoded"),
            ));
            None
        }
        // Batch executes nested token instructions encoded in its data.
        255 => {
            issues.push(Issue::new(
                IssueCode::UnsupportedInstruction,
                Some(ix.id.to_string()),
                "token Batch instruction is not decoded; nested movements unknown",
            ));
            None
        }
        _ => None,
    }
}

/// Decode every movement in execution order.
#[must_use]
pub fn collect(tx: &Tx, balances: &Balances) -> Movements {
    let mut accts = TokenAccounts::default();
    for c in &balances.token {
        accts.learn(
            &c.account,
            Some(&c.mint),
            c.owner.as_deref(),
            c.program_id.as_deref(),
        );
    }
    // ATA create / createIdempotent: [payer, ata, wallet, mint, system, token_program]
    for ix in &tx.ixs {
        if ix.program == ASSOCIATED_TOKEN_PROGRAM
            && matches!(ix.data.first(), None | Some(0 | 1))
            && let Some(ata) = ix.account(1)
        {
            accts.learn(ata, ix.account(3), ix.account(2), ix.account(5));
        }
    }
    let mut items = Vec::new();
    let mut issues = Vec::new();
    for (i, ix) in tx.ixs.iter().enumerate() {
        let m = if ix.program == SYSTEM_PROGRAM {
            system(ix, i)
        } else if is_token_program(&ix.program) {
            token(ix, i, &mut accts, &mut issues)
        } else {
            None
        };
        items.extend(m);
    }
    // Resolve mints of plain `Transfer`s learned after the instruction.
    for m in &mut items {
        if m.mint.is_none() && m.kind == MovementKind::TokenTransfer {
            m.mint = m
                .from
                .as_deref()
                .and_then(|a| accts.mint(a))
                .or_else(|| m.to.as_deref().and_then(|a| accts.mint(a)))
                .map(str::to_owned);
        }
    }
    Movements {
        items,
        accounts: accts,
        issues,
    }
}
