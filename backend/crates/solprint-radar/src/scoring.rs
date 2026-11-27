//! Eligibility, investigation priority (`radar-priority-1`) and preliminary
//! qualification.
//!
//! The priority score orders RADAR's own investigation and monitoring work.
//! It is **not** a trader-quality rating, a recommendation or a signal, and
//! qualification is kept separate from it: a high priority never qualifies a
//! wallet by itself.
//!
//! ```text
//! score = 100 × (0.25·A + 0.15·R + 0.25·E + 0.25·P + 0.10·D)
//! A = min(verified relevant swaps in 30 d / 20, 1)
//! R = 2^(−days since last verified trade / half-life)    (0 when unknown)
//! E = mean(transaction coverage, decoder support, attribution validity,
//!          finalized share, matched coverage)
//! P = 0.5 + n/(n+10) · (raw − 0.5),  raw = clamp(0.5 + ratio/2, 0, 1)
//!     in the denomination with the most evaluated outcomes (n);
//!     P = 0.5 (UNKNOWN) without outcomes
//! D = min(distinct tokens with valid evidence / 3, 1)
//! ```

use rust_decimal::{Decimal, MathematicalOps};

use crate::model::{Denomination, eligibility as el, qualification as q};
use crate::screening::Screening;

/// What is known about a wallet's evidence, for eligibility.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvidenceSummary {
    pub total: u32,
    pub valid: u32,
    pub valid_finalized: u32,
    /// Valid evidence with a quote-denominated side (buy or sell).
    pub valid_quote_denominated: u32,
    /// Invalidated because the swap lost its owner attribution.
    pub invalid_attribution: u32,
    pub distinct_tokens: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountRole {
    pub is_program: bool,
    pub is_known_pool: bool,
    pub is_token_account: bool,
    pub operator_excluded: bool,
}

/// Eligibility outcome with its reasons (first match wins).
#[must_use]
pub fn eligibility(role: &AccountRole, ev: &EvidenceSummary) -> (&'static str, Vec<String>) {
    let r = |s: &str| vec![s.to_owned()];
    if role.operator_excluded {
        return (el::EXCLUDED_BY_POLICY, r("OPERATOR_EXCLUDED"));
    }
    if role.is_program {
        return (el::KNOWN_INFRASTRUCTURE, r("KNOWN_PROGRAM"));
    }
    if role.is_known_pool {
        return (el::KNOWN_INFRASTRUCTURE, r("KNOWN_POOL_ACCOUNT"));
    }
    if role.is_token_account {
        return (el::UNSUPPORTED_ACCOUNT_ROLE, r("TOKEN_ACCOUNT"));
    }
    if ev.valid == 0 {
        if ev.total > 0 && ev.invalid_attribution == ev.total {
            return (el::AMBIGUOUS_ATTRIBUTION, r("ATTRIBUTION_WITHDRAWN"));
        }
        return (el::IRRELEVANT_ACTIVITY, r("NO_VALID_EVIDENCE"));
    }
    if ev.valid_quote_denominated == 0 {
        return (el::NEEDS_MORE_EVIDENCE, r("TOKEN_TO_TOKEN_ONLY"));
    }
    if ev.valid_finalized > 0 {
        (el::ELIGIBLE_CANDIDATE, r("FINALIZED_VERIFIED_SWAP"))
    } else {
        (el::PROVISIONAL_CANDIDATE, r("CONFIRMED_ONLY"))
    }
}

