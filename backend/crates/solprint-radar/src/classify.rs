//! Memecoin relevance classification (`radar-class-1`).
//!
//! Deterministic and evidence-based: a token is classified by its **mint**,
//! provider tags, launchpad evidence and the programs of its pools. Names and
//! symbols are untrusted display text and are never used. A classification is
//! a discovery routing decision, never a statement about a token's quality.

use crate::model::{ESTABLISHED_MINTS, LST_MINTS, PYUSD_MINT, USDC_MINT, USDT_MINT, WSOL_MINT};

pub const MEME_CANDIDATE: &str = "MEME_CANDIDATE";
pub const GENERAL_SPECULATIVE_TOKEN: &str = "GENERAL_SPECULATIVE_TOKEN";
pub const ESTABLISHED_ASSET: &str = "ESTABLISHED_ASSET";
pub const STABLECOIN: &str = "STABLECOIN";
pub const WRAPPED_NATIVE_ASSET: &str = "WRAPPED_NATIVE_ASSET";
pub const LIQUID_STAKING_ASSET: &str = "LIQUID_STAKING_ASSET";
pub const LIQUIDITY_POSITION_TOKEN: &str = "LIQUIDITY_POSITION_TOKEN";
pub const KNOWN_INFRASTRUCTURE_ASSET: &str = "KNOWN_INFRASTRUCTURE_ASSET";
pub const UNKNOWN: &str = "UNKNOWN";

/// Age under which an unverified token without launchpad evidence is
/// treated as speculative (documented assumption, not a market statistic).
pub const YOUNG_TOKEN_DAYS: i64 = 30;

/// What is known about a token when it is classified.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenEvidence<'a> {
    pub mint: &'a str,
    /// Provider tags (Jupiter Tokens V2 `tags`), lower-case.
    pub tags: &'a [String],
    pub launchpad: Option<&'a str>,
    pub verified: Option<bool>,
    /// Age of the token (or of its first pool) in days, when known.
    pub age_days: Option<i64>,
    /// Protocols of the token's validated pools (`PUMP_FUN`, `PUMPSWAP`, ...).
    pub pool_protocols: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub label: &'static str,
    pub reasons: Vec<String>,
}

impl Classification {
    fn of(label: &'static str, reason: &str) -> Self {
        Self {
            label,
            reasons: vec![reason.to_owned()],
        }
    }

    /// Classes that are never sampled for memecoin trader discovery.
    #[must_use]
    pub fn excluded_from_sampling(&self) -> bool {
        matches!(
            self.label,
            STABLECOIN
                | WRAPPED_NATIVE_ASSET
                | LIQUID_STAKING_ASSET
                | LIQUIDITY_POSITION_TOKEN
                | KNOWN_INFRASTRUCTURE_ASSET
        )
    }
}

fn has_tag(tags: &[String], t: &str) -> bool {
    tags.iter().any(|x| x.eq_ignore_ascii_case(t))
}

/// Classify a token. Rules apply in order; the first match wins, and every
/// result carries the reason that produced it.
#[must_use]
pub fn classify(t: &TokenEvidence<'_>) -> Classification {
    match t.mint {
        USDC_MINT | USDT_MINT | PYUSD_MINT => {
            return Classification::of(STABLECOIN, "KNOWN_STABLECOIN_MINT");
        }
        WSOL_MINT => return Classification::of(WRAPPED_NATIVE_ASSET, "WRAPPED_SOL_MINT"),
        m if LST_MINTS.contains(&m) => {
            return Classification::of(LIQUID_STAKING_ASSET, "KNOWN_LST_MINT");
        }
        m if ESTABLISHED_MINTS.contains(&m) => {
            return Classification::of(ESTABLISHED_ASSET, "KNOWN_ESTABLISHED_MINT");
        }
        _ => {}
    }
    if has_tag(t.tags, "lst") {
        return Classification::of(LIQUID_STAKING_ASSET, "PROVIDER_TAG_LST");
    }
    if has_tag(t.tags, "stablecoin") {
        return Classification::of(STABLECOIN, "PROVIDER_TAG_STABLECOIN");
    }
    if has_tag(t.tags, "lp-token") || has_tag(t.tags, "lp") {
        return Classification::of(LIQUIDITY_POSITION_TOKEN, "PROVIDER_TAG_LP");
    }
    if t.launchpad.is_some_and(|l| !l.trim().is_empty()) {
        return Classification::of(MEME_CANDIDATE, "LAUNCHPAD_EVIDENCE");
    }
    if t.pool_protocols
        .iter()
        .any(|p| p == "PUMP_FUN" || p == "PUMPSWAP")
    {
        return Classification::of(MEME_CANDIDATE, "BONDING_CURVE_POOL");
    }
    if t.verified != Some(true)
        && t.age_days
            .is_some_and(|d| (0..YOUNG_TOKEN_DAYS).contains(&d))
    {
        return Classification::of(GENERAL_SPECULATIVE_TOKEN, "YOUNG_UNVERIFIED_TOKEN");
    }
    Classification::of(UNKNOWN, "NO_CLASSIFYING_EVIDENCE")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINT: &str = "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr";

    fn ev<'a>(tags: &'a [String], protocols: &'a [String]) -> TokenEvidence<'a> {
        TokenEvidence {
            mint: MINT,
            tags,
            pool_protocols: protocols,
            ..TokenEvidence::default()
        }
    }

    #[test]
    fn known_mints_are_excluded_from_sampling() {
        for (m, label) in [
            (USDC_MINT, STABLECOIN),
            (WSOL_MINT, WRAPPED_NATIVE_ASSET),
            (LST_MINTS[0], LIQUID_STAKING_ASSET),
        ] {
            let c = classify(&TokenEvidence {
                mint: m,
                ..TokenEvidence::default()
            });
            assert_eq!(c.label, label);
            assert!(c.excluded_from_sampling());
        }
        let bonk = classify(&TokenEvidence {
            mint: ESTABLISHED_MINTS[3],
            ..TokenEvidence::default()
        });
        assert_eq!(bonk.label, ESTABLISHED_ASSET);
        assert!(!bonk.excluded_from_sampling());
    }

    #[test]
    fn launchpad_and_bonding_curve_pools_mark_meme_candidates() {
        let mut e = ev(&[], &[]);
        e.launchpad = Some("pump.fun");
        assert_eq!(classify(&e).label, MEME_CANDIDATE);
        let p = vec!["PUMPSWAP".to_owned()];
        let c = classify(&ev(&[], &p));
        assert_eq!(c.label, MEME_CANDIDATE);
        assert_eq!(c.reasons, vec!["BONDING_CURVE_POOL".to_owned()]);
    }

    #[test]
    fn tags_win_over_launchpad_and_age_needs_evidence() {
        let tags = vec!["LST".to_owned()];
        let mut e = ev(&tags, &[]);
        e.launchpad = Some("x");
        assert_eq!(classify(&e).label, LIQUID_STAKING_ASSET);

        let mut young = ev(&[], &[]);
        young.age_days = Some(3);
        assert_eq!(classify(&young).label, GENERAL_SPECULATIVE_TOKEN);
        young.verified = Some(true);
        assert_eq!(classify(&young).label, UNKNOWN);
        // Unknown age is not "young".
        assert_eq!(classify(&ev(&[], &[])).label, UNKNOWN);
    }
}
