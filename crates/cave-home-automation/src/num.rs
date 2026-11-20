// SPDX-License-Identifier: Apache-2.0
//! Numeric helpers: exact integer-to-float conversion and strict
//! threshold comparisons with the `NaN` handling spelled out.
//!
//! Each helper returns, for **every** input, exactly what the cast or
//! operator it replaces returns; the rounding and comparison rules are
//! written down here instead of being implied at the call sites.

use std::cmp::Ordering;

/// 2^32 as an `f64`.
const TWO_POW_32: f64 = 4_294_967_296.0;

/// `n as f64`: the nearest `f64`, ties to even.
///
/// Each 32-bit half converts exactly, and scaling the high half by 2^32
/// is exact as well, so the only rounding is the single
/// round-to-nearest-even of the final sum — the rounding the cast does.
fn u64_to_f64(n: u64) -> f64 {
    let [b0, b1, b2, b3, b4, b5, b6, b7] = n.to_be_bytes();
    let high = f64::from(u32::from_be_bytes([b0, b1, b2, b3])) * TWO_POW_32;
    high + f64::from(u32::from_be_bytes([b4, b5, b6, b7]))
}

/// `n as f64`: the nearest `f64`, ties to even.
///
/// Exact for every `|n| <= 2^53` (which covers any realistic Unix
/// timestamp); larger magnitudes round to the nearest representable value.
#[must_use]
pub fn i64_to_f64(n: i64) -> f64 {
    let magnitude = u64_to_f64(n.unsigned_abs());
    if n < 0 { -magnitude } else { magnitude }
}

/// `value > threshold`.
///
/// A `NaN` on either side is not comparable and is therefore never
/// above — the same answer the `>` operator gives.
#[must_use]
pub fn is_above(value: f64, threshold: f64) -> bool {
    value.partial_cmp(&threshold) == Some(Ordering::Greater)
}

/// `value < threshold`.
///
/// A `NaN` on either side is not comparable and is therefore never
/// below — the same answer the `<` operator gives.
#[must_use]
pub fn is_below(value: f64, threshold: f64) -> bool {
    value.partial_cmp(&threshold) == Some(Ordering::Less)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn bits(x: f64) -> u64 {
        x.to_bits()
    }

    #[test]
    fn i64_to_f64_is_exact_up_to_two_pow_53() {
        assert_eq!(bits(i64_to_f64(0)), bits(0.0));
        assert_eq!(bits(i64_to_f64(1)), bits(1.0));
        assert_eq!(bits(i64_to_f64(-1)), bits(-1.0));
        assert_eq!(bits(i64_to_f64(1_700_000_000)), bits(1_700_000_000.0));
        assert_eq!(bits(i64_to_f64(-1_700_000_000)), bits(-1_700_000_000.0));
        assert_eq!(bits(i64_to_f64(4_294_967_295)), bits(4_294_967_295.0));
        assert_eq!(bits(i64_to_f64(4_294_967_296)), bits(4_294_967_296.0));
        assert_eq!(bits(i64_to_f64(4_294_967_297)), bits(4_294_967_297.0));
        assert_eq!(
            bits(i64_to_f64(9_007_199_254_740_991)),
            bits(9_007_199_254_740_991.0)
        );
        assert_eq!(
            bits(i64_to_f64(9_007_199_254_740_992)),
            bits(9_007_199_254_740_992.0)
        );
        assert_eq!(
            bits(i64_to_f64(-9_007_199_254_740_992)),
            bits(-9_007_199_254_740_992.0)
        );
    }

    #[test]
    fn i64_to_f64_rounds_to_nearest_even_beyond_two_pow_53() {
        // 2^53 + 1 is a tie between 2^53 and 2^53 + 2: the even mantissa wins.
        assert_eq!(
            bits(i64_to_f64(9_007_199_254_740_993)),
            bits(9_007_199_254_740_992.0)
        );
        // 2^53 + 3 is a tie between 2^53 + 2 and 2^53 + 4: up to the even one.
        assert_eq!(
            bits(i64_to_f64(9_007_199_254_740_995)),
            bits(9_007_199_254_740_996.0)
        );
        assert_eq!(
            bits(i64_to_f64(-9_007_199_254_740_993)),
            bits(-9_007_199_254_740_992.0)
        );
        assert_eq!(
            bits(i64_to_f64(i64::MAX)),
            bits(9_223_372_036_854_775_808.0)
        );
        assert_eq!(
            bits(i64_to_f64(i64::MIN)),
            bits(-9_223_372_036_854_775_808.0)
        );
    }

    #[test]
    fn is_above_is_strict() {
        assert!(is_above(25.0, 20.0));
        assert!(!is_above(20.0, 20.0));
        assert!(!is_above(10.0, 20.0));
        assert!(!is_above(0.0, -0.0));
        assert!(is_above(f64::INFINITY, f64::MAX));
        assert!(!is_above(f64::NEG_INFINITY, f64::MIN));
    }

    #[test]
    fn is_below_is_strict() {
        assert!(is_below(10.0, 15.0));
        assert!(!is_below(15.0, 15.0));
        assert!(!is_below(25.0, 15.0));
        assert!(!is_below(-0.0, 0.0));
        assert!(is_below(f64::NEG_INFINITY, f64::MIN));
        assert!(!is_below(f64::INFINITY, f64::MAX));
    }

    #[test]
    fn nan_is_neither_above_nor_below() {
        assert!(!is_above(f64::NAN, 0.0));
        assert!(!is_above(0.0, f64::NAN));
        assert!(!is_above(f64::NAN, f64::NAN));
        assert!(!is_below(f64::NAN, 0.0));
        assert!(!is_below(0.0, f64::NAN));
        assert!(!is_below(f64::NAN, f64::NAN));
    }
}
