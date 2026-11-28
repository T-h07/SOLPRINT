//! RADAR vocabulary. Every label is the exact string stored in PostgreSQL
//! and shown by the API, so the database CHECK constraints, the wire
//! contract and the UI agree.

/// Known quote assets. A swap between two of these is not memecoin activity.
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
pub const PYUSD_MINT: &str = "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo";

/// Liquid staking tokens (never sampled as memecoin markets).
pub const LST_MINTS: [&str; 5] = [
    "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",  // mSOL
    "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn", // jitoSOL
    "bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1",  // bSOL
    "jupSoLaHXQiZZTSfEWMTRRgpnyFm8f6sZdosWBjx93v",  // JupSOL
    "5oVNBeEEQvYi1cX3ir8Dx5n1P7pdxydbGF2X4TxVusJm", // INF
];

/// Established, non-memecoin assets (by mint, never by name).
pub const ESTABLISHED_MINTS: [&str; 6] = [
    "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN",  // JUP
    "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R", // RAY
    "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3", // PYTH
    "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", // BONK
    "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", // WIF
    "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE",  // ORCA
];

/// Quote denomination of a swap leg. Values in different denominations are
/// never added together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Denomination {
    Sol,
    Usdc,
    Usdt,
}

impl Denomination {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sol => "SOL",
            Self::Usdc => "USDC",
            Self::Usdt => "USDT",
        }
    }

    /// The denomination of an asset, if it is a quote asset. Native SOL and
    /// wrapped SOL are the same denomination.
    #[must_use]
    pub fn of(mint: Option<&str>, asset_kind: &str) -> Option<Self> {
        if matches!(asset_kind, "NATIVE_SOL" | "WRAPPED_SOL") {
            return Some(Self::Sol);
        }
        match mint? {
            WSOL_MINT => Some(Self::Sol),
            USDC_MINT => Some(Self::Usdc),
            USDT_MINT => Some(Self::Usdt),
            _ => None,
        }
    }

    pub const ALL: [Self; 3] = [Self::Sol, Self::Usdc, Self::Usdt];
}

/// Whether a mint is a quote, stable, wrapped-native or staking asset.
#[must_use]
pub fn is_quote_like(mint: &str) -> bool {
    matches!(mint, WSOL_MINT | USDC_MINT | USDT_MINT | PYUSD_MINT) || LST_MINTS.contains(&mint)
}

/// Discovery buckets. Each has a scheduling share so no single provider
/// ranking or market type takes all of RADAR's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bucket {
    Established,
    Emerging,
    LocalActivity,
    Historical,
    Exploration,
}

impl Bucket {
    pub const ALL: [Self; 5] = [
        Self::Established,
        Self::Emerging,
        Self::LocalActivity,
        Self::Historical,
        Self::Exploration,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Established => "ESTABLISHED",
            Self::Emerging => "EMERGING",
            Self::LocalActivity => "LOCAL_ACTIVITY",
            Self::Historical => "HISTORICAL",
            Self::Exploration => "EXPLORATION",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.as_str() == s)
    }
}

/// Discovery sources (`radar_source_runs.source`, evidence provenance).
pub mod source {
    pub const JUPITER_TOP_TRADED: &str = "JUPITER_TOP_TRADED";
    pub const JUPITER_TOP_ORGANIC: &str = "JUPITER_TOP_ORGANIC";
    pub const JUPITER_RECENT: &str = "JUPITER_RECENT";
    /// Paid promotion: never treated as organic demand.
    pub const DEXSCREENER_BOOSTS: &str = "DEXSCREENER_BOOSTS";
    pub const LOCAL_ACTIVITY: &str = "LOCAL_ACTIVITY";
    pub const HISTORICAL: &str = "HISTORICAL";
    pub const EXPLORATION: &str = "EXPLORATION";
    pub const FIXTURE: &str = "FIXTURE";
    /// Evidence from a wallet's own history investigation.
    pub const WALLET_HISTORY: &str = "WALLET_HISTORY";
    pub const MONITORING: &str = "MONITORING";
    /// A swap the DATA ENGINE ingested for reasons other than RADAR.
    pub const DATA_ENGINE: &str = "DATA_ENGINE";
    pub const OPERATOR: &str = "OPERATOR";
}

