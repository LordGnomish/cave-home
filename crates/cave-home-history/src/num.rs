//! Exact numeric conversions between integers and `f64`.
//!
//! Each helper returns, for **every** input, exactly what the corresponding
//! `as` cast returns. They are spelled out with integer arithmetic so the
//! rounding, truncation and saturation rules are explicit instead of implied
//! by a cast.

/// 2^32 as an `f64`.
const TWO_POW_32: f64 = 4_294_967_296.0;

/// Number of explicit mantissa bits in an `f64`.
const MANTISSA_BITS: u64 = 52;

/// Exponent bias of an `f64`.
const EXPONENT_BIAS: u64 = 1023;

/// Biased exponent shared by the infinities and every `NaN`.
const EXPONENT_SPECIAL: u64 = 0x7ff;

/// `n as f64`: the nearest `f64`, ties to even.
///
/// Both 32-bit halves convert exactly and `high` (a 32-bit integer times a
/// power of two) is exact too, so the only rounding is the single
/// round-to-nearest-even of the final sum — the same rounding the cast does.
fn u64_to_f64(n: u64) -> f64 {
    let [b0, b1, b2, b3, b4, b5, b6, b7] = n.to_be_bytes();
    let high = f64::from(u32::from_be_bytes([b0, b1, b2, b3])) * TWO_POW_32;
    high + f64::from(u32::from_be_bytes([b4, b5, b6, b7]))
}

/// `n as f64`: the nearest `f64`, ties to even (exact up to 2^53).
pub fn usize_to_f64(n: usize) -> f64 {
    u64_to_f64(n as u64)
}

/// `n as f64`: the nearest `f64`, ties to even (exact up to ±2^53).
pub fn i64_to_f64(n: i64) -> f64 {
    let magnitude = u64_to_f64(n.unsigned_abs());
    if n < 0 { -magnitude } else { magnitude }
}

/// `x as u64`: truncates toward zero, saturates at `0` and `u64::MAX`, and
/// maps `NaN` to `0`.
const fn f64_to_u64(x: f64) -> u64 {
    let bits = x.to_bits();
    let negative = bits >> 63 == 1;
    let exponent = (bits >> MANTISSA_BITS) & EXPONENT_SPECIAL;
    let fraction = bits & ((1 << MANTISSA_BITS) - 1);
    if exponent == EXPONENT_SPECIAL {
        // NaN and -inf go to 0, +inf saturates.
        return if fraction != 0 || negative {
            0
        } else {
            u64::MAX
        };
    }
    if negative || exponent < EXPONENT_BIAS {
        // Negative values saturate at 0; |x| < 1 truncates to 0.
        return 0;
    }
    let power = exponent - EXPONENT_BIAS;
    if power >= 64 {
        return u64::MAX;
    }
    // 1.fraction × 2^power, as an integer with the fractional bits cut off.
    let mantissa = fraction | (1 << MANTISSA_BITS);
    if power >= MANTISSA_BITS {
        mantissa << (power - MANTISSA_BITS)
    } else {
        mantissa >> (MANTISSA_BITS - power)
    }
}

/// `x as usize`: truncates toward zero, saturates at `0` and `usize::MAX`,
/// and maps `NaN` to `0`.
pub fn f64_to_usize(x: f64) -> usize {
    usize::try_from(f64_to_u64(x)).unwrap_or(usize::MAX)
}

