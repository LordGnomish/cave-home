// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec Model 160 — Multiple MPPT inverter extension.
//!
//! Source: SunSpec spec model 160 — per-MPPT-string current/voltage/power.

use crate::error::Result;
use crate::raw;
use crate::scale::ScaleFactor;
use serde::{Deserialize, Serialize};

/// One MPPT (Maximum Power Point Tracker) module's reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MpptModule {
    pub id: u16,
    pub name: String,
    pub dc_current_a: f64,
    pub dc_voltage_v: f64,
    pub dc_power_w: f64,
    pub lifetime_kwh: f64,
}

/// Full Model 160 reading: shared scale factors + per-module entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MpptReading {
    pub modules: Vec<MpptModule>,
}

impl MpptReading {
    pub const MODEL_ID: u16 = 160;

    /// Each module occupies 20 registers in the repeat block.
    pub const MODULE_BLOCK_LEN: usize = 20;
    /// Fixed-block size before the repeat blocks.
    pub const FIXED_BLOCK_LEN: usize = 8;

    /// Parse a Model 160 payload. Up to `max_modules` modules will be
    /// decoded; missing modules (identifier == 0xFFFF) are skipped.
    ///
    /// # Errors
    ///
    /// [`crate::Error::ShortRead`] if the payload is shorter than the
    /// fixed block.
    pub fn parse(regs: &[u16], max_modules: usize) -> Result<Self> {
        if regs.len() < Self::FIXED_BLOCK_LEN {
            return Err(crate::Error::ShortRead {
                expected: Self::FIXED_BLOCK_LEN as u16,
                actual: regs.len() as u16,
            });
        }
        let dca_sf = ScaleFactor::from_register(raw::read_i16(regs, 1).unwrap_or(0)).unwrap_or_default();
        let dcv_sf = ScaleFactor::from_register(raw::read_i16(regs, 2).unwrap_or(0)).unwrap_or_default();
        let dcw_sf = ScaleFactor::from_register(raw::read_i16(regs, 3).unwrap_or(0)).unwrap_or_default();
        let dcwh_sf = ScaleFactor::from_register(raw::read_i16(regs, 4).unwrap_or(0)).unwrap_or_default();

        let mut modules = Vec::with_capacity(max_modules);
        for n in 0..max_modules {
            let block_start = Self::FIXED_BLOCK_LEN + n * Self::MODULE_BLOCK_LEN;
            if block_start + Self::MODULE_BLOCK_LEN > regs.len() {
                break;
            }
            let block = &regs[block_start..block_start + Self::MODULE_BLOCK_LEN];
            let Some(id) = raw::read_u16(block, 0) else {
                continue;
            };
            let name = raw::read_string(block, 1, 8, "MpptName").unwrap_or_default();
            let dc_current_a = dca_sf.apply_u16(raw::read_u16(block, 9).unwrap_or(0));
            let dc_voltage_v = dcv_sf.apply_u16(raw::read_u16(block, 10).unwrap_or(0));
            let dc_power_w = dcw_sf.apply_u16(raw::read_u16(block, 11).unwrap_or(0));
            let lifetime_wh = raw::read_acc32(block, 12).unwrap_or(0);
            let lifetime_kwh = dcwh_sf.apply_u32(lifetime_wh) / 1000.0;
            modules.push(MpptModule {
                id,
                name,
                dc_current_a,
                dc_voltage_v,
                dc_power_w,
                lifetime_kwh,
            });
        }

        Ok(Self { modules })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_one_module() -> Vec<u16> {
        let mut regs = vec![0u16; MpptReading::FIXED_BLOCK_LEN + MpptReading::MODULE_BLOCK_LEN];
        // Fixed block
        regs[1] = (-1i16) as u16; // DCA_SF -1
        regs[2] = (-1i16) as u16; // DCV_SF -1
        regs[3] = 0u16; // DCW_SF 0
        regs[4] = 0u16; // DCWH_SF 0
        // Module 0 starts at offset 8
        let m0 = MpptReading::FIXED_BLOCK_LEN;
        regs[m0] = 1; // ID
        // Name "MPPT_A" padded over 8 registers (16 bytes)
        let name_bytes = b"MPPT_A\0\0\0\0\0\0\0\0\0\0";
        for (i, c) in name_bytes.chunks(2).enumerate() {
            regs[m0 + 1 + i] = (u16::from(c[0]) << 8) | u16::from(c[1]);
        }
        regs[m0 + 9] = 80; // 8.0 A
        regs[m0 + 10] = 4000; // 400.0 V
        regs[m0 + 11] = 3200; // 3200 W
        regs[m0 + 12] = 0;
        regs[m0 + 13] = 5000; // 5000 Wh
        regs
    }

    #[test]
    fn parses_one_module_block() {
        let regs = build_one_module();
        let r = MpptReading::parse(&regs, 4).unwrap();
        assert_eq!(r.modules.len(), 1);
        let m = &r.modules[0];
        assert_eq!(m.id, 1);
        assert!(m.name.starts_with("MPPT_A"));
        assert!((m.dc_current_a - 8.0).abs() < 1e-6);
        assert!((m.dc_voltage_v - 400.0).abs() < 1e-6);
        assert!((m.dc_power_w - 3200.0).abs() < f64::EPSILON);
        assert!((m.lifetime_kwh - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn short_fixed_block_errs() {
        let regs = vec![0u16; 4];
        assert!(MpptReading::parse(&regs, 4).is_err());
    }

    #[test]
    fn module_id_sentinel_skipped() {
        let mut regs = build_one_module();
        regs[MpptReading::FIXED_BLOCK_LEN] = 0xFFFF;
        let r = MpptReading::parse(&regs, 4).unwrap();
        assert!(r.modules.is_empty());
    }

    #[test]
    fn truncated_after_first_module_does_not_panic() {
        let regs = build_one_module();
        let r = MpptReading::parse(&regs, 8).unwrap();
        // Asked for 8, only 1 present.
        assert_eq!(r.modules.len(), 1);
    }
}
