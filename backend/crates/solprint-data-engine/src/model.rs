//! Normalized domain records produced by decoding. Every enum's text form is
//! exactly the value accepted by the database CHECK constraints in
//! `migrations/20261127000003_data_engine.sql`.

use rust_decimal::Decimal;
use serde::Serialize;

use crate::balances::Balances;
use crate::quality::Issue;

/// Version of the decoding logic. Bump whenever any decoder's output for the
/// same evidence can change; reprocessing compares it.
pub const DECODER_VERSION: &str = "de-2026.11.1";

macro_rules! text_enum {
    ($(#[$m:meta])* $name:ident { $($v:ident => $t:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        pub enum $name { $(#[serde(rename = $t)] $v),* }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$v),*];
            #[must_use]
            pub const fn as_str(self) -> &'static str { match self { $(Self::$v => $t),* } }
            #[must_use]
            pub fn parse(s: &str) -> Option<Self> { match s { $($t => Some(Self::$v),)* _ => None } }
        }
    };
}

text_enum!(
    /// Classification confidence of a decoded record.
    DecodeStatus {
        Verified => "VERIFIED",
        PartiallyDecoded => "PARTIALLY_DECODED",
        Ambiguous => "AMBIGUOUS",
        Unsupported => "UNSUPPORTED",
        InsufficientEvidence => "INSUFFICIENT_EVIDENCE",
        Invalid => "INVALID",
    }
);

impl DecodeStatus {
    /// Rank for "worst of": higher is weaker.
    #[must_use]
    pub const fn weakness(self) -> u8 {
        match self {
            Self::Verified => 0,
            Self::PartiallyDecoded => 1,
            Self::Ambiguous => 2,
            Self::Unsupported => 3,
            Self::InsufficientEvidence => 4,
            Self::Invalid => 5,
        }
    }

    #[must_use]
    pub fn weaker(self, other: Self) -> Self {
        if other.weakness() > self.weakness() {
            other
        } else {
            self
        }
    }
}

text_enum!(ActivityKind {
    Swap => "SWAP",
    TokenTransfer => "TOKEN_TRANSFER",
    NativeTransfer => "NATIVE_TRANSFER",
    TokenMint => "TOKEN_MINT",
    TokenBurn => "TOKEN_BURN",
    LiquidityDeposit => "LIQUIDITY_DEPOSIT",
    LiquidityWithdrawal => "LIQUIDITY_WITHDRAWAL",
    PoolCreation => "POOL_CREATION",
    BondingCurveTrade => "BONDING_CURVE_TRADE",
    LiquidityMigration => "LIQUIDITY_MIGRATION",
    AccountCreation => "ACCOUNT_CREATION",
    AccountClosure => "ACCOUNT_CLOSURE",
    WrapSol => "WRAP_SOL",
    UnwrapSol => "UNWRAP_SOL",
    Fee => "FEE",
    Tip => "TIP",
    FailedTransaction => "FAILED_TRANSACTION",
    UnknownActivity => "UNKNOWN_ACTIVITY",
    AmbiguousActivity => "AMBIGUOUS_ACTIVITY",
});

text_enum!(Attribution {
    VerifiedOwner => "VERIFIED_OWNER",
    VerifiedAuthority => "VERIFIED_AUTHORITY",
    VerifiedBeneficiary => "VERIFIED_BENEFICIARY",
    MultiplePossibleActors => "MULTIPLE_POSSIBLE_ACTORS",
    UnknownOwner => "UNKNOWN_OWNER",
    ProgramControlled => "PROGRAM_CONTROLLED",
    Ambiguous => "AMBIGUOUS",
});

impl Attribution {
    /// Only these attribute a swap to a wallet.
    #[must_use]
    pub const fn is_verified(self) -> bool {
        matches!(
            self,
            Self::VerifiedOwner | Self::VerifiedAuthority | Self::VerifiedBeneficiary
        )
    }
}

text_enum!(AssetKind {
    NativeSol => "NATIVE_SOL",
    WrappedSol => "WRAPPED_SOL",
    SplToken => "SPL_TOKEN",
    Token2022 => "TOKEN_2022",
    Unknown => "UNKNOWN",
});

text_enum!(VenueKind {
    Aggregator => "AGGREGATOR",
    ConstantProductAmm => "CONSTANT_PRODUCT_AMM",
    ConcentratedLiquidity => "CONCENTRATED_LIQUIDITY",
    BondingCurve => "BONDING_CURVE",
});

text_enum!(LegEvidence {
    ProgramEvent => "PROGRAM_EVENT",
    TokenTransfers => "TOKEN_TRANSFERS",
});

text_enum!(FeeKind {
    Network => "NETWORK",
    Protocol => "PROTOCOL",
    Creator => "CREATOR",
    Platform => "PLATFORM",
    TransferFee => "TRANSFER_FEE",
    Other => "OTHER",
});

