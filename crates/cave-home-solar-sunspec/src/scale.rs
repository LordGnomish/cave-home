// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec scale factor handling. Per SunSpec spec §B.3, each
//! measurement register has an associated `_SF` field of type
//! `sunssf` (signed int16) in the range `-10..=10` such that the
//! physical value is `register × 10^SF`.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Scale factor wrapper enforcing the legal range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScaleFactor(pub i16);

impl ScaleFactor {
    /// SunSpec sentinel "not implemented" for sunssf is `0x8000`
    /// (i.e. `i16::MIN`). Source: spec §B.3.
    pub const NOT_IMPLEMENTED: i16 = i16::MIN;

    /// Construct from a raw register value, returning `None` if the
    /// register carries the not-implemented sentinel.
    #[must_use]
    pub fn from_register(raw: i16) -> Option<Self> {
        if raw == Self::NOT_IMPLEMENTED {
            None
        } else {
            Some(Self(raw))
        }
    }

    /// Validate the scale factor is in `-10..=10`.
    pub fn validated(self) -> Result<Self> {
        if self.0 < -10 || self.0 > 10 {
            Err(Error::ScaleOutOfRange(self.0))
        } else {
            Ok(self)
        }
    }

    /// Apply this scale factor to an unsigned register value, returning
    /// the physical f64 value.
    #[must_use]
    pub fn apply_u16(self, raw: u16) -> f64 {
        f64::from(raw) * 10.0_f64.powi(i32::from(self.0))
    }

    /// Apply this scale factor to a signed register value.
    #[must_use]
    pub fn apply_i16(self, raw: i16) -> f64 {
        f64::from(raw) * 10.0_f64.powi(i32::from(self.0))
    }

    /// Apply this scale factor to a 32-bit unsigned value (acc32 / uint32).
    #[must_use]
    pub fn apply_u32(self, raw: u32) -> f64 {
        f64::from(raw) * 10.0_f64.powi(i32::from(self.0))
    }
}

impl Default for ScaleFactor {
    fn default() -> Self {
        Self(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_register_detects_sentinel() {
        assert_eq!(ScaleFactor::from_register(i16::MIN), None);
        assert_eq!(ScaleFactor::from_register(2), Some(ScaleFactor(2)));
    }

    #[test]
    fn validated_rejects_out_of_range() {
        assert!(ScaleFactor(11).validated().is_err());
        assert!(ScaleFactor(-11).validated().is_err());
        assert!(ScaleFactor(0).validated().is_ok());
        assert!(ScaleFactor(10).validated().is_ok());
        assert!(ScaleFactor(-10).validated().is_ok());
    }

    #[test]
    fn apply_u16_zero_scale_identity() {
        assert!((ScaleFactor(0).apply_u16(7350) - 7350.0).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_u16_positive_scale_amplifies() {
        // 7350 × 10^2 = 735000 (e.g. AC power scale +2)
        assert!((ScaleFactor(2).apply_u16(7350) - 735_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_u16_negative_scale_divides() {
        // 7350 × 10^-1 = 735.0 (e.g. voltage scale -1 ⇒ 735.0 V)
        assert!((ScaleFactor(-1).apply_u16(7350) - 735.0).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_i16_handles_negative() {
        assert!((ScaleFactor(-1).apply_i16(-1234) - (-123.4)).abs() < 1e-9);
    }

    #[test]
    fn apply_u32_works() {
        assert!((ScaleFactor(-3).apply_u32(1_234_567) - 1234.567).abs() < 1e-6);
    }
}
