// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Exact numeric conversions for values read from the bridge.
//!
//! [`f64_to_f32`] returns, for **every** input, exactly what the `as` cast
//! returns (for a `NaN` the language only promises *a* `NaN`). It is spelled
//! out on the bit patterns so the rounding and overflow rules are explicit
//! instead of implied by a cast.

/// Number of explicit mantissa bits in an `f64`.
const F64_MANTISSA_BITS: u64 = 52;

/// Biased exponent shared by the `f64` infinities and every `f64` `NaN`.
const F64_EXPONENT_SPECIAL: u64 = 0x7ff;

/// Number of explicit mantissa bits in an `f32`.
const F32_MANTISSA_BITS: u64 = 23;

/// Biased exponent shared by the `f32` infinities and every `f32` `NaN`.
const F32_EXPONENT_SPECIAL: u64 = 0xff;

/// Bit pattern of the positive `f32` infinity.
const F32_INFINITY_BITS: u32 = 0x7f80_0000;

/// The quiet bit of an `f32` `NaN`.
const F32_QUIET_BIT: u32 = 0x0040_0000;

/// Difference between the `f64` and the `f32` exponent bias (1023 - 127).
const BIAS_DIFFERENCE: u64 = 896;

/// A normal `f64` is `mantissa × 2^(exponent - 1075)`, so in units of the
/// smallest `f32` subnormal (2^-149) it is `mantissa >> (926 - exponent)`.
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
        let payload = low_u32(fraction >> (F64_MANTISSA_BITS - F32_MANTISSA_BITS));
        return f32::from_bits(sign | F32_INFINITY_BITS | F32_QUIET_BIT | payload);
    }

    let mantissa = fraction | (1 << F64_MANTISSA_BITS);
    if exponent > BIAS_DIFFERENCE {
        // At least the smallest normal `f32` before rounding.
        let target = exponent - BIAS_DIFFERENCE;
        if target >= F32_EXPONENT_SPECIAL {
            return f32::from_bits(sign | F32_INFINITY_BITS);
        }
        // 24 significant bits, the leading one overlapping the exponent
        // field: adding it carries a rounded-up mantissa into the exponent,
        // and a carry out of the largest exponent yields the infinity bits.
        let rounded = shift_right_round_even(mantissa, F64_MANTISSA_BITS - F32_MANTISSA_BITS);
        let magnitude = ((target - 1) << F32_MANTISSA_BITS) + rounded;
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
    fn low_u32_keeps_the_low_half() {
        assert_eq!(low_u32(0), 0);
        assert_eq!(low_u32(0xffff_ffff), u32::MAX);
        assert_eq!(low_u32(0x1234_5678_9abc_def0), 0x9abc_def0);
    }

    #[test]
    fn shift_right_round_even_rounds_halves_to_the_even_neighbour() {
        assert_eq!(shift_right_round_even(0b1000, 2), 0b10); // exact
        assert_eq!(shift_right_round_even(0b1001, 2), 0b10); // below half
        assert_eq!(shift_right_round_even(0b1010, 2), 0b10); // tie, even stays
        assert_eq!(shift_right_round_even(0b1011, 2), 0b11); // above half
        assert_eq!(shift_right_round_even(0b1110, 2), 0b100); // tie, odd goes up
        assert_eq!(shift_right_round_even(1, 1), 0); // tie towards zero
        assert_eq!(shift_right_round_even(3, 1), 2);
        assert_eq!(shift_right_round_even(u64::MAX, 63), 2);
    }

    #[test]
    fn f64_to_f32_keeps_representable_values_and_rounds_the_rest() {
        assert_narrows(&[
            (0x0000_0000_0000_0000, 0x0000_0000), // 0.0
            (0x8000_0000_0000_0000, 0x8000_0000), // -0.0
            (0x3ff0_0000_0000_0000, 0x3f80_0000), // 1.0
            (0xc004_0000_0000_0000, 0xc020_0000), // -2.5
            (0x3fb9_9999_9999_999a, 0x3dcc_cccd), // 0.1
            (0x3fe6_20c4_9ba5_e354, 0x3f31_0625), // 0.6915 (gamut C red x)
            (0x3fc5_c28f_5c28_f5c3, 0x3e2e_147b), // 0.17 (gamut C green x)
            (0x3fa8_51eb_851e_b852, 0x3d42_8f5c), // 0.0475 (gamut C blue y)
            (0x406f_c000_0000_0000, 0x437e_0000), // 254.0
            (0x406f_e000_0000_0000, 0x437f_0000), // 255.0
        ]);
    }

    #[test]
    fn f64_to_f32_rounds_ties_to_even_like_the_cast() {
        assert_narrows(&[
            (0x3ff0_0000_1000_0000, 0x3f80_0000), // 1 + 2^-24: tie, the even neighbour is below
            (0x3ff0_0000_1000_0001, 0x3f80_0001), // just above that tie
            (0x3ff0_0000_0fff_ffff, 0x3f80_0000), // just below that tie
            (0x3ff0_0000_3000_0000, 0x3f80_0002), // 1 + 3 * 2^-24: tie, the even neighbour is above
            (0x4170_0000_1000_0000, 0x4b80_0000), // 2^24 + 1: tie, the even neighbour is below
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
}