pub mod lifecycle {
    pub const DISCOVERED: &str = "DISCOVERED";
    pub const SCREENING: &str = "SCREENING";
    pub const HISTORY_QUEUED: &str = "HISTORY_QUEUED";
    pub const INVESTIGATING: &str = "INVESTIGATING";
    pub const INSUFFICIENT_EVIDENCE: &str = "INSUFFICIENT_EVIDENCE";
    pub const PRELIMINARY_QUALIFIED: &str = "PRELIMINARY_QUALIFIED";
    pub const PRIORITIZED: &str = "PRIORITIZED";
    pub const MONITORED: &str = "MONITORED";
    pub const DORMANT: &str = "DORMANT";
    pub const REJECTED: &str = "REJECTED";
    pub const ARCHIVED: &str = "ARCHIVED";

    pub const ALL: [&str; 11] = [
        DISCOVERED,
        SCREENING,
        HISTORY_QUEUED,
        INVESTIGATING,
        INSUFFICIENT_EVIDENCE,
        PRELIMINARY_QUALIFIED,
        PRIORITIZED,
        MONITORED,
        DORMANT,
        REJECTED,
        ARCHIVED,
    ];
}

pub mod eligibility {
    pub const ELIGIBLE_CANDIDATE: &str = "ELIGIBLE_CANDIDATE";
    pub const PROVISIONAL_CANDIDATE: &str = "PROVISIONAL_CANDIDATE";
    pub const NEEDS_MORE_EVIDENCE: &str = "NEEDS_MORE_EVIDENCE";
    pub const IRRELEVANT_ACTIVITY: &str = "IRRELEVANT_ACTIVITY";
    pub const KNOWN_INFRASTRUCTURE: &str = "KNOWN_INFRASTRUCTURE";
    pub const AMBIGUOUS_ATTRIBUTION: &str = "AMBIGUOUS_ATTRIBUTION";
    pub const UNSUPPORTED_ACCOUNT_ROLE: &str = "UNSUPPORTED_ACCOUNT_ROLE";
    pub const EXCLUDED_BY_POLICY: &str = "EXCLUDED_BY_POLICY";

    pub const ALL: [&str; 8] = [
        ELIGIBLE_CANDIDATE,
        PROVISIONAL_CANDIDATE,
        NEEDS_MORE_EVIDENCE,
        IRRELEVANT_ACTIVITY,
        KNOWN_INFRASTRUCTURE,
        AMBIGUOUS_ATTRIBUTION,
        UNSUPPORTED_ACCOUNT_ROLE,
        EXCLUDED_BY_POLICY,
    ];

    /// Eligibility outcomes that may be investigated.
    #[must_use]
    pub fn investigable(e: &str) -> bool {
        matches!(
            e,
            ELIGIBLE_CANDIDATE | PROVISIONAL_CANDIDATE | NEEDS_MORE_EVIDENCE
        )
    }
}

pub mod qualification {
    pub const INSUFFICIENT_ACTIVITY: &str = "INSUFFICIENT_ACTIVITY";
    pub const INSUFFICIENT_HISTORY: &str = "INSUFFICIENT_HISTORY";
    pub const INSUFFICIENT_FINANCIAL_EVIDENCE: &str = "INSUFFICIENT_FINANCIAL_EVIDENCE";
    pub const PRELIMINARY_POSITIVE_RETURN: &str = "PRELIMINARY_POSITIVE_RETURN";
    pub const PRELIMINARY_NEGATIVE_RETURN: &str = "PRELIMINARY_NEGATIVE_RETURN";
    pub const RECENT_ACTIVITY_QUALIFIED: &str = "RECENT_ACTIVITY_QUALIFIED";
    pub const READY_FOR_DEEPER_ANALYSIS: &str = "READY_FOR_DEEPER_ANALYSIS";

    pub const ALL: [&str; 7] = [
        INSUFFICIENT_ACTIVITY,
        INSUFFICIENT_HISTORY,
        INSUFFICIENT_FINANCIAL_EVIDENCE,
        PRELIMINARY_POSITIVE_RETURN,
        PRELIMINARY_NEGATIVE_RETURN,
        RECENT_ACTIVITY_QUALIFIED,
        READY_FOR_DEEPER_ANALYSIS,
    ];
}

pub mod coverage {
    pub const NOT_INVESTIGATED: &str = "NOT_INVESTIGATED";
    pub const INVESTIGATION_PENDING: &str = "INVESTIGATION_PENDING";
    pub const PARTIAL_HISTORY: &str = "PARTIAL_HISTORY";
    pub const BOUNDED_SAMPLE_COMPLETE: &str = "BOUNDED_SAMPLE_COMPLETE";
    pub const REQUESTED_SCOPE_COMPLETE: &str = "REQUESTED_SCOPE_COMPLETE";
    pub const PROVIDER_HISTORY_UNAVAILABLE: &str = "PROVIDER_HISTORY_UNAVAILABLE";
    pub const DECODER_COVERAGE_LIMITED: &str = "DECODER_COVERAGE_LIMITED";
    pub const UNRESOLVED_GAPS: &str = "UNRESOLVED_GAPS";
    pub const UNKNOWN: &str = "UNKNOWN";