/// Retrieval and decoding coverage of the wallet's investigated history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    pub signatures_discovered: u64,
    pub transactions_retrieved: u64,
    pub transactions_unavailable: u64,
    pub transactions_verified: u64,
    pub transactions_unsupported: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityInputs {
    pub swaps_30d: u32,
    pub days_since_last_trade: Option<i64>,
    pub half_life_days: u32,
    pub coverage: Coverage,
    pub evidence: EvidenceSummary,
    pub screening_coverage_bps: Option<u32>,
    pub primary: Option<(Denomination, u32, Option<Decimal>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Priority {
    pub score: Decimal,
    pub activity: Decimal,
    pub recency: Decimal,
    pub evidence_quality: Decimal,
    pub performance: Decimal,
    pub performance_status: &'static str,
    pub diversity: Decimal,
    /// The five evidence-quality parts, in formula order.
    pub evidence_parts: [Decimal; 5],
}

fn frac(n: u64, d: u64) -> Decimal {
    if d == 0 {
        return Decimal::ZERO;
    }
    (Decimal::from(n) / Decimal::from(d)).min(Decimal::ONE)
}

fn bps(v: u32) -> Decimal {
    (Decimal::from(v) / Decimal::from(10_000)).min(Decimal::ONE)
}

/// Compute the priority score and every component (each rounded to 6 dp,
/// the score to 4 dp).
#[must_use]
pub fn priority(i: &PriorityInputs) -> Priority {
    let one = Decimal::ONE;
    let half = Decimal::new(5, 1);
    let activity = (Decimal::from(i.swaps_30d) / Decimal::from(20)).min(one);
    let recency = match i.days_since_last_trade {
        Some(d) if i.half_life_days > 0 => {
            let x = Decimal::from(d.max(0)) / Decimal::from(i.half_life_days);
            // 2^(−x); beyond 40 half-lives the value is below 1e-12.
            if x > Decimal::from(40) {
                Decimal::ZERO
            } else {
                Decimal::TWO
                    .checked_powd(-x)
                    .unwrap_or(Decimal::ZERO)
                    .clamp(Decimal::ZERO, one)
            }
        }
        _ => Decimal::ZERO,
    };
    let c = &i.coverage;
    let parts = [
        frac(
            c.transactions_retrieved,
            c.signatures_discovered.max(c.transactions_retrieved),
        ),
        frac(c.transactions_verified, c.transactions_retrieved),
        frac(u64::from(i.evidence.valid), u64::from(i.evidence.total)),
        frac(
            u64::from(i.evidence.valid_finalized),
            u64::from(i.evidence.valid),
        ),
        i.screening_coverage_bps.map_or(Decimal::ZERO, bps),
    ];
    let evidence_quality = parts.iter().copied().sum::<Decimal>() / Decimal::from(5);
    let (performance, performance_status) = match i.primary {
        Some((_, n, Some(ratio))) if n > 0 => {
            let raw = (half + ratio / Decimal::TWO).clamp(Decimal::ZERO, one);
            let shrink = Decimal::from(n) / Decimal::from(n + 10);
            (half + shrink * (raw - half), "ESTIMATED")
        }
        _ => (half, "UNKNOWN"),
    };
    let diversity = (Decimal::from(i.evidence.distinct_tokens) / Decimal::from(3)).min(one);
    let w = |n: i64| Decimal::new(n, 2);
    let score = (Decimal::from(100)
        * (w(25) * activity
            + w(15) * recency
            + w(25) * evidence_quality
            + w(25) * performance
            + w(10) * diversity))
        .round_dp(4)
        .clamp(Decimal::ZERO, Decimal::from(100));
    let r6 = |d: Decimal| d.round_dp(6);
    Priority {
        score,
        activity: r6(activity),
        recency: r6(recency),
        evidence_quality: r6(evidence_quality),
        performance: r6(performance),
        performance_status,
        diversity: r6(diversity),
        evidence_parts: parts.map(r6),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationPolicy {
    pub min_swaps: u32,
    pub min_trading_days: u32,
    pub min_outcomes: u32,
    pub min_matched_coverage_bps: u32,
    pub max_inactive_days: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationInputs {
    pub swaps_30d: u32,
    pub trading_days_30d: u32,
    pub days_since_last_trade: Option<i64>,
    /// Stage B (30-day history) completed.
    pub stage_b_done: bool,
    /// Stage C (90-day history) completed.
    pub stage_c_done: bool,
    pub screening: Option<ScreeningFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreeningFacts {
    pub outcomes: u32,
    pub coverage_bps: Option<u32>,
    pub primary_ratio: Option<Decimal>,
}

impl ScreeningFacts {
    #[must_use]
    pub fn of(s: &Screening) -> Self {
        let primary = s.primary();
        Self {
            outcomes: primary.map_or(0, |(_, p)| p.outcomes),
            coverage_bps: s.matched_coverage_bps(),
            primary_ratio: primary.and_then(|(_, p)| p.return_ratio()),
        }
    }
}

/// Preliminary qualification with its reasons.
#[must_use]
pub fn qualify(p: &QualificationPolicy, i: &QualificationInputs) -> (&'static str, Vec<String>) {
    let mut reasons = Vec::new();
    if i.swaps_30d < p.min_swaps {
        reasons.push("FEW_VERIFIED_SWAPS_30D".to_owned());
    }
    if i.trading_days_30d < p.min_trading_days {
        reasons.push("FEW_TRADING_DAYS_30D".to_owned());
    }
    match i.days_since_last_trade {
        None => reasons.push("LAST_TRADE_UNKNOWN".to_owned()),
        Some(d) if d > i64::from(p.max_inactive_days) => reasons.push("INACTIVE".to_owned()),
        Some(_) => {}
    }
    if !reasons.is_empty() {
        return (q::INSUFFICIENT_ACTIVITY, reasons);
    }
    if !i.stage_b_done {
        return (
            q::INSUFFICIENT_HISTORY,
            vec!["STAGE_B_NOT_COMPLETE".to_owned()],
        );
    }
    let financial = i.screening.as_ref().filter(|s| {
        s.outcomes >= p.min_outcomes
            && s.coverage_bps
                .is_some_and(|c| c >= p.min_matched_coverage_bps)
    });
    let base = match financial.and_then(|s| s.primary_ratio) {
        Some(r) if r > Decimal::ZERO => (q::PRELIMINARY_POSITIVE_RETURN, Vec::new()),
        Some(_) => {
            return (
                q::PRELIMINARY_NEGATIVE_RETURN,
                vec!["MATCHED_RETURN_NOT_POSITIVE".to_owned()],
            );
        }
        None => (
            q::RECENT_ACTIVITY_QUALIFIED,
            vec![q::INSUFFICIENT_FINANCIAL_EVIDENCE.to_owned()],
        ),
    };
    if i.stage_c_done {
        let mut r = vec![format!("BASIS_{}", base.0)];
        r.extend(base.1);
        return (q::READY_FOR_DEEPER_ANALYSIS, r);
    }
    base
}

/// Whether a qualification lets the investigation continue.
#[must_use]
pub fn is_promising(label: &str) -> bool {
    matches!(
        label,
        q::PRELIMINARY_POSITIVE_RETURN
            | q::RECENT_ACTIVITY_QUALIFIED
            | q::READY_FOR_DEEPER_ANALYSIS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> PriorityInputs {
        PriorityInputs {
            swaps_30d: 20,
            days_since_last_trade: Some(0),
            half_life_days: 7,
            coverage: Coverage {
                signatures_discovered: 10,
                transactions_retrieved: 10,
                transactions_unavailable: 0,
                transactions_verified: 10,
                transactions_unsupported: 0,
            },
            evidence: EvidenceSummary {
                total: 10,
                valid: 10,
                valid_finalized: 10,
                valid_quote_denominated: 10,
                invalid_attribution: 0,
                distinct_tokens: 3,
            },
            screening_coverage_bps: Some(10_000),
            primary: None,
        }
    }

    #[test]
    fn perfect_evidence_without_outcomes_scores_the_documented_value() {
        // 100 × (0.25 + 0.15 + 0.25 + 0.25·0.5 + 0.10) = 87.5
        let p = priority(&inputs());
        assert_eq!(p.score, Decimal::new(875_000, 4));
        assert_eq!(p.performance_status, "UNKNOWN");
    }

    #[test]
    fn recency_halves_every_half_life_and_unknown_is_zero() {
        let mut i = inputs();
        i.days_since_last_trade = Some(7);
        assert_eq!(priority(&i).recency, Decimal::new(5, 1));
        i.days_since_last_trade = Some(14);
        assert_eq!(priority(&i).recency, Decimal::new(25, 2));
        i.days_since_last_trade = None;
        assert_eq!(priority(&i).recency, Decimal::ZERO);
    }

    #[test]
    fn performance_shrinks_toward_neutral_with_few_outcomes() {
        let mut i = inputs();
        i.primary = Some((Denomination::Sol, 1, Some(Decimal::ONE)));
        // raw = 1, shrink = 1/11 → 0.5 + 0.5/11
        assert_eq!(priority(&i).performance, Decimal::new(545_455, 6));
        i.primary = Some((Denomination::Sol, 90, Some(Decimal::ONE)));
        assert_eq!(priority(&i).performance, Decimal::new(95, 2));
        i.primary = Some((Denomination::Sol, 90, Some(Decimal::from(-5))));
        assert_eq!(priority(&i).performance, Decimal::new(5, 2));
    }

    #[test]
    fn score_is_bounded() {
        let mut i = inputs();
        i.primary = Some((Denomination::Sol, 1000, Some(Decimal::from(1000))));
        let p = priority(&i);
        assert!(p.score <= Decimal::from(100));
        let z = priority(&PriorityInputs {
            swaps_30d: 0,
            days_since_last_trade: None,
            half_life_days: 7,
            coverage: Coverage::default(),
            evidence: EvidenceSummary::default(),
            screening_coverage_bps: None,
            primary: None,
        });
        // Only the neutral performance prior remains.
        assert_eq!(z.score, Decimal::new(125_000, 4));
    }

    #[test]
    fn eligibility_rules() {
        let ok = EvidenceSummary {
            total: 2,
            valid: 2,
            valid_finalized: 0,
            valid_quote_denominated: 2,
            ..EvidenceSummary::default()
        };
        assert_eq!(
            eligibility(&AccountRole::default(), &ok).0,
            el::PROVISIONAL_CANDIDATE
        );
        let fin = EvidenceSummary {
            valid_finalized: 1,
            ..ok.clone()
        };
        assert_eq!(
            eligibility(&AccountRole::default(), &fin).0,
            el::ELIGIBLE_CANDIDATE
        );
        let rot = EvidenceSummary {
            valid_quote_denominated: 0,
            ..ok.clone()
        };
        assert_eq!(
            eligibility(&AccountRole::default(), &rot).0,
            el::NEEDS_MORE_EVIDENCE
        );
        let gone = EvidenceSummary {
            total: 2,
            invalid_attribution: 2,
            ..EvidenceSummary::default()
        };
        assert_eq!(
            eligibility(&AccountRole::default(), &gone).0,
            el::AMBIGUOUS_ATTRIBUTION
        );
        let role = AccountRole {
            is_known_pool: true,
            ..AccountRole::default()
        };
        assert_eq!(eligibility(&role, &fin).0, el::KNOWN_INFRASTRUCTURE);
        let ex = AccountRole {
            operator_excluded: true,
            is_program: true,
            ..AccountRole::default()
        };
        assert_eq!(eligibility(&ex, &fin).0, el::EXCLUDED_BY_POLICY);
    }

    fn policy() -> QualificationPolicy {
        QualificationPolicy {
            min_swaps: 10,
            min_trading_days: 3,
            min_outcomes: 3,
            min_matched_coverage_bps: 6000,
            max_inactive_days: 7,
        }
    }

    fn qi() -> QualificationInputs {
        QualificationInputs {
            swaps_30d: 12,
            trading_days_30d: 4,
            days_since_last_trade: Some(1),
            stage_b_done: true,
            stage_c_done: false,
            screening: Some(ScreeningFacts {
                outcomes: 3,
                coverage_bps: Some(8000),
                primary_ratio: Some(Decimal::new(1, 1)),
            }),
        }
    }

    #[test]
    fn qualification_is_ordered_and_evidence_gated() {
        assert_eq!(qualify(&policy(), &qi()).0, q::PRELIMINARY_POSITIVE_RETURN);
        let mut i = qi();
        i.swaps_30d = 3;
        assert_eq!(qualify(&policy(), &i).0, q::INSUFFICIENT_ACTIVITY);
        let mut i = qi();
        i.days_since_last_trade = Some(30);
        assert_eq!(qualify(&policy(), &i).0, q::INSUFFICIENT_ACTIVITY);
        let mut i = qi();
        i.stage_b_done = false;
        assert_eq!(qualify(&policy(), &i).0, q::INSUFFICIENT_HISTORY);
        let mut i = qi();
        i.screening = Some(ScreeningFacts {
            outcomes: 3,
            coverage_bps: Some(5000),
            primary_ratio: Some(Decimal::ONE),
        });
        let (label, reasons) = qualify(&policy(), &i);
        assert_eq!(label, q::RECENT_ACTIVITY_QUALIFIED);
        assert_eq!(reasons, vec![q::INSUFFICIENT_FINANCIAL_EVIDENCE.to_owned()]);
        let mut i = qi();
        i.screening = Some(ScreeningFacts {
            outcomes: 5,
            coverage_bps: Some(9000),
            primary_ratio: Some(Decimal::new(-2, 1)),
        });
        assert_eq!(qualify(&policy(), &i).0, q::PRELIMINARY_NEGATIVE_RETURN);
        let mut i = qi();
        i.stage_c_done = true;
        let (label, reasons) = qualify(&policy(), &i);
        assert_eq!(label, q::READY_FOR_DEEPER_ANALYSIS);
        assert_eq!(reasons[0], "BASIS_PRELIMINARY_POSITIVE_RETURN");
    }
}
