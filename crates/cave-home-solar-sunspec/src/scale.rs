// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec scale-factor (`sunssf`) handling.
//!
//! A SunSpec integer point carries its physical value as a raw integer plus a
//! companion `sunssf` point — a signed power-of-ten exponent. The real value
//! is `raw * 10^sf`. For example AC power `W = 1234` with `W_SF = -1` is
//! `123.4` W; a voltage `2301` with `V_SF = -1` is `230.1` V; a large energy
//! counter might use a positive exponent.
//!
//! Source: SunSpec Information Model Specification, "Scale Factors".

/// A SunSpec scale factor: a signed power-of-ten exponent applied to a raw
/// integer point to recover the physical value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleFactor(i16);

impl ScaleFactor {
    /// Build from the decoded `sunssf` exponent. SunSpec constrains the
    /// exponent to roughly `-10..=10`; out-of-range values are clamped so an
    /// arithmetic overflow can never reach the caller.
    #[must_use]
    pub fn new(exponent: i16) -> Self {
        Self(exponent.clamp(-10, 10))
    }

    /// A scale factor of `10^0 == 1` — the identity, used when a model omits
    /// (or sentinels) its scale-factor point.
    #[must_use]
    pub const fn unity() -> Self {
        Self(0)
    }

    /// The underlying exponent.
    #[must_use]
    pub const fn exponent(self) -> i16 {
        self.0
    }

    /// Apply the factor to a raw integer, returning the physical value.
    ///
    /// `value * 10^exponent`, computed in `f64` so both signs of exponent are
    /// exact for the magnitudes inverters report.
    #[must_use]
    pub fn apply(self, raw: f64) -> f64 {
        raw * 10f64.powi(i32::from(self.0))
    }

    /// Convenience: apply to a signed 16-bit raw point.
    #[must_use]
    pub fn apply_i16(self, raw: i16) -> f64 {
        self.apply(f64::from(raw))
    }

    /// Convenience: apply to an unsigned 16-bit raw point.
    #[must_use]
    pub fn apply_u16(self, raw: u16) -> f64 {
        self.apply(f64::from(raw))
    }

    /// Convenience: apply to a 32-bit accumulator/raw point.
    #[must_use]
    pub fn apply_u32(self, raw: u32) -> f64 {
        self.apply(f64::from(raw))
    }
}

impl Default for ScaleFactor {
    fn default() -> Self {
        Self::unity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_exponent_divides() {
        let sf = ScaleFactor::new(-1);
        assert!((sf.apply_u16(2301) - 230.1).abs() < 1e-9, "230.1 V");
        let sf2 = ScaleFactor::new(-2);
        assert!((sf2.apply_u16(5000) - 50.0).abs() < 1e-9, "50.00 Hz");
    }

    #[test]
    fn positive_exponent_multiplies() {
        let sf = ScaleFactor::new(3);
        assert!((sf.apply_u16(12) - 12_000.0).abs() < 1e-9, "12 * 10^3");
    }

    #[test]
    fn unity_is_identity() {
        let sf = ScaleFactor::unity();
        assert!((sf.apply_i16(1234) - 1234.0).abs() < 1e-9);
        assert_eq!(ScaleFactor::default(), ScaleFactor::unity());
    }

    #[test]
    fn signed_raw_with_factor() {
        let sf = ScaleFactor::new(-1);
        assert!((sf.apply_i16(-500) + 50.0).abs() < 1e-9, "-50.0 W (consuming)");
    }

    #[test]
    fn exponent_is_clamped_not_overflowing() {
        let sf = ScaleFactor::new(100);
        assert_eq!(sf.exponent(), 10);
        let sf = ScaleFactor::new(-100);
        assert_eq!(sf.exponent(), -10);
    }
}
