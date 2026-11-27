//! Exact native and token balance changes from transaction metadata.
//! All amounts are integer base units; deltas are `i128` (they fit the
//! database's 65-bit signed domain because each side is a `u64`).

use std::collections::BTreeMap;

use crate::programs::WSOL_MINT;
use crate::reconstruct::Tx;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeChange {
    pub account_index: usize,
    pub account: String,
    pub pre: u64,
    pub post: u64,
    pub delta: i128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenChange {
    pub account_index: usize,
    /// The token account.
    pub account: String,
    pub mint: String,
    pub owner: Option<String>,
    pub program_id: Option<String>,
    pub decimals: u8,
    pub pre: u64,
    pub post: u64,
    pub delta: i128,
    /// `false`: absent from preTokenBalances (account created in this tx).
    pub pre_reported: bool,
    /// `false`: absent from postTokenBalances (account closed in this tx).
    pub post_reported: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Balances {
    pub native: Vec<NativeChange>,
    pub token: Vec<TokenChange>,
    /// `false` when token balances were not recorded.
    pub token_known: bool,
}

#[must_use]
pub fn delta(pre: u64, post: u64) -> i128 {
    i128::from(post) - i128::from(pre)
}

/// Compute every non-zero native change and every token-account change
/// (including accounts created or closed in the transaction, whose missing
/// side is zero).
#[must_use]
pub fn compute(tx: &Tx) -> Balances {
    let e = &tx.evidence;
    let mut native = Vec::new();
    for (i, acct) in tx.accounts.iter().enumerate() {
        let (Some(&pre), Some(&post)) = (e.pre_balances.get(i), e.post_balances.get(i)) else {
            continue;
        };
        if pre != post {
            native.push(NativeChange {
                account_index: i,
                account: acct.key.clone(),
                pre,
                post,
                delta: delta(pre, post),
            });
        }
    }
    let (Some(pre_t), Some(post_t)) = (&e.pre_token_balances, &e.post_token_balances) else {
        return Balances {
            native,
            token: Vec::new(),
            token_known: false,
        };
    };
    // (account index, mint) -> (pre, post, owner, program, decimals)
    type Entry = (u64, u64, Option<String>, Option<String>, u8, bool, bool);
    let mut map: BTreeMap<(usize, String), Entry> = BTreeMap::new();
    for b in pre_t {
        let en = map
            .entry((b.account_index, b.mint.clone()))
            .or_insert((0, 0, None, None, b.decimals, false, false));
        en.0 = b.amount;
        en.5 = true;
        en.2.clone_from(&b.owner);
        en.3.clone_from(&b.program_id);
    }
    for b in post_t {
        let en = map
            .entry((b.account_index, b.mint.clone()))
            .or_insert((0, 0, None, None, b.decimals, false, false));
        en.1 = b.amount;
        en.6 = true;
        if b.owner.is_some() {
            en.2.clone_from(&b.owner);
        }
        if b.program_id.is_some() {
            en.3.clone_from(&b.program_id);
        }
        en.4 = b.decimals;
    }
    let token = map
        .into_iter()
        .map(
            |(
                (idx, mint),
                (pre, post, owner, program_id, decimals, pre_reported, post_reported),
            )| TokenChange {
                account_index: idx,
                account: tx
                    .accounts
                    .get(idx)
                    .map(|a| a.key.clone())
                    .unwrap_or_default(),
                mint,
                owner,
                program_id,
                decimals,
                pre,
                post,
                delta: delta(pre, post),
                pre_reported,
                post_reported,
            },
        )
        .collect();
    Balances {
        native,
        token,
        token_known: true,
    }
}

impl Balances {
    #[must_use]
    pub fn native_delta(&self, account: &str) -> i128 {
        self.native
            .iter()
            .filter(|c| c.account == account)
            .map(|c| c.delta)
            .sum()
    }

    #[must_use]
    pub fn token_account(&self, account: &str) -> Option<&TokenChange> {
        self.token.iter().find(|c| c.account == account)
    }

    /// Net change of `mint` across all token accounts owned by `owner`.
    #[must_use]
    pub fn owner_token_delta(&self, owner: &str, mint: &str) -> i128 {
        self.token
            .iter()
            .filter(|c| c.owner.as_deref() == Some(owner) && c.mint == mint)
            .map(|c| c.delta)
            .sum()
    }

    /// Decimals of `mint` observed in this transaction's metadata.
    #[must_use]
    pub fn decimals_of(&self, mint: &str) -> Option<u8> {
        if mint == WSOL_MINT {
            return Some(9);
        }
        self.token
            .iter()
            .find(|c| c.mint == mint)
            .map(|c| c.decimals)
    }

    /// Mint and owner of a token account known from balances metadata.
    #[must_use]
    pub fn mint_of(&self, account: &str) -> Option<&str> {
        self.token_account(account).map(|c| c.mint.as_str())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn delta_is_exact(pre in any::<u64>(), post in any::<u64>()) {
            let d = delta(pre, post);
            prop_assert_eq!(i128::from(pre) + d, i128::from(post));
            prop_assert!(d.unsigned_abs() <= u128::from(u64::MAX));
        }
    }
}
