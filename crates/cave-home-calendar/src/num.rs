//! Exact integer conversions for the calendar arithmetic.
//!
//! Each helper returns, for **every** input, exactly what the corresponding
//! `as` cast returns. They are spelled out on the two's-complement byte
//! representation so the truncation and sign-extension rules are explicit
//! instead of implied by a cast.

/// `n as i32`: keeps the low 32 bits (two's-complement wrap).
pub const fn i64_to_i32(n: i64) -> i32 {
    let [b0, b1, b2, b3, ..] = n.to_le_bytes();
    i32::from_le_bytes([b0, b1, b2, b3])
}

/// `n as u8`: keeps the low 8 bits.
pub const fn i64_to_u8(n: i64) -> u8 {
    let [low, ..] = n.to_le_bytes();
    low
}

/// `n as u8`: reinterprets the two's-complement bit pattern.
pub const fn i8_to_u8(n: i8) -> u8 {
    u8::from_ne_bytes(n.to_ne_bytes())
}

/// `n as usize`: sign-extends, then reinterprets the bit pattern.
pub fn i8_to_usize(n: i8) -> usize {
    usize::from_ne_bytes(isize::from(n).to_ne_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i64_to_i32_keeps_values_that_fit() {
        assert_eq!(i64_to_i32(0), 0);
        assert_eq!(i64_to_i32(1), 1);
        assert_eq!(i64_to_i32(-1), -1);
        assert_eq!(i64_to_i32(2026), 2026);
        assert_eq!(i64_to_i32(-2026), -2026);
        assert_eq!(i64_to_i32(2_147_483_647), i32::MAX);
        assert_eq!(i64_to_i32(-2_147_483_648), i32::MIN);
    }

    #[test]
    fn i64_to_i32_wraps_like_the_cast() {
        assert_eq!(i64_to_i32(2_147_483_648), i32::MIN);
        assert_eq!(i64_to_i32(-2_147_483_649), i32::MAX);
        assert_eq!(i64_to_i32(4_294_967_295), -1);
        assert_eq!(i64_to_i32(4_294_967_296), 0);
        assert_eq!(i64_to_i32(4_294_967_297), 1);
        assert_eq!(i64_to_i32(-4_294_967_296), 0);
        assert_eq!(i64_to_i32(i64::MAX), -1);
        assert_eq!(i64_to_i32(i64::MIN), 0);
    }

    #[test]
    fn i64_to_u8_keeps_the_low_byte() {
        assert_eq!(i64_to_u8(0), 0);
        assert_eq!(i64_to_u8(1), 1);
        assert_eq!(i64_to_u8(12), 12);
        assert_eq!(i64_to_u8(31), 31);
        assert_eq!(i64_to_u8(59), 59);
        assert_eq!(i64_to_u8(255), 255);
        assert_eq!(i64_to_u8(256), 0);
        assert_eq!(i64_to_u8(257), 1);
        assert_eq!(i64_to_u8(-1), 255);
        assert_eq!(i64_to_u8(-128), 128);
        assert_eq!(i64_to_u8(-256), 0);
        assert_eq!(i64_to_u8(i64::MAX), 255);
        assert_eq!(i64_to_u8(i64::MIN), 0);
    }

    #[test]
    fn i8_to_u8_reinterprets_the_bits() {
        assert_eq!(i8_to_u8(0), 0);
        assert_eq!(i8_to_u8(1), 1);
        assert_eq!(i8_to_u8(31), 31);
        assert_eq!(i8_to_u8(i8::MAX), 127);
        assert_eq!(i8_to_u8(-1), 255);
        assert_eq!(i8_to_u8(-127), 129);
        assert_eq!(i8_to_u8(i8::MIN), 128);
    }

    #[test]
    fn i8_to_usize_sign_extends() {
        assert_eq!(i8_to_usize(0), 0);
        assert_eq!(i8_to_usize(1), 1);
        assert_eq!(i8_to_usize(5), 5);
        assert_eq!(i8_to_usize(i8::MAX), 127);
        assert_eq!(i8_to_usize(-1), usize::MAX);
        assert_eq!(i8_to_usize(-127), usize::MAX - 126);
        assert_eq!(i8_to_usize(i8::MIN), usize::MAX - 127);
    }
}
