//! Monitoring rotation with hysteresis. Live monitoring slots are scarce
//! (subscription ceilings, credits), so RADAR assigns them to the highest-
//! priority qualified wallets, but avoids churn: an assignment is kept for a
//! minimum duration, a released wallet waits a cooldown before it can return,
//! a challenger must beat the weakest incumbent by a margin, and at most one
//! replacement happens per pass.

use std::time::Duration;

use rust_decimal::Decimal;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub address: String,
    pub priority: Decimal,
    pub started_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub address: String,
    pub priority: Decimal,
    pub last_ended_at: Option<OffsetDateTime>,
    /// No verified trade within the reduce-monitoring window (or none
    /// known): released from monitoring and never started.
    pub quiet: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationPolicy {
    pub capacity: usize,
    pub min_duration: Duration,
    pub cooldown: Duration,
    /// Score points a challenger must exceed the weakest incumbent by.
    pub margin: Decimal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub start: Vec<String>,
    /// `(address, reason)`.
    pub stop: Vec<(String, &'static str)>,
}

/// Plan one rotation pass. `current` assignments whose wallet is no longer a
/// candidate are released at once (`NO_LONGER_QUALIFIED`); quiet wallets are
/// released (`REDUCED_ACTIVITY`) regardless of tenure, so inactive wallets
/// leave the expensive workload before dormancy.
#[must_use]
pub fn rotate(
    current: &[Assignment],
    candidates: &[Candidate],
    p: &RotationPolicy,
    now: OffsetDateTime,
) -> Plan {
    let mut plan = Plan::default();
    let mut kept: Vec<&Assignment> = Vec::new();
    for a in current {
        match candidates.iter().find(|c| c.address == a.address) {
            Some(c) if c.quiet => plan.stop.push((a.address.clone(), "REDUCED_ACTIVITY")),
            Some(_) => kept.push(a),
            None => plan.stop.push((a.address.clone(), "NO_LONGER_QUALIFIED")),
        }
    }
    // Over capacity (the ceiling shrank): release the weakest first.
    kept.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.address.cmp(&b.address)));
    while kept.len() > p.capacity {
        if let Some(a) = kept.pop() {
            plan.stop.push((a.address.clone(), "CAPACITY_REDUCED"));
        }
    }
    let mut challengers: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| !c.quiet && !kept.iter().any(|a| a.address == c.address))
        .filter(|c| c.last_ended_at.is_none_or(|e| now - e >= p.cooldown))
        .collect();
    challengers.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.address.cmp(&b.address)));
    let mut next = challengers.into_iter();
    let mut filled = kept.len();
    while filled < p.capacity {
        let Some(c) = next.next() else { break };
        plan.start.push(c.address.clone());
        filled += 1;
    }
    if filled >= p.capacity
        && let (Some(best), Some(weakest)) = (next.next(), kept.last())
    {
        let tenure_ok = now - weakest.started_at >= p.min_duration;
        let current_priority = candidates
            .iter()
            .find(|c| c.address == weakest.address)
            .map_or(weakest.priority, |c| c.priority);
        if tenure_ok && best.priority > current_priority + p.margin {
            plan.stop.push((weakest.address.clone(), "ROTATED_OUT"));
            plan.start.push(best.address.clone());
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    fn pol(cap: usize) -> RotationPolicy {
        RotationPolicy {
            capacity: cap,
            min_duration: Duration::from_secs(3600),
            cooldown: Duration::from_secs(3600),
            margin: Decimal::from(5),
        }
    }

    fn cand(a: &str, p: i64) -> Candidate {
        Candidate {
            address: a.to_owned(),
            priority: Decimal::from(p),
            last_ended_at: None,
            quiet: false,
        }
    }

    fn asg(a: &str, p: i64, age_secs: u64) -> Assignment {
        Assignment {
            address: a.to_owned(),
            priority: Decimal::from(p),
            started_at: now() - Duration::from_secs(age_secs),
        }
    }

    #[test]
    fn fills_free_slots_by_priority() {
        let plan = rotate(
            &[],
            &[cand("a", 10), cand("b", 50), cand("c", 30)],
            &pol(2),
            now(),
        );
        assert_eq!(plan.start, vec!["b".to_owned(), "c".to_owned()]);
        assert!(plan.stop.is_empty());
    }

    #[test]
    fn hysteresis_margin_and_minimum_tenure() {
        let cur = [asg("a", 60, 7200), asg("b", 40, 7200)];
        // Challenger 44 does not beat 40 + 5.
        let p = rotate(
            &cur,
            &[cand("a", 60), cand("b", 40), cand("c", 44)],
            &pol(2),
            now(),
        );
        assert!(p.start.is_empty() && p.stop.is_empty());
        // 46 does: one swap.
        let p = rotate(
            &cur,
            &[cand("a", 60), cand("b", 40), cand("c", 46), cand("d", 90)],
            &pol(2),
            now(),
        );
        assert_eq!(p.stop, vec![("b".to_owned(), "ROTATED_OUT")]);
        assert_eq!(p.start, vec!["d".to_owned()]);
        // Too young to rotate.
        let young = [asg("a", 60, 7200), asg("b", 40, 60)];
        let p = rotate(
            &young,
            &[cand("a", 60), cand("b", 40), cand("d", 90)],
            &pol(2),
            now(),
        );
        assert!(p.start.is_empty());
    }

    #[test]
    fn cooldown_and_disqualification() {
        let mut c = cand("c", 99);
        c.last_ended_at = Some(now() - Duration::from_secs(60));
        let p = rotate(&[asg("a", 50, 10)], &[c], &pol(2), now());
        assert_eq!(p.stop, vec![("a".to_owned(), "NO_LONGER_QUALIFIED")]);
        assert!(p.start.is_empty());
    }

    #[test]
    fn capacity_reduction_releases_weakest() {
        let cur = [asg("a", 60, 10), asg("b", 40, 10)];
        let p = rotate(&cur, &[cand("a", 60), cand("b", 40)], &pol(1), now());
        assert_eq!(p.stop, vec![("b".to_owned(), "CAPACITY_REDUCED")]);
        let p = rotate(&cur, &[cand("a", 60), cand("b", 40)], &pol(0), now());
        assert_eq!(p.stop.len(), 2);
    }

    #[test]
    fn quiet_wallets_leave_monitoring_and_are_not_started() {
        let mut quiet = cand("a", 90);
        quiet.quiet = true;
        let mut quiet_challenger = cand("c", 99);
        quiet_challenger.quiet = true;
        // Released even before the minimum tenure; the quiet challenger is skipped.
        let p = rotate(
            &[asg("a", 90, 10)],
            &[quiet, quiet_challenger, cand("b", 20)],
            &pol(1),
            now(),
        );
        assert_eq!(p.stop, vec![("a".to_owned(), "REDUCED_ACTIVITY")]);
        assert_eq!(p.start, vec!["b".to_owned()]);
    }
}