text_enum!(FeeProvenance {
    Explicit => "EXPLICIT",
    Derived => "DERIVED",
    Unknown => "UNKNOWN",
});

text_enum!(
    /// Finality of a transaction (and of records derived from it).
    Finality {
        ObservedProcessed => "OBSERVED_PROCESSED",
        ObservedConfirmed => "OBSERVED_CONFIRMED",
        Finalized => "FINALIZED",
        FinalityUnknown => "FINALITY_UNKNOWN",
        PossiblyOrphaned => "POSSIBLY_ORPHANED",
        Orphaned => "ORPHANED",
    }
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Activity {
    pub index: u32,
    pub kind: ActivityKind,
    pub status: DecodeStatus,
    pub protocol: Option<&'static str>,
    pub program_id: Option<String>,
    pub instruction_ref: Option<String>,
    pub actor: Option<String>,
    pub attribution: Option<Attribution>,
    /// Mint, or `"SOL"` for native lamports.
    pub mint: Option<String>,
    pub amount: Option<u64>,
    pub source_account: Option<String>,
    pub destination_account: Option<String>,
    pub counterparty: Option<String>,
    pub reasons: Vec<String>,
    pub decoder: &'static str,
}

impl Activity {
    /// Deterministic id: identical evidence and decoder produce identical ids.
    #[must_use]
    pub fn id(&self, signature: &str) -> String {
        format!(
            "{signature}:{}:{}:{}",
            self.instruction_ref.as_deref().unwrap_or("tx"),
            self.kind.as_str(),
            self.index
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwapSide {
    pub mint: Option<String>,
    pub amount: Option<u64>,
    pub decimals: Option<u8>,
    pub asset_kind: AssetKind,
    /// The user token account (or wallet, for native SOL) on this side.
    pub account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteLeg {
    pub protocol: String,
    pub program_id: Option<String>,
    pub pool: Option<String>,
    pub input_mint: String,
    pub input_amount: u64,
    pub output_mint: String,
    pub output_amount: u64,
    pub instruction_ref: Option<String>,
    pub evidence: LegEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwapFee {
    pub kind: FeeKind,
    pub mint: Option<String>,
    pub amount: Option<u64>,
    pub provenance: FeeProvenance,
    pub recipient: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Swap {
    pub instruction_ref: String,
    pub protocol: &'static str,
    pub program_id: String,
    pub venue_kind: VenueKind,
    pub aggregator: Option<&'static str>,
    pub pool: Option<String>,
    pub wallet: Option<String>,
    pub attribution: Attribution,
    pub authority: Option<String>,
    pub beneficiary: Option<String>,
    pub input: SwapSide,
    pub output: SwapSide,
    /// Output per one unit of input, in UI units, 18 dp, rounded toward zero.
    pub exchange_ratio: Option<Decimal>,
    pub route_complete: bool,
    pub status: DecodeStatus,
    pub reasons: Vec<String>,
    pub legs: Vec<RouteLeg>,
    pub fees: Vec<SwapFee>,
    pub decoder: &'static str,
    /// Index of the matching activity in `DecodedTransaction::activities`.
    pub activity_index: u32,
}

impl Swap {
    #[must_use]
    pub fn id(&self, signature: &str) -> String {
        format!("swap:{signature}:{}", self.instruction_ref)
    }

    /// A verified trade: canonical rows of successful transactions only.
    #[must_use]
    pub fn is_verified_trade(&self) -> bool {
        self.status == DecodeStatus::Verified && self.attribution.is_verified()
    }
}

/// Everything derived from one evidence revision.
#[derive(Debug, Clone)]
pub struct DecodedTransaction {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub transaction_index: Option<u32>,
    pub version: &'static str,
    pub failed: bool,
    pub err_json: Option<String>,
    pub fee: u64,
    pub fee_payer: String,
    pub signers: Vec<String>,
    pub compute_units: Option<u64>,
    pub balances: Balances,
    /// Per-token-account ownership evidence (account -> source label).
    pub ownership: std::collections::BTreeMap<String, &'static str>,
    pub activities: Vec<Activity>,
    pub swaps: Vec<Swap>,
    pub issues: Vec<Issue>,
    pub status: DecodeStatus,
    pub protocols: Vec<&'static str>,
    pub decoder_version: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_forms_round_trip() {
        for k in ActivityKind::ALL {
            assert_eq!(ActivityKind::parse(k.as_str()), Some(*k));
        }
        assert_eq!(ActivityKind::ALL.len(), 19);
        for s in DecodeStatus::ALL {
            assert_eq!(DecodeStatus::parse(s.as_str()), Some(*s));
        }
        assert_eq!(
            DecodeStatus::Verified.weaker(DecodeStatus::Ambiguous),
            DecodeStatus::Ambiguous
        );
    }
}
