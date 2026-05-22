// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec Model 124 — Storage.
//!
//! Source: SunSpec spec model 124 — battery / storage control surface.

use crate::error::Result;
use crate::raw;
use crate::scale::ScaleFactor;
use serde::{Deserialize, Serialize};

/// Battery charge / discharge state per Model 124 `ChaSt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageChargeStatus {
    Off,
    Empty,
    Discharging,
    Charging,
    Full,
    Holding,
    Testing,
    Unknown(u16),
}

impl StorageChargeStatus {
    #[must_use]
    pub const fn from_register(raw: u16) -> Self {
        match raw {
            1 => Self::Off,
            2 => Self::Empty,
            3 => Self::Discharging,
            4 => Self::Charging,
            5 => Self::Full,
            6 => Self::Holding,
            7 => Self::Testing,
            other => Self::Unknown(other),
        }
    }
}

/// Decoded Model 124 (Storage).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BatteryReading {
    /// State of Charge in percent (0–100).
    pub soc_percent: f64,
    /// Maximum sustained charge rate in watts (positive).
    pub max_charge_w: f64,
    /// Maximum sustained discharge rate in watts (positive).
    pub max_discharge_w: f64,
    pub status: StorageChargeStatus,
}

impl BatteryReading {
    pub const MODEL_ID: u16 = 124;
    /// Minimum length needed for cave-home's fields.
    pub const MIN_LENGTH: u16 = 12;

    /// Parse a Model 124 payload.
    pub fn parse(regs: &[u16]) -> Result<Self> {
        if regs.len() < Self::MIN_LENGTH as usize {
            return Err(crate::Error::ShortRead {
                expected: Self::MIN_LENGTH,
                actual: regs.len() as u16,
            });
        }
        let w_chg_max_sf = ScaleFactor::from_register(raw::read_i16(regs, 3).unwrap_or(0)).unwrap_or_default();
        let w_dis_chg_max_sf = ScaleFactor::from_register(raw::read_i16(regs, 4).unwrap_or(0)).unwrap_or_default();
        let soc_sf = ScaleFactor::from_register(raw::read_i16(regs, 8).unwrap_or(0)).unwrap_or_default();

        let max_charge_w = w_chg_max_sf.apply_u16(raw::read_u16(regs, 1).unwrap_or(0));
        let max_discharge_w = w_dis_chg_max_sf.apply_u16(raw::read_u16(regs, 2).unwrap_or(0));
        let soc_percent = soc_sf.apply_u16(raw::read_u16(regs, 7).unwrap_or(0));
        let status = StorageChargeStatus::from_register(raw::read_u16(regs, 11).unwrap_or(0));

        Ok(Self {
            soc_percent,
            max_charge_w,
            max_discharge_w,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(soc_raw: u16, soc_sf: i16, status: u16) -> Vec<u16> {
        let mut regs = vec![0u16; 12];
        regs[1] = 5000; // WChaMax
        regs[2] = 5000; // WDisChaMax
        regs[3] = 0; // WChaMax_SF
        regs[4] = 0; // WDisChaMax_SF
        regs[7] = soc_raw;
        regs[8] = soc_sf as u16;
        regs[11] = status;
        regs
    }

    #[test]
    fn parses_soc_with_scale_factor() {
        let regs = build(7350, -2, 4); // 7350 × 10^-2 = 73.50%
        let r = BatteryReading::parse(&regs).unwrap();
        assert!((r.soc_percent - 73.50).abs() < 1e-6);
        assert_eq!(r.status, StorageChargeStatus::Charging);
        assert!((r.max_charge_w - 5000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn status_full_decodes() {
        let regs = build(10000, -2, 5);
        let r = BatteryReading::parse(&regs).unwrap();
        assert_eq!(r.status, StorageChargeStatus::Full);
    }

    #[test]
    fn short_payload_errs() {
        let regs = vec![0u16; 5];
        assert!(BatteryReading::parse(&regs).is_err());
    }

    #[test]
    fn unknown_status_preserves_register_value() {
        let regs = build(5000, -2, 42);
        let r = BatteryReading::parse(&regs).unwrap();
        assert_eq!(r.status, StorageChargeStatus::Unknown(42));
    }
}
