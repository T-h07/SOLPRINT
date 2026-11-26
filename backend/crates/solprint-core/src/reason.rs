//! Reason codes.
//!
//! A reason code is a stable, machine-readable `UPPER_SNAKE_CASE` token that
//! explains a decision or state (`DATABASE_UNAVAILABLE`,
//! `LIVE_TRADING_NOT_PERMITTED`, later `LIQUIDITY_BELOW_MINIMUM`, ...).
//! Codes are part of the public contract: once emitted they are never renamed,
//! only deprecated. Human-readable messages may change freely.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("reason codes must be 1-64 chars of A-Z, 0-9 and '_', starting with a letter")]
pub struct ReasonCodeError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReasonCode(String);

impl ReasonCode {
    pub fn new(code: impl Into<String>) -> Result<Self, ReasonCodeError> {
        let code = code.into();
        let valid = (1..=64).contains(&code.len())
            && code.starts_with(|c: char| c.is_ascii_uppercase())
            && code
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        if valid {
            Ok(Self(code))
        } else {
            Err(ReasonCodeError)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ReasonCode {
    type Error = ReasonCodeError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ReasonCode> for String {
    fn from(value: ReasonCode) -> Self {
        value.0
    }
}

impl fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_format() {
        assert!(ReasonCode::new("DATABASE_UNAVAILABLE").is_ok());
        assert!(ReasonCode::new("E2E_OK").is_ok());
        for bad in [
            "",
            "lower",
            "_LEADING",
            "HAS-DASH",
            "SPACE X",
            &"A".repeat(65),
        ] {
            assert!(ReasonCode::new(bad).is_err(), "accepted {bad:?}");
        }
    }
}
