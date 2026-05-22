// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec Models 101 / 102 / 103 — Single-, Split-, and Three-phase
//! inverter (integer scale factor variant).
//!
//! Source: SunSpec Inverter Model Specification (1.7), models 101-103.
//!
//! Common register layout (payload offsets):
//! ```text
//!   0  A          AC current (sum of phases)              uint16
//!   1  AphA       AC current phase A                       uint16
//!   2  AphB       AC current phase B                       uint16 (102/103)
//!   3  AphC       AC current phase C                       uint16 (103)
//!   4  A_SF       AC current scale factor                  sunssf
//!   5  PPVphAB    Phase voltage AB                         uint16 (102/103)
//!   6  PPVphBC    Phase voltage BC                         uint16 (103)
//!   7  PPVphCA    Phase voltage CA                         uint16 (103)
//!   8  PhVphA     Phase A line-neutral voltage             uint16
//!   9  PhVphB     Phase B line-neutral voltage             uint16
//!  10  PhVphC     Phase C line-neutral voltage             uint16
//!  11  V_SF       Voltage scale factor                     sunssf
//!  12  W          AC power                                 int16
//!  13  W_SF       AC power scale factor                    sunssf
//!  14  Hz         Grid frequency                           uint16
//!  15  Hz_SF      Frequency scale factor                   sunssf
//!  16  VA         Apparent power                           int16
//!  17  VA_SF                                               sunssf
//!  18  VAr        Reactive power                           int16
//!  19  VAr_SF                                              sunssf
//!  20  PF         Power factor                             int16
//!  21  PF_SF                                               sunssf
//!  22  WH         AC lifetime production                   acc32 (2 regs)
//!  24  WH_SF                                               sunssf
//!  25  DCA        DC current                               uint16
//!  26  DCA_SF                                              sunssf
//!  27  DCV        DC voltage                               uint16
//!  28  DCV_SF                                              sunssf
//!  29  DCW        DC power                                 int16
//!  30  DCW_SF                                              sunssf
//!  31  TmpCab     Cabinet temperature                      int16
//!  32  TmpSnk     Heatsink temperature                     int16
//!  33  TmpTrns    Transformer temperature                  int16
//!  34  TmpOt      Other temperature                        int16
//!  35  Tmp_SF                                              sunssf
//!  36  St         Operating state                          enum16
//!  37  StVnd                                               enum16
//!  38  Evt1       Event bitfield 1                         bitfield32
//!  40  Evt2                                                bitfield32
//!  42  EvtVnd1..  Vendor events                            bitfield32 ×4
//! ```

use crate::error::Result;
use crate::raw;
use crate::scale::ScaleFactor;
use serde::{Deserialize, Serialize};

/// Phase count of the parsed inverter model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InverterPhase {
    Single,
    Split,
    Three,
}

impl InverterPhase {
    #[must_use]
    pub const fn from_model_id(id: u16) -> Option<Self> {
        match id {
            101 => Some(Self::Single),
            102 => Some(Self::Split),
            103 => Some(Self::Three),
            _ => None,
        }
    }

    #[must_use]
    pub const fn model_id(self) -> u16 {
        match self {
            Self::Single => 101,
            Self::Split => 102,
            Self::Three => 103,
        }
    }
}

/// Operating state. Source: SunSpec spec §C.5 enum16 table for `St`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InverterStatus {
    Off,
    Sleeping,
    Starting,
    Mppt,
    Throttled,
    ShuttingDown,
    Fault,
    Standby,
    Unknown(u16),
}

impl InverterStatus {
    #[must_use]
    pub const fn from_register(raw: u16) -> Self {
        match raw {
            1 => Self::Off,
            2 => Self::Sleeping,
            3 => Self::Starting,
            4 => Self::Mppt,
            5 => Self::Throttled,
            6 => Self::ShuttingDown,
            7 => Self::Fault,
            8 => Self::Standby,
            other => Self::Unknown(other),
        }
    }