    pub const ALL: [&str; 9] = [
        NOT_INVESTIGATED,
        INVESTIGATION_PENDING,
        PARTIAL_HISTORY,
        BOUNDED_SAMPLE_COMPLETE,
        REQUESTED_SCOPE_COMPLETE,
        PROVIDER_HISTORY_UNAVAILABLE,
        DECODER_COVERAGE_LIMITED,
        UNRESOLVED_GAPS,
        UNKNOWN,
    ];
}

pub mod monitoring {
    pub const LIVE_SUBSCRIPTION: &str = "LIVE_SUBSCRIPTION";
    pub const PERIODIC_HISTORY_REFRESH: &str = "PERIODIC_HISTORY_REFRESH";
    pub const PASSIVE_LOCAL_OBSERVATION: &str = "PASSIVE_LOCAL_OBSERVATION";
    pub const DEFERRED: &str = "DEFERRED";
    pub const INACTIVE: &str = "INACTIVE";
}

pub mod work {
    pub const TRANSACTION_SAMPLING: &str = "TRANSACTION_SAMPLING";
    pub const INITIAL_HISTORY: &str = "INITIAL_HISTORY";
    pub const EXPANDED_HISTORY: &str = "EXPANDED_HISTORY";
    pub const EXTENDED_HISTORY: &str = "EXTENDED_HISTORY";
    pub const ACTIVE_MONITORING: &str = "ACTIVE_MONITORING";
    pub const DORMANT_RECHECK: &str = "DORMANT_RECHECK";

    pub const HISTORY: [&str; 5] = [
        INITIAL_HISTORY,
        EXPANDED_HISTORY,
        EXTENDED_HISTORY,
        ACTIVE_MONITORING,
        DORMANT_RECHECK,
    ];
}

/// Versioned policies (stored with every decision).
pub const CLASSIFICATION_RULESET: &str = "radar-class-1";
pub const PRIORITY_POLICY: &str = "radar-priority-1";
pub const LIFECYCLE_POLICY: &str = "radar-lifecycle-1";

/// A RADAR history stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    A,
    B,
    C,
}

impl Stage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
        }
    }

    #[must_use]
    pub const fn work_class(self) -> &'static str {
        match self {
            Self::A => work::INITIAL_HISTORY,
            Self::B => work::EXPANDED_HISTORY,
            Self::C => work::EXTENDED_HISTORY,
        }
    }

    #[must_use]
    pub fn of_work_class(w: &str) -> Option<Self> {
        match w {
            work::INITIAL_HISTORY => Some(Self::A),
            work::EXPANDED_HISTORY => Some(Self::B),
            work::EXTENDED_HISTORY => Some(Self::C),
            _ => None,
        }
    }

    /// The last completed stage from its stored label (`NONE` → `None`).
    #[must_use]
    pub fn parse_completed(s: &str) -> Option<Self> {
        match s {
            "A" => Some(Self::A),
            "B" => Some(Self::B),
            "C" => Some(Self::C),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_wrapped_sol_are_one_denomination() {
        assert_eq!(
            Denomination::of(None, "NATIVE_SOL"),
            Some(Denomination::Sol)
        );
        assert_eq!(
            Denomination::of(Some(WSOL_MINT), "SPL_TOKEN"),
            Some(Denomination::Sol)
        );
        assert_eq!(
            Denomination::of(Some(USDC_MINT), "SPL_TOKEN"),
            Some(Denomination::Usdc)
        );
        assert_eq!(Denomination::of(Some(PYUSD_MINT), "SPL_TOKEN"), None);
        assert!(is_quote_like(PYUSD_MINT));
        assert!(is_quote_like(LST_MINTS[1]));
        assert!(!is_quote_like(ESTABLISHED_MINTS[3]));
    }

    #[test]
    fn labels_round_trip() {
        for b in Bucket::ALL {
            assert_eq!(Bucket::parse(b.as_str()), Some(b));
        }
        for s in [Stage::A, Stage::B, Stage::C] {
            assert_eq!(Stage::of_work_class(s.work_class()), Some(s));
            assert_eq!(Stage::parse_completed(s.as_str()), Some(s));
        }
        assert_eq!(Stage::parse_completed("NONE"), None);
    }
}
