// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Float-to-integer conversions for the `[developer]` raw-field lines.
//!
//! Each helper returns, for **every** input, exactly what the `as` cast it
//! replaces returns: the value truncated toward zero, saturated at the
//! integer type's bounds, and `0` for `NaN`. Spelling the rule out with
//! integer arithmetic keeps the printed raw fields byte-identical while
//! making the truncation explicit instead of implied by a cast.

/// Number of explicit mantissa bits in an `f64`.
const MANTISSA_BITS: u64 = 52;

/// Exponent bias of an `f64`.
const EXPONENT_BIAS: u64 = 1023;

/// Biased exponent shared by the infinities and every `NaN`.
const EXPONENT_SPECIAL: u64 = 0x7ff;

/// `|x|` truncated toward zero: saturates at `u64::MAX` and maps `NaN` to `0`.
const fn truncated_magnitude(x: f64) -> u64 {
    let bits = x.to_bits();
    let exponent = (bits >> MANTISSA_BITS) & EXPONENT_SPECIAL;
    let fraction = bits & ((1 << MANTISSA_BITS) - 1);
    if exponent == EXPONENT_SPECIAL {
        // The infinities saturate; a `NaN` (non-zero fraction) is 0.
        return if fraction == 0 { u64::MAX } else { 0 };
    }
    if exponent < EXPONENT_BIAS {
        // |x| < 1, zeros and subnormals included, truncates to 0.
        return 0;
    }
    let power = exponent - EXPONENT_BIAS;
    if power >= 64 {
        return u64::MAX;
    }
    // 1.fraction × 2^power as an integer, with the fractional bits cut off.
    let mantissa = fraction | (1 << MANTISSA_BITS);
    if power >= MANTISSA_BITS {
        mantissa << (power - MANTISSA_BITS)
    } else {
        mantissa >> (MANTISSA_BITS - power)
    }
}

/// `x as i64`: truncates toward zero, saturates at `i64::MIN` and
/// `i64::MAX`, and maps `NaN` to `0`.
#[must_use]
pub fn f64_to_i64(x: f64) -> i64 {
    let magnitude = truncated_magnitude(x);
    if x.is_sign_negative() {
        0_i64.checked_sub_unsigned(magnitude).unwrap_or(i64::MIN)
    } else {
        i64::try_from(magnitude).unwrap_or(i64::MAX)
    }
}

/// `x as i32`: truncates toward zero, saturates at `i32::MIN` and
/// `i32::MAX`, and maps `NaN` to `0`.
#[must_use]
pub fn f64_to_i32(x: f64) -> i32 {
    let wide = f64_to_i64(x);
    i32::try_from(wide).unwrap_or(if wide < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i64_truncates_toward_zero() {
        assert_eq!(f64_to_i64(0.0), 0);
        assert_eq!(f64_to_i64(-0.0), 0);
        assert_eq!(f64_to_i64(0.999), 0);
        assert_eq!(f64_to_i64(-0.999), 0);
        assert_eq!(f64_to_i64(1.0), 1);
        assert_eq!(f64_to_i64(-1.0), -1);
        assert_eq!(f64_to_i64(2.9), 2);
        assert_eq!(f64_to_i64(-2.9), -2);
        assert_eq!(f64_to_i64(4200.0), 4200);
        assert_eq!(f64_to_i64(-900.0), -900);
        assert_eq!(f64_to_i64(1234.56), 1234);
        assert_eq!(f64_to_i64(-1234.56), -1234);
    }

    #[test]
    fn i64_maps_tiny_magnitudes_to_zero() {
        // The smallest subnormal and the smallest normal value.
        assert_eq!(f64_to_i64(f64::from_bits(1)), 0);
        assert_eq!(f64_to_i64(-f64::from_bits(1)), 0);
        assert_eq!(f64_to_i64(f64::MIN_POSITIVE), 0);
        assert_eq!(f64_to_i64(-f64::MIN_POSITIVE), 0);
        assert_eq!(f64_to_i64(f64::EPSILON), 0);
    }

    #[test]
    fn i64_is_exact_for_large_integers() {
        // 2^52, 2^53 and the largest `f64` below 2^63.
        assert_eq!(f64_to_i64(4_503_599_627_370_496.0), 4_503_599_627_370_496);
        assert_eq!(f64_to_i64(9_007_199_254_740_992.0), 9_007_199_254_740_992);
        assert_eq!(f64_to_i64(-9_007_199_254_740_992.0), -9_007_199_254_740_992);
        assert_eq!(
            f64_to_i64(9_223_372_036_854_774_784.0),
            9_223_372_036_854_774_784
        );
        assert_eq!(
            f64_to_i64(-9_223_372_036_854_774_784.0),
            -9_223_372_036_854_774_784
        );
    }

    #[test]
    fn i64_saturates_and_maps_nan_to_zero() {
        assert_eq!(f64_to_i64(9_223_372_036_854_775_808.0), i64::MAX);
        assert_eq!(f64_to_i64(-9_223_372_036_854_775_808.0), i64::MIN);
        assert_eq!(f64_to_i64(1e300), i64::MAX);
        assert_eq!(f64_to_i64(-1e300), i64::MIN);
        assert_eq!(f64_to_i64(f64::MAX), i64::MAX);
        assert_eq!(f64_to_i64(f64::MIN), i64::MIN);
        assert_eq!(f64_to_i64(f64::INFINITY), i64::MAX);
        assert_eq!(f64_to_i64(f64::NEG_INFINITY), i64::MIN);
        assert_eq!(f64_to_i64(f64::NAN), 0);
        assert_eq!(f64_to_i64(-f64::NAN), 0);
    }

    #[test]
    fn i32_truncates_toward_zero() {
        assert_eq!(f64_to_i32(0.0), 0);
        assert_eq!(f64_to_i32(-0.0), 0);
        assert_eq!(f64_to_i32(0.9), 0);
        assert_eq!(f64_to_i32(-0.9), 0);
        assert_eq!(f64_to_i32(72.0), 72);
        assert_eq!(f64_to_i32(99.9), 99);
        assert_eq!(f64_to_i32(5200.0), 5200);
        assert_eq!(f64_to_i32(-3800.0), -3800);
        assert_eq!(f64_to_i32(-3799.999), -3799);
    }

    #[test]
    fn i32_saturates_and_maps_nan_to_zero() {
        assert_eq!(f64_to_i32(2_147_483_647.0), i32::MAX);
        assert_eq!(f64_to_i32(2_147_483_647.9), i32::MAX);
        assert_eq!(f64_to_i32(2_147_483_646.9), 2_147_483_646);
        assert_eq!(f64_to_i32(2_147_483_648.0), i32::MAX);
        assert_eq!(f64_to_i32(-2_147_483_648.0), i32::MIN);
        assert_eq!(f64_to_i32(-2_147_483_648.9), i32::MIN);
        assert_eq!(f64_to_i32(-2_147_483_647.9), -2_147_483_647);
        assert_eq!(f64_to_i32(-2_147_483_649.0), i32::MIN);
        assert_eq!(f64_to_i32(1e300), i32::MAX);
        assert_eq!(f64_to_i32(-1e300), i32::MIN);
        assert_eq!(f64_to_i32(f64::INFINITY), i32::MAX);
        assert_eq!(f64_to_i32(f64::NEG_INFINITY), i32::MIN);
        assert_eq!(f64_to_i32(f64::NAN), 0);
        assert_eq!(f64_to_i32(-f64::NAN), 0);
    }
}
