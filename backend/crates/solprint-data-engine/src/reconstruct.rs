//! Transaction reconstruction: the complete account list (static keys plus
//! address-lookup-table loaded addresses, in runtime order), signer and
//! writable flags from the message header, and the executed instruction tree
//! (top-level instructions and their CPIs, ordered as executed, with parent
//! links derived from `stackHeight`).
//!
//! See `docs/architecture/TRANSACTION_RECONSTRUCTION.md`.

use std::fmt;

use crate::evidence::{RawInstruction, TxEvidence, TxVersion};
use crate::programs::ANCHOR_EVENT_IX_TAG_LE;
use crate::quality::{Issue, IssueCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountSource {
    Static,
    LookupWritable,
    LookupReadonly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub key: String,
    pub signer: bool,
    pub writable: bool,
    pub source: AccountSource,
}

/// Position of an instruction: top-level index, and the index inside that
/// instruction's inner list for CPIs. Displays as `"3"` or `"3.2"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IxRef {
    pub outer: u16,
    pub inner: Option<u16>,
}

impl fmt::Display for IxRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner {
            None => write!(f, "{}", self.outer),
            Some(i) => write!(f, "{}.{i}", self.outer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ix {
    pub id: IxRef,
    pub program: String,
    /// Resolved account keys, in instruction order.
    pub accounts: Vec<String>,
    pub data: Vec<u8>,
    /// 1 for top level; CPI depth + 1 below.
    pub stack_height: u32,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Index one past this instruction's last descendant in `Tx::ixs`.
    pub subtree_end: usize,
}

impl Ix {
    #[must_use]
    pub fn account(&self, i: usize) -> Option<&str> {
        self.accounts.get(i).map(String::as_str)
    }

    #[must_use]
    pub fn disc(&self) -> Option<[u8; 8]> {
        let d = self.data.get(..8)?;
        let mut out = [0_u8; 8];
        out.copy_from_slice(d);
        Some(out)
    }

    /// An Anchor `emit_cpi!` self-invocation carrying an event payload.
    #[must_use]
    pub fn is_anchor_event(&self) -> bool {
        self.data.get(..8) == Some(&ANCHOR_EVENT_IX_TAG_LE[..])
    }

    /// Event discriminator and body of an Anchor event CPI.
    #[must_use]
    pub fn anchor_event(&self) -> Option<([u8; 8], &[u8])> {
        if !self.is_anchor_event() {
            return None;
        }
        let d = self.data.get(8..16)?;
        let mut disc = [0_u8; 8];
        disc.copy_from_slice(d);
        Some((disc, self.data.get(16..).unwrap_or(&[])))
    }
}

/// A reconstructed transaction: evidence plus resolved structure.
#[derive(Debug, Clone)]
pub struct Tx {
    pub evidence: TxEvidence,
    pub signature: String,
    pub accounts: Vec<Account>,
    /// Instructions in execution order (pre-order: each instruction is
    /// followed by its descendants).
    pub ixs: Vec<Ix>,
    pub top_level: Vec<usize>,
    pub issues: Vec<Issue>,
    /// `false` when inner instructions were not recorded: CPI-derived facts
    /// are then unknown, not absent.
    pub inner_complete: bool,
}

impl Tx {
    #[must_use]
    pub fn fee_payer(&self) -> &str {
        self.accounts.first().map_or("", |a| a.key.as_str())
    }

    #[must_use]
    pub fn is_signer(&self, key: &str) -> bool {
        self.accounts.iter().any(|a| a.signer && a.key == key)
    }

    #[must_use]
    pub fn account_index(&self, key: &str) -> Option<usize> {
        self.accounts.iter().position(|a| a.key == key)
    }

    #[must_use]
    pub fn signers(&self) -> Vec<&str> {
        self.accounts
            .iter()
            .filter(|a| a.signer)
            .map(|a| a.key.as_str())
            .collect()
    }

    /// Indices of all descendants of `ix` (excluding itself).
    #[must_use]
    pub fn descendants(&self, ix: usize) -> std::ops::Range<usize> {
        let end = self.ixs.get(ix).map_or(ix, |i| i.subtree_end);
        (ix + 1).min(end)..end
    }

    /// Whether `node` lies inside `ancestor`'s subtree.
    #[must_use]
    pub fn is_descendant(&self, node: usize, ancestor: usize) -> bool {
        self.descendants(ancestor).contains(&node)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReconstructError {
    #[error("account index {0} out of range")]
    AccountIndex(usize),
    #[error("v0 transaction references lookup tables but no loaded addresses were returned")]
    LoadedAddressesMissing,
    #[error("message header is inconsistent with the account keys")]
    Header,
    #[error("transaction has no signature")]
    NoSignature,
    #[error("instruction index does not fit the reference format")]
    TooManyInstructions,
}

fn build_accounts(e: &TxEvidence) -> Result<Vec<Account>, ReconstructError> {
    let n = e.static_keys.len();
    if e.num_required_signatures > n
        || e.num_readonly_signed > e.num_required_signatures
        || e.num_readonly_unsigned > n - e.num_required_signatures
    {
        return Err(ReconstructError::Header);
    }
    let writable_signed = e.num_required_signatures - e.num_readonly_signed;
    let writable_unsigned_end = n - e.num_readonly_unsigned;
    let mut out = Vec::with_capacity(n + e.loaded_writable.len() + e.loaded_readonly.len());
    for (i, key) in e.static_keys.iter().enumerate() {
        let signer = i < e.num_required_signatures;
        let writable = if signer {
            i < writable_signed
        } else {
            i < writable_unsigned_end
        };
        out.push(Account {
            key: key.clone(),
            signer,
            writable,
            source: AccountSource::Static,
        });
    }
    for key in &e.loaded_writable {
        out.push(Account {
            key: key.clone(),
            signer: false,
            writable: true,
            source: AccountSource::LookupWritable,
        });
    }
    for key in &e.loaded_readonly {
        out.push(Account {
            key: key.clone(),
            signer: false,
            writable: false,
            source: AccountSource::LookupReadonly,
        });
    }
    Ok(out)
}

fn resolve(
    accounts: &[Account],
    raw: &RawInstruction,
) -> Result<(String, Vec<String>), ReconstructError> {
    let key = |i: usize| {
        accounts
            .get(i)
            .map(|a| a.key.clone())
            .ok_or(ReconstructError::AccountIndex(i))
    };
    let program = key(raw.program_id_index)?;
    let accs = raw
        .accounts
        .iter()
        .map(|&i| key(i))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((program, accs))
}

/// Reconstruct a transaction from parsed evidence.
pub fn reconstruct(evidence: TxEvidence) -> Result<Tx, ReconstructError> {
    let signature = evidence
        .signatures
        .first()
        .cloned()
        .ok_or(ReconstructError::NoSignature)?;
    let mut issues = Vec::new();
    if evidence.version == TxVersion::V0
        && !evidence.lookup_tables.is_empty()
        && evidence.loaded_writable.is_empty()
        && evidence.loaded_readonly.is_empty()
    {
        return Err(ReconstructError::LoadedAddressesMissing);
    }
    let accounts = build_accounts(&evidence)?;
    if evidence.pre_balances.len() != accounts.len()
        || evidence.post_balances.len() != accounts.len()
    {
        issues.push(Issue::new(
            IssueCode::BalanceLengthMismatch,
            None,
            format!(
                "{} accounts, {} pre / {} post balances",
                accounts.len(),
                evidence.pre_balances.len(),
                evidence.post_balances.len()
            ),
        ));
    }
    if evidence.instructions.len() > usize::from(u16::MAX) {
        return Err(ReconstructError::TooManyInstructions);
    }
    let inner_complete = evidence.inner.is_some();
    if !inner_complete {
        issues.push(Issue::new(
            IssueCode::InnerInstructionsMissing,
            None,
            "node did not record inner instructions; CPI activity unknown",
        ));
    }
    if evidence.pre_token_balances.is_none() || evidence.post_token_balances.is_none() {
        issues.push(Issue::new(
            IssueCode::TokenBalancesMissing,
            None,
            "token balances not recorded; token deltas unknown",
        ));
    }

    let mut ixs: Vec<Ix> = Vec::new();
    let mut top_level = Vec::with_capacity(evidence.instructions.len());
    let mut stack_height_missing = false;
    for (oi, raw) in evidence.instructions.iter().enumerate() {
        let outer = u16::try_from(oi).map_err(|_| ReconstructError::TooManyInstructions)?;
        let (program, accs) = resolve(&accounts, raw)?;
        let root = ixs.len();
        top_level.push(root);
        ixs.push(Ix {
            id: IxRef { outer, inner: None },
            program,
            accounts: accs,
            data: raw.data.clone(),
            stack_height: 1,
            parent: None,
            children: Vec::new(),
            subtree_end: root + 1,
        });
        let inner_list = evidence
            .inner
            .as_ref()
            .and_then(|sets| sets.iter().find(|s| s.index == oi))
            .map(|s| s.instructions.as_slice())
            .unwrap_or(&[]);
        // Stack of (stack height, ixs index) along the current CPI path.
        let mut path: Vec<(u32, usize)> = vec![(1, root)];
        for (ii, raw_inner) in inner_list.iter().enumerate() {
            let inner = u16::try_from(ii).map_err(|_| ReconstructError::TooManyInstructions)?;
            let (program, accs) = resolve(&accounts, raw_inner)?;
            let height = match raw_inner.stack_height {
                Some(h) if h >= 2 => h,
                _ => {
                    stack_height_missing = true;
                    2
                }
            };
            while path.last().is_some_and(|&(h, _)| h >= height) {
                path.pop();
            }
            let parent = path.last().map_or(root, |&(_, p)| p);
            let idx = ixs.len();
            ixs.push(Ix {
                id: IxRef {
                    outer,
                    inner: Some(inner),
                },
                program,
                accounts: accs,
                data: raw_inner.data.clone(),
                stack_height: height,
                parent: Some(parent),
                children: Vec::new(),
                subtree_end: idx + 1,
            });
            if let Some(p) = ixs.get_mut(parent) {
                p.children.push(idx);
            }
            path.push((height, idx));
        }
    }
    // Subtree ends: walk backwards so children are final before parents.
    for i in (0..ixs.len()).rev() {
        let end = ixs[i]
            .children
            .iter()
            .map(|&c| ixs[c].subtree_end)
            .max()
            .unwrap_or(i + 1)
            .max(i + 1);
        ixs[i].subtree_end = end;
    }
    if stack_height_missing {
        issues.push(Issue::new(
            IssueCode::StackHeightMissing,
            None,
            "stackHeight absent; inner instructions attached to their top-level instruction",
        ));
    }
    Ok(Tx {
        evidence,
        signature,
        accounts,
        ixs,
        top_level,
        issues,
        inner_complete,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::evidence::InnerSet;

    fn raw(p: usize, accts: &[usize], h: Option<u32>) -> RawInstruction {
        RawInstruction {
            program_id_index: p,
            accounts: accts.to_vec(),
            data: vec![],
            stack_height: h,
        }
    }

    fn ev(keys: usize, ixs: Vec<RawInstruction>, inner: Option<Vec<InnerSet>>) -> TxEvidence {
        TxEvidence {
            signatures: vec!["sig".into()],
            slot: 1,
            block_time: None,
            version: TxVersion::V0,
            num_required_signatures: 1,
            num_readonly_signed: 0,
            num_readonly_unsigned: 1,
            static_keys: (0..keys).map(|i| format!("k{i}")).collect(),
            loaded_writable: vec!["lw".into()],
            loaded_readonly: vec!["lr".into()],
            lookup_tables: vec!["alt".into()],
            instructions: ixs,
            inner,
            fee: 5_000,
            failed: false,
            err_json: None,
            pre_balances: vec![0; keys + 2],
            post_balances: vec![0; keys + 2],
            pre_token_balances: Some(vec![]),
            post_token_balances: Some(vec![]),
            log_messages: None,
            compute_units_consumed: None,
            transaction_index: None,
        }
    }

    #[test]
    fn account_order_and_flags_follow_the_runtime_rules() {
        let tx = reconstruct(ev(3, vec![], Some(vec![]))).unwrap();
        let keys: Vec<_> = tx.accounts.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, ["k0", "k1", "k2", "lw", "lr"]);
        assert!(tx.accounts[0].signer && tx.accounts[0].writable);
        assert!(!tx.accounts[1].signer && tx.accounts[1].writable);
        assert!(
            !tx.accounts[2].writable,
            "last static key is readonly-unsigned"
        );
        assert!(tx.accounts[3].writable && !tx.accounts[4].writable);
        assert_eq!(tx.fee_payer(), "k0");
    }

    #[test]
    fn stack_heights_build_the_cpi_tree() {
        // outer 0 -> [a(h2) -> [b(h3) -> c(h4)], d(h2)]
        let inner = vec![InnerSet {
            index: 0,
            instructions: vec![
                raw(1, &[0], Some(2)),
                raw(2, &[0], Some(3)),
                raw(1, &[0], Some(4)),
                raw(2, &[3], Some(2)),
            ],
        }];
        let tx = reconstruct(ev(
            3,
            vec![raw(2, &[0, 4], None), raw(1, &[], None)],
            Some(inner),
        ))
        .unwrap();
        assert_eq!(tx.ixs.len(), 6);
        assert_eq!(tx.ixs[0].children, vec![1, 4]);
        assert_eq!(tx.ixs[1].children, vec![2]);
        assert_eq!(tx.ixs[2].children, vec![3]);
        assert_eq!(tx.ixs[0].subtree_end, 5);
        assert_eq!(tx.ixs[1].subtree_end, 4);
        assert_eq!(tx.ixs[3].id.to_string(), "0.2");
        assert_eq!(tx.ixs[5].id.to_string(), "1");
        assert_eq!(tx.ixs[0].accounts, ["k0", "lr"]);
        assert!(tx.is_descendant(3, 0) && !tx.is_descendant(5, 0));
        assert!(tx.issues.is_empty());
    }

    #[test]
    fn missing_stack_height_and_inner_are_reported_not_guessed() {
        let inner = vec![InnerSet {
            index: 0,
            instructions: vec![raw(1, &[], None)],
        }];
        let tx = reconstruct(ev(3, vec![raw(2, &[], None)], Some(inner))).unwrap();
        assert!(
            tx.issues
                .iter()
                .any(|i| i.code == IssueCode::StackHeightMissing)
        );
        let tx = reconstruct(ev(3, vec![raw(2, &[], None)], None)).unwrap();
        assert!(!tx.inner_complete);
        assert!(
            tx.issues
                .iter()
                .any(|i| i.code == IssueCode::InnerInstructionsMissing)
        );
    }

    #[test]
    fn bad_indices_and_missing_loaded_addresses_fail() {
        assert_eq!(
            reconstruct(ev(3, vec![raw(9, &[], None)], Some(vec![]))).unwrap_err(),
            ReconstructError::AccountIndex(9)
        );
        let mut e = ev(3, vec![], Some(vec![]));
        e.loaded_readonly.clear();
        e.loaded_writable.clear();
        assert_eq!(
            reconstruct(e).unwrap_err(),
            ReconstructError::LoadedAddressesMissing
        );
    }
}