/// `x as i64`: truncates toward zero, saturates at `i64::MIN` and
/// `i64::MAX`, and maps `NaN` to `0`.
pub fn f64_to_i64(x: f64) -> i64 {
    let magnitude = f64_to_u64(x.abs());
    if x.is_sign_negative() {
        0_i64.checked_sub_unsigned(magnitude).unwrap_or(i64::MIN)
    } else {
        i64::try_from(magnitude).unwrap_or(i64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_POW_53: u64 = 1 << 53;

    fn bits(x: f64) -> u64 {
        x.to_bits()
    }

    #[test]
    fn unsigned_to_float_is_exact_below_two_pow_53() {
        assert_eq!(bits(usize_to_f64(0)), bits(0.0));
        assert_eq!(bits(usize_to_f64(1)), bits(1.0));
        assert_eq!(bits(usize_to_f64(4_294_967_295)), bits(4_294_967_295.0));
        assert_eq!(bits(u64_to_f64(4_294_967_296)), bits(4_294_967_296.0));
        assert_eq!(
            bits(u64_to_f64(TWO_POW_53 - 1)),
            bits(9_007_199_254_740_991.0)
        );
        assert_eq!(bits(u64_to_f64(TWO_POW_53)), bits(9_007_199_254_740_992.0));
    }

    #[test]
    fn unsigned_to_float_rounds_ties_to_even_like_the_cast() {
        // 2^53 + 1 is halfway between two floats: the even one is below.
        assert_eq!(
            bits(u64_to_f64(TWO_POW_53 + 1)),
            bits(9_007_199_254_740_992.0)
        );
        assert_eq!(
            bits(u64_to_f64(TWO_POW_53 + 2)),
            bits(9_007_199_254_740_994.0)
        );
        // 2^53 + 3 is halfway again: the even one is above.
        assert_eq!(
            bits(u64_to_f64(TWO_POW_53 + 3)),
            bits(9_007_199_254_740_996.0)
        );
        // u64::MAX rounds up to 2^64.
        assert_eq!(
            bits(u64_to_f64(u64::MAX)),
            bits(18_446_744_073_709_551_616.0)
        );
        // Rounding at a carry between the two 32-bit halves.
        assert_eq!(
            bits(u64_to_f64(0x7fff_ffff_ffff_ffff)),
            bits(9_223_372_036_854_775_808.0)
        );
    }

    #[test]
    fn signed_to_float_is_symmetric() {
        assert_eq!(bits(i64_to_f64(0)), bits(0.0));
        assert_eq!(bits(i64_to_f64(-1)), bits(-1.0));
        assert_eq!(bits(i64_to_f64(86_400)), bits(86_400.0));
        assert_eq!(
            bits(i64_to_f64(i64::MAX)),
            bits(9_223_372_036_854_775_808.0)
        );
        assert_eq!(
            bits(i64_to_f64(i64::MIN)),
            bits(-9_223_372_036_854_775_808.0)
        );
        assert_eq!(
            bits(i64_to_f64(-9_007_199_254_740_993)),
            bits(-9_007_199_254_740_992.0)
        );
    }

    #[test]
    fn float_to_unsigned_truncates_toward_zero() {
        assert_eq!(f64_to_usize(0.0), 0);
        assert_eq!(f64_to_usize(0.999_999), 0);
        assert_eq!(f64_to_usize(1.0), 1);
        assert_eq!(f64_to_usize(2.9), 2);
        assert_eq!(f64_to_usize(94.999_999_999), 94);
        assert_eq!(f64_to_usize(95.0), 95);
        assert_eq!(f64_to_u64(4_294_967_296.75), 4_294_967_296);
        assert_eq!(f64_to_u64(9_007_199_254_740_992.0), TWO_POW_53);
        assert_eq!(f64_to_u64(9_223_372_036_854_775_808.0), 1 << 63);
        // Largest float below 2^64.
        assert_eq!(
            f64_to_u64(18_446_744_073_709_549_568.0),
            0xffff_ffff_ffff_f800
        );
    }

    #[test]
    fn float_to_unsigned_saturates_and_maps_nan_to_zero() {
        assert_eq!(f64_to_usize(f64::NAN), 0);
        assert_eq!(f64_to_usize(-f64::NAN), 0);
        assert_eq!(f64_to_usize(-0.0), 0);
        assert_eq!(f64_to_usize(-0.5), 0);
        assert_eq!(f64_to_usize(-1.0), 0);
        assert_eq!(f64_to_usize(f64::NEG_INFINITY), 0);
        assert_eq!(f64_to_usize(f64::MIN_POSITIVE / 2.0), 0);
        assert_eq!(f64_to_usize(f64::INFINITY), usize::MAX);
        assert_eq!(f64_to_usize(1e300), usize::MAX);
        assert_eq!(f64_to_u64(18_446_744_073_709_551_616.0), u64::MAX);
    }

    #[test]
    fn float_to_signed_truncates_and_saturates() {
        assert_eq!(f64_to_i64(0.0), 0);
        assert_eq!(f64_to_i64(-0.0), 0);
        assert_eq!(f64_to_i64(2.9), 2);
        assert_eq!(f64_to_i64(-2.9), -2);
        assert_eq!(f64_to_i64(21.0), 21);
        assert_eq!(f64_to_i64(-21.0), -21);
        assert_eq!(f64_to_i64(f64::NAN), 0);
        assert_eq!(f64_to_i64(-f64::NAN), 0);
        assert_eq!(f64_to_i64(9_223_372_036_854_775_808.0), i64::MAX);
        assert_eq!(f64_to_i64(1e300), i64::MAX);
        assert_eq!(f64_to_i64(f64::INFINITY), i64::MAX);
        assert_eq!(f64_to_i64(-9_223_372_036_854_775_808.0), i64::MIN);
        assert_eq!(f64_to_i64(-1e300), i64::MIN);
        assert_eq!(f64_to_i64(f64::NEG_INFINITY), i64::MIN);
        // Largest magnitudes that still fit.
        assert_eq!(
            f64_to_i64(9_223_372_036_854_774_784.0),
            9_223_372_036_854_774_784
        );
        assert_eq!(
            f64_to_i64(-9_223_372_036_854_774_784.0),
            -9_223_372_036_854_774_784
        );
    }
}
