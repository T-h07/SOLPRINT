//! Evidence-based wallet attribution. See `docs/architecture/WALLET_ATTRIBUTION.md`.
//!
//! The wallet of a swap is the account the program requires as the user /
//! transfer authority, and only when that account signed the transaction.
//! The fee payer is never used as a substitute: relayers and bots commonly
//! pay fees for other users.

use crate::model::{AssetKind, Attribution, SwapSide};
use crate::movements::TokenAccounts;
use crate::reconstruct::Tx;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attributed {
    pub attribution: Attribution,
    pub wallet: Option<String>,
    pub beneficiary: Option<String>,
    pub reasons: Vec<&'static str>,
}

fn side_owner(accounts: &TokenAccounts, side: &SwapSide) -> Option<String> {
    let acct = side.account.as_deref()?;
    if side.asset_kind == AssetKind::NativeSol && accounts.owner(acct).is_none() {
        // Native SOL sides name the wallet itself.
        return Some(acct.to_owned());
    }
    accounts.owner(acct).map(str::to_owned)
}

#[must_use]
pub fn attribute(
    tx: &Tx,
    accounts: &TokenAccounts,
    authority: Option<&str>,
    input: &SwapSide,
    output: &SwapSide,
) -> Attributed {
    let mut reasons = Vec::new();
    let input_owner = side_owner(accounts, input);
    let beneficiary = side_owner(accounts, output);
    let Some(auth) = authority else {
        reasons.push("AUTHORITY_UNKNOWN");
        return Attributed {
            attribution: Attribution::UnknownOwner,
            wallet: None,
            beneficiary,
            reasons,
        };
    };
    if !tx.is_signer(auth) {
        // An inner-instruction authority that did not sign the transaction
        // can only have signed through `invoke_signed`: a program address.
        reasons.push("AUTHORITY_IS_PROGRAM_SIGNER");
        return Attributed {
            attribution: Attribution::ProgramControlled,
            wallet: None,
            beneficiary,
            reasons,
        };
    }
    let attribution = match input_owner.as_deref() {
        Some(o) if o == auth => Attribution::VerifiedOwner,
        Some(_) => {
            reasons.push("AUTHORITY_IS_DELEGATE");
            Attribution::VerifiedAuthority
        }
        None => {
            reasons.push("SOURCE_OWNER_UNKNOWN");
            Attribution::VerifiedAuthority
        }
    };
    if beneficiary.as_deref().is_some_and(|b| b != auth) {
        reasons.push("BENEFICIARY_DIFFERS");
    }
    Attributed {
        attribution,
        wallet: Some(auth.to_owned()),
        beneficiary,
        reasons,
    }
}
