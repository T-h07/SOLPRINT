//! Data-quality issues. An issue is a recorded, typed doubt about a piece of
//! derived data; it never silently disappears and never upgrades data to
//! VERIFIED. See `docs/architecture/DATA_QUALITY.md`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Info,
    Warning,
    /// The derived data is wrong or unusable.
    Invalid,
    /// Pipeline/coverage condition (gaps, unknown finality), not a decoding fault.
    Operational,
    Critical,
}

impl Severity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Invalid => "INVALID",
            Self::Operational => "OPERATIONAL",
            Self::Critical => "CRITICAL",
        }
    }
}

macro_rules! issue_codes {
    ($($name:ident => ($text:literal, $sev:ident)),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum IssueCode { $($name),* }

        impl IssueCode {
            pub const ALL: &'static [IssueCode] = &[$(IssueCode::$name),*];
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$name => $text),* }
            }
            #[must_use]
            pub const fn default_severity(self) -> Severity {
                match self { $(Self::$name => Severity::$sev),* }
            }
        }
    };
}

issue_codes! {
    EvidenceParseFailed => ("EVIDENCE_PARSE_FAILED", Invalid),
    EvidenceTooLarge => ("EVIDENCE_TOO_LARGE", Invalid),
    InnerInstructionsMissing => ("INNER_INSTRUCTIONS_MISSING", Warning),
    StackHeightMissing => ("STACK_HEIGHT_MISSING", Info),
    TokenBalancesMissing => ("TOKEN_BALANCES_MISSING", Warning),
    AccountIndexOutOfRange => ("ACCOUNT_INDEX_OUT_OF_RANGE", Invalid),
    BalanceLengthMismatch => ("BALANCE_LENGTH_MISMATCH", Invalid),
    LoadedAddressesMissing => ("LOADED_ADDRESSES_MISSING", Invalid),
    BalanceReconciliationFailed => ("BALANCE_RECONCILIATION_FAILED", Warning),
    UnsupportedInstruction => ("UNSUPPORTED_INSTRUCTION", Info),
    UnsupportedProgram => ("UNSUPPORTED_PROGRAM", Info),
    UnsupportedTokenExtension => ("UNSUPPORTED_TOKEN_EXTENSION", Warning),
    DecoderAmountMismatch => ("DECODER_AMOUNT_MISMATCH", Warning),
    DecoderMalformedData => ("DECODER_MALFORMED_DATA", Warning),
    EventMissing => ("EVENT_MISSING", Warning),
    RouteIncomplete => ("ROUTE_INCOMPLETE", Warning),
    AttributionAmbiguous => ("ATTRIBUTION_AMBIGUOUS", Info),
    UnknownMintDecimals => ("UNKNOWN_MINT_DECIMALS", Warning),
    PriceUnavailable => ("PRICE_UNAVAILABLE", Info),
    FinalityUnknown => ("FINALITY_UNKNOWN", Operational),
    SlotMismatch => ("SLOT_MISMATCH", Invalid),
    TransactionOrphaned => ("TRANSACTION_ORPHANED", Critical),
    TransactionUnavailable => ("TRANSACTION_UNAVAILABLE", Operational),
    StreamGap => ("STREAM_GAP", Operational),
    ContinuityUnknown => ("CONTINUITY_UNKNOWN", Operational),
    BackfillIncomplete => ("BACKFILL_INCOMPLETE", Operational),
    QueueSaturated => ("QUEUE_SATURATED", Operational),
    BudgetExhausted => ("BUDGET_EXHAUSTED", Operational),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub severity: Severity,
    /// Instruction reference (`"3"`, `"3.2"`) when the issue is local.
    pub instruction_ref: Option<String>,
    /// Short, factual, secret-free detail (capped at 256 chars).
    pub detail: String,
}

impl Issue {
    #[must_use]
    pub fn new(
        code: IssueCode,
        instruction_ref: Option<String>,
        detail: impl Into<String>,
    ) -> Self {
        let detail: String = detail.into();
        Self {
            code,
            severity: code.default_severity(),
            instruction_ref,
            detail: detail.chars().take(256).collect(),
        }
    }

    /// Stable key for deduplicating open issues of one subject.
    #[must_use]
    pub fn dedup_key(&self, subject: &str) -> String {
        format!(
            "{subject}|{}|{}",
            self.code.as_str(),
            self.instruction_ref.as_deref().unwrap_or("-")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_screaming_snake() {
        let mut seen = std::collections::BTreeSet::new();
        for c in IssueCode::ALL {
            assert!(seen.insert(c.as_str()));
            assert!(
                c.as_str()
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch == '_')
            );
            assert_eq!(
                serde_json::to_value(c).ok(),
                Some(serde_json::json!(c.as_str()))
            );
        }
    }

    #[test]
    fn detail_is_capped() {
        let i = Issue::new(IssueCode::RouteIncomplete, None, "x".repeat(1_000));
        assert_eq!(i.detail.len(), 256);
        assert_eq!(i.dedup_key("sig"), "sig|ROUTE_INCOMPLETE|-");
    }
}
