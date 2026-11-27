//! Exact numeric decoding.
//!
//! Provider JSON numbers (Jupiter `usdPrice`, DEX Screener volumes) are read
//! from their raw JSON text with `serde_json::value::RawValue` and parsed
//! straight into `rust_decimal::Decimal`. They never pass through `f64`, so
//! no digit the provider sent is lost or invented.

use std::str::FromStr;

use rust_decimal::Decimal;
use serde_json::value::RawValue;

/// Parse a JSON number or numeric string (plain or scientific notation).
/// Returns `None` for `null`, non-numeric text, or values outside `Decimal`.
#[must_use]
pub fn decimal_from_raw(raw: &RawValue) -> Option<Decimal> {
    let text = raw.get().trim();
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    decimal_from_str(text)
}

#[must_use]
pub fn decimal_from_str(text: &str) -> Option<Decimal> {
    if text.is_empty() || text == "null" {
        return None;
    }
    if text.contains(['e', 'E']) {
        Decimal::from_scientific(text).ok()
    } else {
        Decimal::from_str(text).ok()
    }
}

/// `u64` from a JSON string or integer (amounts travel as strings).
#[must_use]
pub fn u64_from_value(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn raw(s: &str) -> Box<RawValue> {
        RawValue::from_string(s.to_owned()).unwrap()
    }

    #[test]
    fn keeps_every_digit_the_provider_sent() {
        // More significant digits than an f64 can hold.
        assert_eq!(
            decimal_from_raw(&raw("0.12345678901234567890123"))
                .unwrap()
                .to_string(),
            "0.12345678901234567890123"
        );
        assert_eq!(
            decimal_from_raw(&raw("\"147.0012\"")).unwrap().to_string(),
            "147.0012"
        );
        assert_eq!(
            decimal_from_raw(&raw("1.5e-7")).unwrap().to_string(),
            "0.00000015"
        );
        assert_eq!(decimal_from_raw(&raw("null")), None);
        assert_eq!(decimal_from_raw(&raw("\"abc\"")), None);
        assert_eq!(decimal_from_raw(&raw("1e400")), None);
    }

    #[test]
    fn amounts_accept_strings_and_integers() {
        assert_eq!(
            u64_from_value(&serde_json::json!("18446744073709551615")),
            Some(u64::MAX)
        );
        assert_eq!(u64_from_value(&serde_json::json!(5)), Some(5));
        assert_eq!(u64_from_value(&serde_json::json!(-5)), None);
        assert_eq!(u64_from_value(&serde_json::json!(1.5)), None);
    }
}
