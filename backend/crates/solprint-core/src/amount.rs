//! Financial amount primitives.
//!
//! * [`Lamports`] — native SOL in base units (`u64`, 1 SOL = 10^9 lamports).
//! * [`TokenAmount`] — SPL token amount in base units plus its mint decimals.
//! * [`BasisPoints`] — a bounded ratio in 1/10 000 units.
//! * [`Price`] — fixed-precision decimal price (never `f64`).
//!
//! All arithmetic is checked: overflow, underflow and precision loss return
//! an [`AmountError`] instead of wrapping, saturating or truncating silently.
//! Amounts serialize as strings because `u64` exceeds JavaScript's safe
//! integer range (2^53 - 1).

use std::fmt;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Errors produced by checked financial arithmetic and parsing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AmountError {
    #[error("arithmetic overflow")]
    Overflow,
    #[error("arithmetic underflow")]
    Underflow,
    #[error("token decimals mismatch: {left} vs {right}")]
    DecimalsMismatch { left: u8, right: u8 },
    #[error("token decimals {0} exceeds the supported maximum")]
    UnsupportedDecimals(u8),
    #[error("value has more fractional digits than the token supports ({decimals})")]
    PrecisionLoss { decimals: u8 },
    #[error("invalid amount literal")]
    InvalidLiteral,
    #[error("negative amounts are not representable")]
    Negative,
    #[error("basis points {0} out of range 0..=10000")]
    BasisPointsOutOfRange(u32),
}

/// Explicit rounding policy for any operation that can produce a fraction of
/// a base unit. There is no implicit default: callers must choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// Toward zero. Use when the result is an amount we *receive* and must not
    /// be overstated.
    Down,
    /// Away from zero. Use when the result is an amount we *pay* (fees,
    /// slippage budgets) and must not be understated.
    Up,
}

/// Native SOL amount in lamports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Lamports(u64);

impl Lamports {
    pub const ZERO: Self = Self(0);
    pub const PER_SOL: u64 = 1_000_000_000;

    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(AmountError::Underflow)
    }

    pub fn checked_mul(self, factor: u64) -> Result<Self, AmountError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    /// Exact SOL value as a decimal (9 fractional digits). For display and
    /// pricing only; accounting stays in lamports.
    #[must_use]
    pub fn to_sol_decimal(self) -> Decimal {
        Decimal::from_i128_with_scale(i128::from(self.0), 9)
    }
}

impl fmt::Display for Lamports {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for Lamports {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Lamports {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        parse_u64_strict(&raw)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// Parse a canonical unsigned integer: digits only, no sign, no whitespace,
/// no leading zeros (except "0"). Rejecting non-canonical forms keeps
/// round-trips exact and avoids ambiguous inputs.
fn parse_u64_strict(raw: &str) -> Result<u64, AmountError> {
    let canonical = !raw.is_empty()
        && raw.bytes().all(|b| b.is_ascii_digit())
        && (raw == "0" || !raw.starts_with('0'));
    if !canonical {
        return Err(AmountError::InvalidLiteral);
    }
    raw.parse::<u64>().map_err(|_| AmountError::Overflow)
}

/// SPL token amount: raw base units plus the mint's decimals.
///
/// Two amounts can only be combined when their decimals match; mixing mints is
/// prevented at a higher level, mixing precisions is prevented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenAmount {
    #[serde(with = "u64_string")]
    raw: u64,
    decimals: u8,
}

impl TokenAmount {
    /// SPL mints store decimals as `u8`; `Decimal` supports a scale of at most 28.
    pub const MAX_DECIMALS: u8 = 28;

    pub fn new(raw: u64, decimals: u8) -> Result<Self, AmountError> {
        if decimals > Self::MAX_DECIMALS {
            return Err(AmountError::UnsupportedDecimals(decimals));
        }
        Ok(Self { raw, decimals })
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.raw
    }

    #[must_use]
    pub const fn decimals(self) -> u8 {
        self.decimals
    }

    /// Parse a human ("UI") amount such as `"12.5"` into base units.
    /// Fails rather than truncating when the literal carries more significant
    /// fractional digits than the token supports.
    pub fn from_ui_str(literal: &str, decimals: u8) -> Result<Self, AmountError> {
        if decimals > Self::MAX_DECIMALS {
            return Err(AmountError::UnsupportedDecimals(decimals));
        }
        let value = Decimal::from_str_exact(literal).map_err(|_| AmountError::InvalidLiteral)?;
        if value.is_sign_negative() && !value.is_zero() {
            return Err(AmountError::Negative);
        }
        // Trailing zeros are allowed ("1.500" for a 2-decimal token).
        let value = value.normalize();
        if value.scale() > u32::from(decimals) {
            return Err(AmountError::PrecisionLoss { decimals });
        }
        // value = mantissa * 10^-scale  ->  raw = mantissa * 10^(decimals - scale)
        let shift = u32::from(decimals) - value.scale();
        let factor = 10_u128.checked_pow(shift).ok_or(AmountError::Overflow)?;
        let mantissa = u128::try_from(value.mantissa()).map_err(|_| AmountError::Negative)?;
        let raw = mantissa.checked_mul(factor).ok_or(AmountError::Overflow)?;
        let raw = u64::try_from(raw).map_err(|_| AmountError::Overflow)?;
        Self::new(raw, decimals)
    }

