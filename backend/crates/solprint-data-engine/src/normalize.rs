//! Exact decimal arithmetic for normalized swap values. No floating point:
//! amounts are integers, ratios and values are `Decimal` with an explicit
//! scale and rounding direction (toward zero).

use rust_decimal::{Decimal, RoundingStrategy};

/// Scale of stored ratios and prices (NUMERIC(48,18)).
pub const RATIO_SCALE: u32 = 18;

fn pow10(exp: u32) -> Option<Decimal> {
    let mut d = Decimal::ONE;
    for _ in 0..exp {
        d = d.checked_mul(Decimal::TEN)?;
    }
    Some(d)
}

/// UI-unit amount: `raw / 10^decimals`, exact (Decimal supports scale ≤ 28).
#[must_use]
pub fn ui_amount(raw: u64, decimals: u8) -> Option<Decimal> {
    let d = u32::from(decimals);
    if d > 28 {
        return Decimal::from(raw).checked_div(pow10(d)?);
    }
    Some(Decimal::from_i128_with_scale(i128::from(raw), d))
}

/// Output received per one unit of input, in UI units, rounded toward zero
/// to 18 dp. `None` when an amount is zero or the result does not fit.
#[must_use]
pub fn exchange_ratio(
    input: u64,
    input_decimals: u8,
    output: u64,
    output_decimals: u8,
) -> Option<Decimal> {
    if input == 0 || output == 0 {
        return None;
    }
    let raw = Decimal::from(output).checked_div(Decimal::from(input))?;
    let shift = i32::from(input_decimals) - i32::from(output_decimals);
    let scaled = if shift >= 0 {
        raw.checked_mul(pow10(shift.unsigned_abs())?)?
    } else {
        raw.checked_div(pow10(shift.unsigned_abs())?)?
    };
    let r = scaled.round_dp_with_strategy(RATIO_SCALE, RoundingStrategy::ToZero);
    (r > Decimal::ZERO).then_some(r)
}

/// Value of `raw` units at `price` per UI unit, rounded toward zero to 18 dp.
#[must_use]
pub fn value(raw: u64, decimals: u8, price: Decimal) -> Option<Decimal> {
    Some(
        ui_amount(raw, decimals)?
            .checked_mul(price)?
            .round_dp_with_strategy(RATIO_SCALE, RoundingStrategy::ToZero),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::str::FromStr;

    #[test]
    fn ratio_accounts_for_decimals() {
        // 1 SOL (9 dp) -> 150 USDC (6 dp) = 150 USDC per SOL.
        assert_eq!(
            exchange_ratio(1_000_000_000, 9, 150_000_000, 6),
            Some(Decimal::from(150))
        );
        // 3 -> 1 rounds toward zero.
        assert_eq!(
            exchange_ratio(3, 0, 1, 0),
            Some(Decimal::from_str("0.333333333333333333").unwrap())
        );
        assert_eq!(exchange_ratio(0, 9, 1, 6), None);
        assert_eq!(ui_amount(1_500, 3), Some(Decimal::from_str("1.5").unwrap()));
    }

    #[test]
    fn value_is_exact() {
        let p = Decimal::from_str("152.25").unwrap();
        assert_eq!(
            value(2_000_000_000, 9, p),
            Some(Decimal::from_str("304.5").unwrap())
        );
    }

    proptest! {
        #[test]
        fn ratio_never_panics_and_is_positive(a in 1_u64.., b in 1_u64.., da in 0_u8..=18, db in 0_u8..=18) {
            if let Some(r) = exchange_ratio(a, da, b, db) {
                prop_assert!(r > Decimal::ZERO);
                prop_assert!(r.scale() <= RATIO_SCALE);
            }
        }
    }
}