    /// Grandma-friendly label per Charter §6.3.
    #[must_use]
    pub const fn home_word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Sleeping | Self::Standby => "standby",
            Self::Starting => "starting",
            Self::Mppt | Self::Throttled => "producing",
            Self::ShuttingDown => "shutting_down",
            Self::Fault => "fault",
            Self::Unknown(_) => "unknown",
        }
    }
}

/// Decoded inverter reading. All values converted to SI / physical units
/// via their scale factors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InverterReading {
    pub phase: InverterPhase,
    /// AC active power in watts (signed; negative ⇒ consuming).
    pub ac_power_w: f64,
    /// AC current in amps (sum of phases).
    pub ac_current_a: f64,
    /// AC voltage in volts (phase A nominal).
    pub ac_voltage_v: f64,
    /// Grid frequency in Hz.
    pub frequency_hz: f64,
    /// DC voltage from the inverter (string average).
    pub dc_voltage_v: f64,
    /// DC current from the inverter.
    pub dc_current_a: f64,
    /// DC power in watts.
    pub dc_power_w: f64,
    /// Lifetime production in kWh.
    pub lifetime_kwh: f64,
    /// Cabinet temperature in °C (or `None` if not implemented).
    pub temperature_c: Option<f64>,
    pub status: InverterStatus,
}

