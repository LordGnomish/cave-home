// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! Exact numeric conversions for the Hue wire formats.
//!
//! Each helper returns, for **every** input, exactly what the corresponding
//! `as` cast returns (for a `NaN` the language only promises *a* `NaN`). They
//! are spelled out on the bit patterns so the rounding, truncation and
//! saturation rules are explicit instead of implied by a cast.

/// Number of explicit mantissa bits in an `f64`.
const F64_MANTISSA_BITS: u64 = 52;

/// Biased exponent shared by the `f64` infinities and every `f64` `NaN`.
const F64_EXPONENT_SPECIAL: u64 = 0x7ff;

/// Number of explicit mantissa bits in an `f32`.
const F32_MANTISSA_BITS: u32 = 23;

/// Biased exponent shared by the `f32` infinities and every `f32` `NaN`.
const F32_EXPONENT_SPECIAL: u32 = 0xff;

/// Exponent bias of an `f32`.
const F32_EXPONENT_BIAS: u32 = 127;

/// Bit pattern of the positive `f32` infinity.
const F32_INFINITY_BITS: u32 = 0x7f80_0000;

/// The quiet bit of an `f32` `NaN`.
const F32_QUIET_BIT: u32 = 0x0040_0000;

/// Difference between the `f64` and the `f32` exponent bias (1023 - 127).
const BIAS_DIFFERENCE: u64 = 896;

/// `f64` biased exponent of one `f32` subnormal step (2^-149) scaled by the
/// 52 mantissa bits: a normal `f64` is `mantissa × 2^(exponent - 1075)`, so
/// in units of 2^-149 it is `mantissa >> (926 - exponent)`.
const SUBNORMAL_SHIFT_BASE: u64 = 926;

/// The low 32 bits of `n` (callers pass values below 2^32).
const fn low_u32(n: u64) -> u32 {
    let [b0, b1, b2, b3, ..] = n.to_le_bytes();
    u32::from_le_bytes([b0, b1, b2, b3])
}

/// `n >> shift`, rounded to nearest with ties to even. `shift` is 1..=63.
const fn shift_right_round_even(n: u64, shift: u64) -> u64 {
    let quotient = n >> shift;
    let remainder = n & ((1 << shift) - 1);
    let half = 1 << (shift - 1);
    if remainder > half || (remainder == half && quotient & 1 == 1) {
        quotient + 1
    } else {
        quotient
    }
}

/// `x as f32`: the nearest `f32`, ties to even. Magnitudes beyond the `f32`
/// range become an infinity, magnitudes below half the smallest subnormal
/// become a signed zero, and a `NaN` stays a `NaN`.
pub const fn f64_to_f32(x: f64) -> f32 {
    let bits = x.to_bits();
    let sign: u32 = if bits >> 63 == 1 { 1 << 31 } else { 0 };
    let exponent = (bits >> F64_MANTISSA_BITS) & F64_EXPONENT_SPECIAL;
    let fraction = bits & ((1 << F64_MANTISSA_BITS) - 1);

    if exponent == F64_EXPONENT_SPECIAL {
        if fraction == 0 {
            return f32::from_bits(sign | F32_INFINITY_BITS);
        }
        // NaN: keep the sign and the top payload bits, and make it quiet.
        let payload = low_u32(fraction >> (F64_MANTISSA_BITS - 23));
        return f32::from_bits(sign | F32_INFINITY_BITS | F32_QUIET_BIT | payload);
    }

    let mantissa = fraction | (1 << F64_MANTISSA_BITS);
    if exponent > BIAS_DIFFERENCE {
        // At least the smallest normal `f32` before rounding.
        let target = exponent - BIAS_DIFFERENCE;
        if target >= 0xff {
            return f32::from_bits(sign | F32_INFINITY_BITS);
        }
        // 24 significant bits, the leading one overlapping the exponent
        // field: adding it carries a rounded-up mantissa into the exponent,
        // and a carry out of the largest exponent yields the infinity bits.
        let rounded = shift_right_round_even(mantissa, F64_MANTISSA_BITS - 23);
        let magnitude = ((target - 1) << 23) + rounded;
        return f32::from_bits(sign | low_u32(magnitude));
    }

    // Below the normal `f32` range: count in subnormal steps of 2^-149.
    let shift = SUBNORMAL_SHIFT_BASE - exponent;
    if exponent == 0 || shift > F64_MANTISSA_BITS + 1 {
        // `f64` subnormals and zero, and anything under half a step.
        return f32::from_bits(sign);
    }
    f32::from_bits(sign | low_u32(shift_right_round_even(mantissa, shift)))
}

