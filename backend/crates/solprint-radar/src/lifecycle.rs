//! Candidate lifecycle (`radar-lifecycle-1`): a pure decision from the
//! wallet's current state and evidence to its next state and the history
//! work it needs. Every decision carries its trigger and reasons, and the
//! registry records each change as a transition.
//!
//! Dormancy and archival are **coverage-aware**: a wallet is called dormant
//! or archived only when RADAR investigated its history after the inactivity
//! threshold had passed. Without that, the wallet is scheduled for a
//! dormancy recheck instead; silence in data RADAR never looked at is not
//! evidence of inactivity.

use crate::model::{Stage, eligibility, lifecycle as lc, qualification as q};
use crate::scoring::is_promising;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleInputs<'a> {
    pub current: &'a str,
    pub eligibility: &'a str,
    pub qualification: &'a str,
    /// Last completed history stage.
    pub stage_done: Option<Stage>,
    /// An investigation job (history stage or dormancy recheck) is open.
    pub job_open: bool,
    /// The open job is already running (leased or waiting for data).
    pub job_running: bool,
    pub valid_swaps_in_history: u32,
    pub days_since_last_trade: Option<i64>,
    /// Days between the last verified trade and the last investigation.
    pub investigated_days_after_last_trade: Option<i64>,
    /// New valid evidence arrived since the wallet was last evaluated.
    pub new_evidence: bool,
    pub dormant_after_days: u32,
    pub archive_after_days: u32,
    /// Stage A must show at least this many swaps to continue to stage B.
    pub min_swaps_for_stage_b: u32,
}

/// What the wallet needs next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Nothing,
    History(Stage),
    DormancyRecheck,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub lifecycle: &'static str,
    pub need: Need,
    pub trigger: &'static str,
    pub reasons: Vec<String>,
}

fn d(lifecycle: &'static str, need: Need, trigger: &'static str, reason: &str) -> Decision {
    Decision {
        lifecycle,
        need,
        trigger,
        reasons: vec![reason.to_owned()],
    }
}

fn as_static(s: &str) -> &'static str {
    lc::ALL
        .into_iter()
        .find(|x| *x == s)
        .unwrap_or(lc::DISCOVERED)
}

/// The next stage after `done`, if any.
#[must_use]
pub const fn next_stage(done: Option<Stage>) -> Option<Stage> {
    match done {
        None => Some(Stage::A),
        Some(Stage::A) => Some(Stage::B),
        Some(Stage::B) => Some(Stage::C),
        Some(Stage::C) => None,
    }
}

