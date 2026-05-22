// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec Model 120 — Nameplate.
//!
//! Source: SunSpec spec model 120. Carries the inverter's electrical
//! limits (max AC power, max DC voltage, …) as scaled integers.

use crate::error::Result;
use crate::raw;
use crate::scale::ScaleFactor;
use serde::{Deserialize, Serialize};

/// Inverter sub-type. Matches the DERTyp enum16 in §C.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NameplateInverterType {
    PvInverter,
    Storage,
    Other(u16),
}

impl NameplateInverterType {
    #[must_use]
    pub const fn from_register(raw: u16) -> Self {
        match raw {
            4 => Self::PvInverter,
            82 => Self::Storage,
            other => Self::Other(other),
        }
    }
}

/// Decoded Model 120 (Nameplate).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Nameplate {
    pub inverter_type: NameplateInverterType,
    pub max_ac_power_w: f64,
    pub max_charge_w: Option<f64>,
    pub max_discharge_w: Option<f64>,
}

impl Nameplate {
    pub const MODEL_ID: u16 = 120;
    pub const MODEL_LENGTH: u16 = 26;

    /// Parse a Model 120 payload.
    pub fn parse(regs: &[u16]) -> Result<Self> {
        if regs.len() < 26 {
            return Err(crate::Error::ShortRead {
                expected: 26,
                actual: regs.len() as u16,
            });
        }
        let inverter_type = NameplateInverterType::from_register(raw::read_u16(regs, 0).unwrap_or(0));
        let w_rtg_sf = ScaleFactor::from_register(raw::read_i16(regs, 2).unwrap_or(0)).unwrap_or_default();
        let max_ac_power_w = w_rtg_sf.apply_u16(raw::read_u16(regs, 1).unwrap_or(0));
        let w_chg_rtg_sf = ScaleFactor::from_register(raw::read_i16(regs, 8).unwrap_or(0)).unwrap_or_default();
        let w_dis_chg_rtg_sf = ScaleFactor::from_register(raw::read_i16(regs, 10).unwrap_or(0)).unwrap_or_default();
        let max_charge_w = raw::read_u16(regs, 7).map(|raw_w| w_chg_rtg_sf.apply_u16(raw_w));
        let max_discharge_w = raw::read_u16(regs, 9).map(|raw_w| w_dis_chg_rtg_sf.apply_u16(raw_w));
        Ok(Self {
            inverter_type,
            max_ac_power_w,
            max_charge_w,
            max_discharge_w,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverter_type_decodes() {
        assert_eq!(
            NameplateInverterType::from_register(4),
            NameplateInverterType::PvInverter
        );
        assert_eq!(
            NameplateInverterType::from_register(82),
            NameplateInverterType::Storage
        );
        match NameplateInverterType::from_register(7) {
            NameplateInverterType::Other(7) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_basic_nameplate() {
        let mut regs = vec![0u16; 26];
        regs[0] = 4; // PV Inverter
        regs[1] = 8200; // 8200 × 10^0 = 8200 W
        regs[2] = 0; // W_RTG_SF
        let np = Nameplate::parse(&regs).unwrap();
        assert_eq!(np.inverter_type, NameplateInverterType::PvInverter);
        assert!((np.max_ac_power_w - 8200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_short_payload_errs() {
        let regs = vec![0u16; 10];
        assert!(Nameplate::parse(&regs).is_err());
    }

    #[test]
    fn nameplate_scale_factor_applied() {
        let mut regs = vec![0u16; 26];
        regs[1] = 100; // 100
        regs[2] = 2u16; // SF +2 ⇒ 100 × 10^2 = 10_000
        let np = Nameplate::parse(&regs).unwrap();
        assert!((np.max_ac_power_w - 10_000.0).abs() < f64::EPSILON);
    }
}