/// `x as u16`: truncates toward zero, saturates at `0` and `u16::MAX`, and
/// maps `NaN` to `0`.
pub const fn f32_to_u16(x: f32) -> u16 {
    let bits = x.to_bits();
    let negative = bits >> 31 == 1;
    let exponent = (bits >> F32_MANTISSA_BITS) & F32_EXPONENT_SPECIAL;
    let fraction = bits & ((1 << F32_MANTISSA_BITS) - 1);
    if exponent == F32_EXPONENT_SPECIAL {
        // NaN and -inf go to 0, +inf saturates.
        return if fraction != 0 || negative {
            0
        } else {
            u16::MAX
        };
    }
    if negative || exponent < F32_EXPONENT_BIAS {
        // Negative values saturate at 0; |x| < 1 truncates to 0.
        return 0;
    }
    let power = exponent - F32_EXPONENT_BIAS;
    if power >= 16 {
        return u16::MAX;
    }
    // 1.fraction × 2^power, as an integer with the fractional bits cut off.
    let mantissa = fraction | (1 << F32_MANTISSA_BITS);
    let [low, high, ..] = (mantissa >> (F32_MANTISSA_BITS - power)).to_le_bytes();
    u16::from_le_bytes([low, high])
}

/// `n as u16`: keeps the low 16 bits (two's-complement wrap).
pub const fn i64_to_u16(n: i64) -> u16 {
    let [low, high, ..] = n.to_le_bytes();
    u16::from_le_bytes([low, high])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert `f64_to_f32` maps each `f64` bit pattern to the `f32` one.
    fn assert_narrows(cases: &[(u64, u32)]) {
        for &(input, expected) in cases {
            let got = f64_to_f32(f64::from_bits(input)).to_bits();
            assert_eq!(got, expected, "input bits {input:#018x}");
        }
    }

    #[test]
    fn f64_to_f32_keeps_representable_values_and_rounds_the_rest() {
        assert_narrows(&[
            (0x0000_0000_0000_0000, 0x0000_0000), // 0.0
            (0x8000_0000_0000_0000, 0x8000_0000), // -0.0
            (0x3ff0_0000_0000_0000, 0x3f80_0000), // 1.0
            (0xc004_0000_0000_0000, 0xc020_0000), // -2.5
            (0x3fb9_9999_9999_999a, 0x3dcc_cccd), // 0.1
            (0x3fdd_4467_381d_7dbf, 0x3eea_233a), // 0.4573
            (0x3fda_3d70_a3d7_0a3d, 0x3ed1_eb85), // 0.41
            (0x404b_c000_0000_0000, 0x425e_0000), // 55.5
        ]);
    }

    #[test]
    fn f64_to_f32_rounds_ties_to_even_like_the_cast() {
        assert_narrows(&[
            (0x3ff0_0000_1000_0000, 0x3f80_0000), // 1 + 2^-24: tie, the even neighbour is below
            (0x3ff0_0000_1000_0001, 0x3f80_0001), // just above that tie
            (0x3ff0_0000_0fff_ffff, 0x3f80_0000), // just below that tie
            (0x3ff0_0000_3000_0000, 0x3f80_0002), // 1 + 3 * 2^-24: tie, the even neighbour is above
        ]);
    }

    #[test]
    fn f64_to_f32_overflows_to_infinity() {
        assert_narrows(&[
            (0x47ef_ffff_e000_0000, 0x7f7f_ffff), // f32::MAX
            (0x47ef_ffff_efff_ffff, 0x7f7f_ffff), // just below f32::MAX + half a step
            (0x47ef_ffff_f000_0000, 0x7f80_0000), // f32::MAX + half a step: tie, rounds up to infinity
            (0x47f0_0000_0000_0000, 0x7f80_0000), // 2^128
            (0x483d_6329_f1c3_5ca5, 0x7f80_0000), // 1e40
            (0xc83d_6329_f1c3_5ca5, 0xff80_0000), // -1e40
            (0x7fef_ffff_ffff_ffff, 0x7f80_0000), // f64::MAX
            (0x7ff0_0000_0000_0000, 0x7f80_0000), // infinity
            (0xfff0_0000_0000_0000, 0xff80_0000), // -infinity
        ]);
    }

    #[test]
    fn f64_to_f32_handles_subnormals_and_underflow() {
        assert_narrows(&[
            (0x3810_0000_0000_0000, 0x0080_0000), // f32::MIN_POSITIVE
            (0x380f_ffff_c000_0000, 0x007f_ffff), // the largest f32 subnormal
            (0x380f_ffff_f000_0000, 0x0080_0000), // halfway up to f32::MIN_POSITIVE: tie, rounds to even
            (0x36a0_0000_0000_0000, 0x0000_0001), // 2^-149, the smallest f32 subnormal
            (0x36a8_0000_0000_0000, 0x0000_0002), // 1.5 * 2^-149: tie, rounds to even
            (0x3690_0000_0000_0001, 0x0000_0001), // just above 2^-150
            (0x3690_0000_0000_0000, 0x0000_0000), // 2^-150: tie, rounds to zero
            (0xb680_0000_0000_0000, 0x8000_0000), // -2^-151 keeps its sign
            (0x358d_ee7a_4ad4_b81f, 0x0000_0000), // 1e-50
            (0x0010_0000_0000_0000, 0x0000_0000), // f64::MIN_POSITIVE
            (0x0000_0000_0000_0001, 0x0000_0000), // the smallest f64 subnormal
        ]);
    }

    #[test]
    fn f64_to_f32_keeps_nan_a_nan() {
        assert!(f64_to_f32(f64::NAN).is_nan());
        assert!(f64_to_f32(-f64::NAN).is_nan());
        assert!(f64_to_f32(f64::from_bits(0x7ff0_0000_0000_0001)).is_nan());
    }

    #[test]
    fn f32_to_u16_truncates_toward_zero() {
        assert_eq!(f32_to_u16(0.0), 0);
        assert_eq!(f32_to_u16(0.999), 0);
        assert_eq!(f32_to_u16(1.0), 1);
        assert_eq!(f32_to_u16(1.5), 1);
        assert_eq!(f32_to_u16(127.0), 127);
        assert_eq!(f32_to_u16(127.5), 127);
        assert_eq!(f32_to_u16(253.999_98), 253);
        assert_eq!(f32_to_u16(254.0), 254);
        assert_eq!(f32_to_u16(65_534.5), 65_534);
        assert_eq!(f32_to_u16(65_535.0), u16::MAX);
    }

    #[test]
    fn f32_to_u16_saturates_and_maps_nan_to_zero() {
        assert_eq!(f32_to_u16(65_536.0), u16::MAX);
        assert_eq!(f32_to_u16(1e9), u16::MAX);
        assert_eq!(f32_to_u16(f32::MAX), u16::MAX);
        assert_eq!(f32_to_u16(f32::INFINITY), u16::MAX);
        assert_eq!(f32_to_u16(-0.0), 0);
        assert_eq!(f32_to_u16(-0.5), 0);
        assert_eq!(f32_to_u16(-1.0), 0);
        assert_eq!(f32_to_u16(f32::NEG_INFINITY), 0);
        assert_eq!(f32_to_u16(f32::MIN_POSITIVE), 0);
        assert_eq!(f32_to_u16(f32::NAN), 0);
        assert_eq!(f32_to_u16(-f32::NAN), 0);
    }

    #[test]
    fn i64_to_u16_keeps_the_low_sixteen_bits() {
        assert_eq!(i64_to_u16(0), 0);
        assert_eq!(i64_to_u16(1), 1);
        assert_eq!(i64_to_u16(153), 153);
        assert_eq!(i64_to_u16(254), 254);
        assert_eq!(i64_to_u16(500), 500);
        assert_eq!(i64_to_u16(65_535), u16::MAX);
        assert_eq!(i64_to_u16(65_536), 0);
        assert_eq!(i64_to_u16(65_537), 1);
        assert_eq!(i64_to_u16(-1), u16::MAX);
        assert_eq!(i64_to_u16(-65_536), 0);
        assert_eq!(i64_to_u16(i64::MAX), u16::MAX);
        assert_eq!(i64_to_u16(i64::MIN), 0);
    }
}