/// Decide the next lifecycle state.
#[must_use]
pub fn decide(i: &LifecycleInputs<'_>) -> Decision {
    let cur = as_static(i.current);
    let quiet_for = |days: u32| {
        i.days_since_last_trade
            .is_some_and(|x| x >= i64::from(days))
    };
    let established_after = |days: u32| {
        i.investigated_days_after_last_trade
            .is_some_and(|x| x >= i64::from(days))
    };

    // Policy exclusions and non-trader roles end the investigation.
    if !eligibility::investigable(i.eligibility) {
        if cur == lc::REJECTED || cur == lc::ARCHIVED {
            return d(cur, Need::Nothing, "NO_CHANGE", i.eligibility);
        }
        return d(lc::REJECTED, Need::Nothing, "ELIGIBILITY", i.eligibility);
    }

    // Reactivation: new activity after dormancy, archival or a negative
    // preliminary result reopens the case (history already done is reused).
    if matches!(cur, lc::DORMANT | lc::ARCHIVED | lc::REJECTED) {
        let recent = i
            .days_since_last_trade
            .is_some_and(|x| x < i64::from(i.dormant_after_days));
        if i.new_evidence && recent {
            return d(
                lc::DISCOVERED,
                Need::Nothing,
                "REACTIVATED",
                "NEW_VERIFIED_ACTIVITY",
            );
        }
        if cur == lc::DORMANT && quiet_for(i.archive_after_days) {
            if established_after(i.archive_after_days) {
                return d(
                    lc::ARCHIVED,
                    Need::Nothing,
                    "INACTIVITY",
                    "ARCHIVE_THRESHOLD_REACHED",
                );
            }
            let need = if i.job_open {
                Need::Nothing
            } else {
                Need::DormancyRecheck
            };
            return d(cur, need, "COVERAGE_CHECK", "INACTIVITY_NOT_ESTABLISHED");
        }
        return d(cur, Need::Nothing, "NO_CHANGE", "TERMINAL_OR_RESTING");
    }

    // Dormancy, only once inactivity is established by coverage.
    if quiet_for(i.dormant_after_days) && i.stage_done.is_some() {
        if established_after(i.dormant_after_days) {
            return d(
                lc::DORMANT,
                Need::Nothing,
                "INACTIVITY",
                "DORMANCY_THRESHOLD_REACHED",
            );
        }
        if !i.job_open {
            return d(
                cur,
                Need::DormancyRecheck,
                "COVERAGE_CHECK",
                "INACTIVITY_NOT_ESTABLISHED",
            );
        }
    }

    if i.job_open {
        // A preliminarily qualified wallet keeps that verdict visible while
        // its stage C history is fetched.
        let state = if cur == lc::PRELIMINARY_QUALIFIED {
            lc::PRELIMINARY_QUALIFIED
        } else if i.job_running {
            lc::INVESTIGATING
        } else {
            lc::HISTORY_QUEUED
        };
        return d(state, Need::Nothing, "INVESTIGATION", "JOB_OPEN");
    }

    match i.stage_done {
        None => d(
            lc::DISCOVERED,
            Need::History(Stage::A),
            "SCREENING",
            "STAGE_A_REQUIRED",
        ),
        Some(Stage::A) => {
            if i.valid_swaps_in_history >= i.min_swaps_for_stage_b {
                d(
                    lc::INVESTIGATING,
                    Need::History(Stage::B),
                    "STAGE_A_COMPLETE",
                    "ENOUGH_SWAPS_FOR_STAGE_B",
                )
            } else {
                d(
                    lc::INSUFFICIENT_EVIDENCE,
                    Need::Nothing,
                    "STAGE_A_COMPLETE",
                    "FEW_VERIFIED_SWAPS",
                )
            }
        }
        Some(Stage::B) => {
            if i.qualification == q::PRELIMINARY_NEGATIVE_RETURN {
                d(
                    lc::REJECTED,
                    Need::Nothing,
                    "STAGE_B_COMPLETE",
                    q::PRELIMINARY_NEGATIVE_RETURN,
                )
            } else if is_promising(i.qualification) {
                d(
                    lc::PRELIMINARY_QUALIFIED,
                    Need::History(Stage::C),
                    "STAGE_B_COMPLETE",
                    i.qualification,
                )
            } else {
                d(
                    lc::INSUFFICIENT_EVIDENCE,
                    Need::Nothing,
                    "STAGE_B_COMPLETE",
                    i.qualification,
                )
            }
        }
        Some(Stage::C) => {
            if i.qualification == q::PRELIMINARY_NEGATIVE_RETURN {
                d(
                    lc::REJECTED,
                    Need::Nothing,
                    "STAGE_C_COMPLETE",
                    q::PRELIMINARY_NEGATIVE_RETURN,
                )
            } else if is_promising(i.qualification) {
                // Monitoring assignment moves PRIORITIZED → MONITORED.
                let keep = if cur == lc::MONITORED {
                    lc::MONITORED
                } else {
                    lc::PRIORITIZED
                };
                d(keep, Need::Nothing, "STAGE_C_COMPLETE", i.qualification)
            } else {
                d(
                    lc::INSUFFICIENT_EVIDENCE,
                    Need::Nothing,
                    "STAGE_C_COMPLETE",
                    i.qualification,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base<'a>() -> LifecycleInputs<'a> {
        LifecycleInputs {
            current: lc::DISCOVERED,
            eligibility: eligibility::ELIGIBLE_CANDIDATE,
            qualification: q::INSUFFICIENT_HISTORY,
            stage_done: None,
            job_open: false,
            job_running: false,
            valid_swaps_in_history: 1,
            days_since_last_trade: Some(0),
            investigated_days_after_last_trade: Some(0),
            new_evidence: false,
            dormant_after_days: 14,
            archive_after_days: 60,
            min_swaps_for_stage_b: 3,
        }
    }

    #[test]
    fn progressive_stages() {
        let i = base();
        assert_eq!(decide(&i).need, Need::History(Stage::A));
        let mut i = base();
        i.job_open = true;
        assert_eq!(decide(&i).lifecycle, lc::HISTORY_QUEUED);
        i.job_running = true;
        assert_eq!(decide(&i).lifecycle, lc::INVESTIGATING);
        let mut i = base();
        i.stage_done = Some(Stage::A);
        assert_eq!(decide(&i).lifecycle, lc::INSUFFICIENT_EVIDENCE);
        i.valid_swaps_in_history = 3;
        assert_eq!(decide(&i).need, Need::History(Stage::B));
        let mut i = base();
        i.stage_done = Some(Stage::B);
        i.qualification = q::PRELIMINARY_POSITIVE_RETURN;
        let r = decide(&i);
        assert_eq!(
            (r.lifecycle, r.need),
            (lc::PRELIMINARY_QUALIFIED, Need::History(Stage::C))
        );
        i.qualification = q::PRELIMINARY_NEGATIVE_RETURN;
        assert_eq!(decide(&i).lifecycle, lc::REJECTED);
        let mut i = base();
        i.stage_done = Some(Stage::C);
        i.qualification = q::READY_FOR_DEEPER_ANALYSIS;
        assert_eq!(decide(&i).lifecycle, lc::PRIORITIZED);
        i.current = lc::MONITORED;
        assert_eq!(decide(&i).lifecycle, lc::MONITORED);
    }

    #[test]
    fn dormancy_requires_established_inactivity() {
        let mut i = base();
        i.current = lc::MONITORED;
        i.stage_done = Some(Stage::C);
        i.qualification = q::READY_FOR_DEEPER_ANALYSIS;
        i.days_since_last_trade = Some(20);
        i.investigated_days_after_last_trade = Some(2);
        let r = decide(&i);
        assert_eq!(
            (r.lifecycle, r.need),
            (lc::MONITORED, Need::DormancyRecheck)
        );
        i.investigated_days_after_last_trade = Some(15);
        assert_eq!(decide(&i).lifecycle, lc::DORMANT);
    }

    #[test]
    fn archival_and_reactivation() {
        let mut i = base();
        i.current = lc::DORMANT;
        i.stage_done = Some(Stage::C);
        i.days_since_last_trade = Some(70);
        i.investigated_days_after_last_trade = Some(30);
        assert_eq!(decide(&i).need, Need::DormancyRecheck);
        i.investigated_days_after_last_trade = Some(61);
        assert_eq!(decide(&i).lifecycle, lc::ARCHIVED);
        i.current = lc::ARCHIVED;
        i.new_evidence = true;
        i.days_since_last_trade = Some(1);
        let r = decide(&i);
        assert_eq!((r.lifecycle, r.trigger), (lc::DISCOVERED, "REACTIVATED"));
    }

    #[test]
    fn ineligible_wallets_are_rejected_once() {
        let mut i = base();
        i.eligibility = eligibility::KNOWN_INFRASTRUCTURE;
        assert_eq!(decide(&i).lifecycle, lc::REJECTED);
        i.current = lc::REJECTED;
        assert_eq!(decide(&i).trigger, "NO_CHANGE");
    }
}
