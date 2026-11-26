//! Data freshness classification.
//!
//! Freshness is computed from a monotonic age and an explicit staleness
//! threshold owned by whoever produces the data. The UI renders these states
//! literally (`LIVE`, `STALE · 2m 14s`, `NOT INITIALIZED`); absent data must
//! never be presented as fresh.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataFreshness {
    /// The producer exists but has never delivered data.
    NotInitialized,
    /// Data is younger than the staleness threshold.
    Fresh,
    /// Data exists but is older than the staleness threshold.
    Stale,
}

impl DataFreshness {
    /// Classify data by its age. `None` means nothing has been received yet.
    #[must_use]
    pub fn classify(age: Option<Duration>, stale_after: Duration) -> Self {
        match age {
            None => Self::NotInitialized,
            Some(age) if age <= stale_after => Self::Fresh,
            Some(_) => Self::Stale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_threshold() {
        let t = Duration::from_secs(30);
        assert_eq!(
            DataFreshness::classify(None, t),
            DataFreshness::NotInitialized
        );
        assert_eq!(
            DataFreshness::classify(Some(Duration::from_secs(30)), t),
            DataFreshness::Fresh
        );
        assert_eq!(
            DataFreshness::classify(Some(Duration::from_secs(31)), t),
            DataFreshness::Stale
        );
    }
}
