//! RADAR budget admission. RADAR never creates credits: it may use at most a
//! configured share of the provider's **currently remaining normal
//! allowance**, minus work it already has in flight, and it stops provider
//! work entirely when GATEWAY's budget state is degraded or unknown.
//! GATEWAY still admits (or denies) every individual request.

/// GATEWAY budget as RADAR reads it (local state only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetView {
    pub state: String,
    pub normal_credits: u64,
    pub credits_used: u64,
    pub ledger_loaded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Provider discovery, sampling and every history stage.
    Full,
    /// Conservation: local discovery sources only; samples of locally
    /// discovered markets and stage-A history only.
    Conservation,
    /// No provider work (local evaluation continues).
    Paused(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admission {
    pub mode: Mode,
    /// RADAR's remaining allowance after outstanding estimates.
    pub available: u64,
    pub allowance: u64,
}

impl Admission {
    #[must_use]
    pub const fn admits(&self, estimate: u64) -> bool {
        !matches!(self.mode, Mode::Paused(_)) && estimate <= self.available
    }

    #[must_use]
    pub const fn reason(&self) -> Option<&'static str> {
        match self.mode {
            Mode::Paused(r) => Some(r),
            _ => None,
        }
    }
}

/// Decide RADAR's provider allowance.
#[must_use]
pub fn admission(view: Option<&BudgetView>, share_bps: u32, outstanding: u64) -> Admission {
    let paused = |r: &'static str| Admission {
        mode: Mode::Paused(r),
        available: 0,
        allowance: 0,
    };
    let Some(v) = view else {
        return paused("BUDGET_UNKNOWN");
    };
    if !v.ledger_loaded {
        return paused("BUDGET_LEDGER_NOT_LOADED");
    }
    let mode = match v.state.as_str() {
        "NORMAL" => Mode::Full,
        "CONSERVATION" => Mode::Conservation,
        "CRITICAL" => return paused("BUDGET_CRITICAL"),
        "NORMAL_BUDGET_EXHAUSTED" => return paused("NORMAL_BUDGET_EXHAUSTED"),
        "HARD_LIMIT_REACHED" => return paused("HARD_LIMIT_REACHED"),
        _ => return paused("ACCOUNT_USAGE_UNKNOWN"),
    };
    let remaining = v.normal_credits.saturating_sub(v.credits_used);
    let allowance =
        u64::try_from(u128::from(remaining) * u128::from(share_bps.min(10_000)) / 10_000)
            .unwrap_or(u64::MAX);
    Admission {
        mode,
        available: allowance.saturating_sub(outstanding),
        allowance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(state: &str) -> BudgetView {
        BudgetView {
            state: state.to_owned(),
            normal_credits: 1000,
            credits_used: 500,
            ledger_loaded: true,
        }
    }

    #[test]
    fn share_of_remaining_minus_outstanding() {
        let a = admission(Some(&view("NORMAL")), 4000, 50);
        assert_eq!((a.allowance, a.available), (200, 150));
        assert!(a.admits(150));
        assert!(!a.admits(151));
    }

    #[test]
    fn degraded_or_unknown_budgets_pause() {
        assert_eq!(admission(None, 4000, 0).reason(), Some("BUDGET_UNKNOWN"));
        let mut v = view("NORMAL");
        v.ledger_loaded = false;
        assert!(!admission(Some(&v), 4000, 0).admits(0));
        for s in [
            "CRITICAL",
            "NORMAL_BUDGET_EXHAUSTED",
            "HARD_LIMIT_REACHED",
            "ACCOUNT_USAGE_UNKNOWN",
        ] {
            assert!(admission(Some(&view(s)), 4000, 0).reason().is_some(), "{s}");
        }
        assert_eq!(
            admission(Some(&view("CONSERVATION")), 4000, 0).mode,
            Mode::Conservation
        );
    }

    #[test]
    fn exhausted_allowance_admits_nothing_costly() {
        let mut v = view("NORMAL");
        v.credits_used = 2000;
        let a = admission(Some(&v), 4000, 0);
        assert_eq!(a.available, 0);
        assert!(!a.admits(1));
    }
}
