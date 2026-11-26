//! Trading mode.
//!
//! The backend is the single authority on the operating mode. `Live` exists
//! as a domain concept so that future phases can reason about it, but no code
//! path in Phase 00 can activate it: configuration rejects it (see
//! `solprint-config`) and there is no signing or submission capability.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradingMode {
    /// Simulation only. No transaction is ever signed or submitted.
    Paper,
    /// Real execution with real funds. Not permitted before Phase 11.
    Live,
}

impl TradingMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "PAPER",
            Self::Live => "LIVE",
        }
    }

    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }
}

impl fmt::Display for TradingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown trading mode (expected `paper` or `live`)")]
pub struct TradingModeParseError;

impl FromStr for TradingMode {
    type Err = TradingModeParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "paper" => Ok(Self::Paper),
            "live" => Ok(Self::Live),
            _ => Err(TradingModeParseError),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitively_and_rejects_unknown() {
        assert_eq!("PAPER".parse::<TradingMode>().unwrap(), TradingMode::Paper);
        assert_eq!(" live ".parse::<TradingMode>().unwrap(), TradingMode::Live);
        assert!("simulate".parse::<TradingMode>().is_err());
        assert!("".parse::<TradingMode>().is_err());
    }

    #[test]
    fn serializes_upper_case() {
        assert_eq!(
            serde_json::to_string(&TradingMode::Paper).unwrap(),
            "\"PAPER\""
        );
    }
}