    /// Exact UI value as a decimal.
    #[must_use]
    pub fn to_decimal(self) -> Decimal {
        Decimal::from_i128_with_scale(i128::from(self.raw), u32::from(self.decimals))
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self, AmountError> {
        self.same_decimals(rhs)?;
        let raw = self.raw.checked_add(rhs.raw).ok_or(AmountError::Overflow)?;
        Ok(Self { raw, ..self })
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self, AmountError> {
        self.same_decimals(rhs)?;
        let raw = self
            .raw
            .checked_sub(rhs.raw)
            .ok_or(AmountError::Underflow)?;
        Ok(Self { raw, ..self })
    }

    fn same_decimals(self, rhs: Self) -> Result<(), AmountError> {
        if self.decimals == rhs.decimals {
            Ok(())
        } else {
            Err(AmountError::DecimalsMismatch {
                left: self.decimals,
                right: rhs.decimals,
            })
        }
    }
}

impl fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_decimal())
    }
}

/// Ratio in basis points, bounded to `0..=10_000` (0 % to 100 %).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct BasisPoints(u16);

impl BasisPoints {
    pub const MAX: u16 = 10_000;
    pub const ZERO: Self = Self(0);
    pub const FULL: Self = Self(Self::MAX);

    pub fn new(value: u32) -> Result<Self, AmountError> {
        match u16::try_from(value) {
            Ok(v) if v <= Self::MAX => Ok(Self(v)),
            _ => Err(AmountError::BasisPointsOutOfRange(value)),
        }
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Apply this ratio to an amount with an explicit rounding policy.
    /// Computed in `u128`, so the intermediate product cannot overflow.
    pub fn apply_to_lamports(
        self,
        amount: Lamports,
        rounding: Rounding,
    ) -> Result<Lamports, AmountError> {
        let numerator = u128::from(amount.get()) * u128::from(self.0);
        let denominator = u128::from(Self::MAX);
        let mut quotient = numerator / denominator;
        if rounding == Rounding::Up && numerator % denominator != 0 {
            quotient += 1;
        }
        u64::try_from(quotient)
            .map(Lamports::new)
            .map_err(|_| AmountError::Overflow)
    }
}

impl TryFrom<u32> for BasisPoints {
    type Error = AmountError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<BasisPoints> for u32 {
    fn from(value: BasisPoints) -> Self {
        u32::from(value.0)
    }
}

/// A non-negative fixed-precision price (e.g. USD per token, SOL per token).
/// Serialized as a decimal string so no precision is lost in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "Decimal", into = "Decimal")]
pub struct Price(Decimal);

impl Price {
    pub fn new(value: Decimal) -> Result<Self, AmountError> {
        if value.is_sign_negative() && !value.is_zero() {
            Err(AmountError::Negative)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> Decimal {
        self.0
    }
}

impl TryFrom<Decimal> for Price {
    type Error = AmountError;
    fn try_from(value: Decimal) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Price> for Decimal {
    fn from(value: Price) -> Self {
        value.0
    }
}

impl FromStr for Price {
    type Err = AmountError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Decimal::from_str_exact(s)
            .map_err(|_| AmountError::InvalidLiteral)
            .and_then(Self::new)
    }
}

mod u64_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let raw = String::deserialize(deserializer)?;
        super::parse_u64_strict(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn lamports_checked_arithmetic_rejects_overflow_and_underflow() {
        let max = Lamports::new(u64::MAX);
        assert_eq!(
            max.checked_add(Lamports::new(1)),
            Err(AmountError::Overflow)
        );
        assert_eq!(
            Lamports::ZERO.checked_sub(Lamports::new(1)),
            Err(AmountError::Underflow)
        );
        assert_eq!(max.checked_mul(2), Err(AmountError::Overflow));
        assert_eq!(
            Lamports::new(2).checked_add(Lamports::new(3)),
            Ok(Lamports::new(5))
        );
    }

    #[test]
    fn lamports_serialize_as_string_and_preserve_u64_max() {
        let json = serde_json::to_string(&Lamports::new(u64::MAX)).unwrap();
        assert_eq!(json, "\"18446744073709551615\"");
        let back: Lamports = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get(), u64::MAX);
    }

    #[test]
    fn lamports_reject_non_canonical_and_numeric_json() {
        for bad in [
            "\"-1\"",
            "\"01\"",
            "\" 1\"",
            "\"1.0\"",
            "\"\"",
            "1",
            "\"18446744073709551616\"",
        ] {
            assert!(
                serde_json::from_str::<Lamports>(bad).is_err(),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn lamports_to_sol_is_exact() {
        assert_eq!(
            Lamports::new(1_500_000_001).to_sol_decimal().to_string(),
            "1.500000001"
        );
    }

    #[test]
    fn token_amount_parses_ui_literals_exactly() {
        let a = TokenAmount::from_ui_str("12.345678", 6).unwrap();
        assert_eq!(a.raw(), 12_345_678);
        assert_eq!(a.to_decimal().to_string(), "12.345678");
        assert_eq!(TokenAmount::from_ui_str("1.500", 2).unwrap().raw(), 150);
        assert_eq!(TokenAmount::from_ui_str("0", 9).unwrap().raw(), 0);
        assert_eq!(
            TokenAmount::from_ui_str("100", 9).unwrap().raw(),
            100_000_000_000
        );
    }

    #[test]
    fn token_amount_refuses_silent_truncation() {
        assert_eq!(
            TokenAmount::from_ui_str("1.0000001", 6),
            Err(AmountError::PrecisionLoss { decimals: 6 })
        );
        assert_eq!(
            TokenAmount::from_ui_str("-1", 6),
            Err(AmountError::Negative)
        );
        assert_eq!(
            TokenAmount::from_ui_str("abc", 6),
            Err(AmountError::InvalidLiteral)
        );
        assert_eq!(
            TokenAmount::from_ui_str("1e3", 6),
            Err(AmountError::InvalidLiteral)
        );
    }

    #[test]
    fn token_amount_rejects_values_beyond_u64() {
        assert_eq!(
            TokenAmount::from_ui_str("18446744073709551616", 0),
            Err(AmountError::Overflow)
        );
        assert_eq!(
            TokenAmount::from_ui_str("18446744073709.551616", 6),
            Err(AmountError::Overflow)
        );
        assert_eq!(
            TokenAmount::from_ui_str("18446744073709.551615", 6)
                .unwrap()
                .raw(),
            u64::MAX
        );
    }

    #[test]
    fn token_amount_arithmetic_requires_matching_decimals() {
        let a = TokenAmount::new(10, 6).unwrap();
        let b = TokenAmount::new(10, 9).unwrap();
        assert_eq!(
            a.checked_add(b),
            Err(AmountError::DecimalsMismatch { left: 6, right: 9 })
        );
        assert_eq!(
            a.checked_sub(TokenAmount::new(11, 6).unwrap()),
            Err(AmountError::Underflow)
        );
        assert_eq!(a.checked_add(a).unwrap().raw(), 20);
        assert_eq!(
            TokenAmount::new(1, 29),
            Err(AmountError::UnsupportedDecimals(29))
        );
    }

    #[test]
    fn token_amount_serializes_raw_as_string() {
        let json = serde_json::to_string(&TokenAmount::new(u64::MAX, 6).unwrap()).unwrap();
        assert_eq!(json, r#"{"raw":"18446744073709551615","decimals":6}"#);
        let back: TokenAmount = serde_json::from_str(&json).unwrap();
        assert_eq!(back.raw(), u64::MAX);
    }

    #[test]
    fn basis_points_are_bounded_and_round_explicitly() {
        assert!(BasisPoints::new(10_001).is_err());
        assert!(serde_json::from_str::<BasisPoints>("10001").is_err());
        let fee = BasisPoints::new(25).unwrap(); // 0.25 %
        let amount = Lamports::new(1_001);
        assert_eq!(
            fee.apply_to_lamports(amount, Rounding::Down).unwrap(),
            Lamports::new(2)
        );
        assert_eq!(
            fee.apply_to_lamports(amount, Rounding::Up).unwrap(),
            Lamports::new(3)
        );
        assert_eq!(
            BasisPoints::FULL
                .apply_to_lamports(Lamports::new(u64::MAX), Rounding::Up)
                .unwrap(),
            Lamports::new(u64::MAX)
        );
        assert_eq!(
            BasisPoints::ZERO
                .apply_to_lamports(amount, Rounding::Up)
                .unwrap(),
            Lamports::ZERO
        );
    }

    #[test]
    fn price_is_decimal_string_and_non_negative() {
        let p: Price = "0.000012345678901234".parse().unwrap();
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            "\"0.000012345678901234\""
        );
        assert!("-1".parse::<Price>().is_err());
        assert!(serde_json::from_str::<Price>("\"-0.5\"").is_err());
    }
}