impl InverterReading {
    /// Parse one of models 101 / 102 / 103.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsupportedModel`] if `model_id` is not
    /// 101–103, and [`crate::Error::ShortRead`] if the payload is too
    /// short to hold the model.
    pub fn parse(model_id: u16, regs: &[u16]) -> Result<Self> {
        let phase = InverterPhase::from_model_id(model_id)
            .ok_or(crate::Error::UnsupportedModel(model_id))?;
        // 50 registers is the canonical length for 103 (largest of the three);
        // we accept anything ≥ 36 since the offsets we need fit there.
        if regs.len() < 36 {
            return Err(crate::Error::ShortRead {
                expected: 50,
                actual: regs.len() as u16,
            });
        }

        let a_sf = ScaleFactor::from_register(raw::read_i16(regs, 4).unwrap_or(0)).unwrap_or_default();
        let v_sf = ScaleFactor::from_register(raw::read_i16(regs, 11).unwrap_or(0)).unwrap_or_default();
        let w_sf = ScaleFactor::from_register(raw::read_i16(regs, 13).unwrap_or(0)).unwrap_or_default();
        let hz_sf = ScaleFactor::from_register(raw::read_i16(regs, 15).unwrap_or(0)).unwrap_or_default();
        let wh_sf = ScaleFactor::from_register(raw::read_i16(regs, 24).unwrap_or(0)).unwrap_or_default();
        let dca_sf = ScaleFactor::from_register(raw::read_i16(regs, 26).unwrap_or(0)).unwrap_or_default();
        let dcv_sf = ScaleFactor::from_register(raw::read_i16(regs, 28).unwrap_or(0)).unwrap_or_default();
        let dcw_sf = ScaleFactor::from_register(raw::read_i16(regs, 30).unwrap_or(0)).unwrap_or_default();
        let tmp_sf = ScaleFactor::from_register(raw::read_i16(regs, 35).unwrap_or(0)).unwrap_or_default();

        let ac_current_a = a_sf.apply_u16(raw::read_u16(regs, 0).unwrap_or(0));
        let ac_voltage_v = v_sf.apply_u16(raw::read_u16(regs, 8).unwrap_or(0));
        let ac_power_w = w_sf.apply_i16(raw::read_i16(regs, 12).unwrap_or(0));
        let frequency_hz = hz_sf.apply_u16(raw::read_u16(regs, 14).unwrap_or(0));
        let lifetime_kwh = wh_sf.apply_u32(raw::read_acc32(regs, 22).unwrap_or(0)) / 1000.0;
        let dc_current_a = dca_sf.apply_u16(raw::read_u16(regs, 25).unwrap_or(0));
        let dc_voltage_v = dcv_sf.apply_u16(raw::read_u16(regs, 27).unwrap_or(0));
        let dc_power_w = dcw_sf.apply_i16(raw::read_i16(regs, 29).unwrap_or(0));
        let temperature_c = raw::read_i16(regs, 31).map(|t| tmp_sf.apply_i16(t));
        let status = InverterStatus::from_register(raw::read_u16(regs, 36).unwrap_or(0));

        Ok(Self {
            phase,
            ac_power_w,
            ac_current_a,
            ac_voltage_v,
            frequency_hz,
            dc_voltage_v,
            dc_current_a,
            dc_power_w,
            lifetime_kwh,
            temperature_c,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_model_103(power_w: i16, w_sf: i16, voltage_v: u16, v_sf: i16) -> Vec<u16> {
        let mut regs = vec![0u16; 50];
        regs[0] = 1500; // A
        regs[4] = (-1i16) as u16; // A_SF -1 ⇒ × 0.1
        regs[8] = voltage_v; // PhVphA
        regs[11] = v_sf as u16; // V_SF
        regs[12] = power_w as u16; // W
        regs[13] = w_sf as u16; // W_SF
        regs[14] = 5000; // Hz
        regs[15] = (-2i16) as u16; // Hz_SF -2 ⇒ × 0.01 ⇒ 50.00 Hz
        // WH (acc32) — 1_000_000 Wh = 1000 kWh @ SF 0
        regs[22] = 0x000F;
        regs[23] = 0x4240; // 0x000F4240 == 1_000_000
        regs[24] = 0; // WH_SF
        regs[25] = 80; // DCA
        regs[26] = (-1i16) as u16; // DCA_SF -1
        regs[27] = 4000; // DCV
        regs[28] = (-1i16) as u16; // DCV_SF -1 ⇒ 400.0 V
        regs[29] = power_w as u16; // DCW
        regs[30] = w_sf as u16; // DCW_SF
        regs[31] = 350; // TmpCab
        regs[35] = (-1i16) as u16; // Tmp_SF -1 ⇒ 35.0 °C
        regs[36] = 4; // St == MPPT
        regs
    }

    #[test]
    fn parse_three_phase_basic() {
        let regs = build_model_103(7500, 0, 2300, -1); // 7500 W, 230.0 V
        let r = InverterReading::parse(103, &regs).unwrap();
        assert_eq!(r.phase, InverterPhase::Three);
        assert!((r.ac_power_w - 7500.0).abs() < f64::EPSILON);
        assert!((r.ac_voltage_v - 230.0).abs() < f64::EPSILON);
        assert!((r.frequency_hz - 50.0).abs() < f64::EPSILON);
        assert!((r.lifetime_kwh - 1000.0).abs() < f64::EPSILON);
        assert_eq!(r.status, InverterStatus::Mppt);
    }

    #[test]
    fn parse_single_phase_model_101() {
        let regs = build_model_103(1500, 0, 2300, -1);
        let r = InverterReading::parse(101, &regs).unwrap();
        assert_eq!(r.phase, InverterPhase::Single);
    }

    #[test]
    fn parse_unsupported_model_id() {
        let regs = vec![0u16; 50];
        let r = InverterReading::parse(999, &regs);
        assert!(matches!(r, Err(crate::Error::UnsupportedModel(999))));
    }

    #[test]
    fn parse_short_payload_errs() {
        let regs = vec![0u16; 10];
        let r = InverterReading::parse(103, &regs);
        assert!(matches!(r, Err(crate::Error::ShortRead { .. })));
    }

    #[test]
    fn status_home_word_mapping() {
        assert_eq!(InverterStatus::Mppt.home_word(), "producing");
        assert_eq!(InverterStatus::Off.home_word(), "off");
        assert_eq!(InverterStatus::Fault.home_word(), "fault");
        assert_eq!(InverterStatus::Sleeping.home_word(), "standby");
    }

    #[test]
    fn temperature_optional() {
        let mut regs = build_model_103(7500, 0, 2300, -1);
        regs[31] = i16::MIN as u16; // sentinel
        let r = InverterReading::parse(103, &regs).unwrap();
        assert!(r.temperature_c.is_none());
    }
}
