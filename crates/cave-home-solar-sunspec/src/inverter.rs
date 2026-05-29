// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec inverter models — integer (101/102/103) and float (111/112/113).
//!
//! Source: SunSpec Information Model Specification, inverter models.
//!
//! The integer models 101/102/103 (single / split / three phase) and the
//! float models 111/112/113 share the same point *layout* — the only
//! difference is whether each measurement is an integer-with-scale-factor or
//! an IEEE-754 `float32`. This module decodes both into one
//! [`InverterReading`].
//!
//! Integer-model payload layout (register offsets):
//! ```text
//!   0  A        AC current (sum of phases)   uint16
//!   4  A_SF     AC current scale factor      sunssf
//!   8  PhVphA   Phase-A line-neutral voltage uint16
//!  11  V_SF     Voltage scale factor         sunssf
//!  12  W        AC power                     int16
//!  13  W_SF     AC power scale factor        sunssf
//!  14  Hz       Grid frequency               uint16
//!  15  Hz_SF    Frequency scale factor       sunssf
//!  22  WH       Lifetime energy              acc32 (2 regs)
//!  24  WH_SF    Energy scale factor          sunssf
//!  25  DCA      DC current                   uint16
//!  26  DCA_SF   DC current scale factor      sunssf
//!  27  DCV      DC voltage                   uint16
//!  28  DCV_SF   DC voltage scale factor      sunssf
//!  29  DCW      DC power                     int16
//!  30  DCW_SF   DC power scale factor        sunssf
//!  31  TmpCab   Cabinet temperature          int16
//!  35  Tmp_SF   Temperature scale factor     sunssf
//!  36  St       Operating state              enum16
//! ```
//!
//! The float models 111/112/113 widen every measurement to `float32`
//! (2 registers each) and carry no scale-factor points; their `St` operating
//! state sits at a different offset. cave-home decodes the float variant via
//! [`InverterReading::decode_float`].

use crate::fault::DecodeError;
use crate::point;
use crate::scale::ScaleFactor;

/// Phase topology of the decoded inverter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InverterPhase {
    Single,
    Split,
    Three,
}

impl InverterPhase {
    /// Map a model id to its phase topology, for both the integer (101-103)
    /// and float (111-113) families.
    #[must_use]
    pub const fn from_model_id(id: u16) -> Option<Self> {
        match id {
            101 | 111 => Some(Self::Single),
            102 | 112 => Some(Self::Split),
            103 | 113 => Some(Self::Three),
            _ => None,
        }
    }
}

/// SunSpec inverter operating state (`St`, enum16).
///
/// Source: SunSpec inverter model `St` enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingState {
    /// `1` OFF — inverter shut off.
    Off,
    /// `2` SLEEPING — auto-shutdown, e.g. no sun.
    Sleeping,
    /// `3` STARTING — coming online.
    Starting,
    /// `4` MPPT — tracking maximum power point (producing normally).
    Mppt,
    /// `5` THROTTLED — producing but power-limited.
    Throttled,
    /// `6` SHUTTING_DOWN.
    ShuttingDown,
    /// `7` FAULT — an error condition.
    Fault,
    /// `8` STANDBY — held off, ready to start.
    Standby,
    /// A vendor / future state outside the standard enumeration.
    Other(u16),
}

impl OperatingState {
    /// Decode the raw `St` register value.
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
            other => Self::Other(other),
        }
    }

    /// Whether the inverter is actively producing power.
    #[must_use]
    pub const fn is_producing(self) -> bool {
        matches!(self, Self::Mppt | Self::Throttled)
    }

    /// Whether the inverter is in a fault condition needing attention.
    #[must_use]
    pub const fn is_fault(self) -> bool {
        matches!(self, Self::Fault)
    }
}

/// A decoded inverter reading in physical units. All measurements have had
/// their scale factors applied. `None` means the device did not implement
/// that point (a SunSpec sentinel).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InverterReading {
    pub phase: InverterPhase,
    /// AC active power, watts (signed; negative ⇒ importing).
    pub ac_power_w: Option<f64>,
    /// AC current, amps (sum of phases).
    pub ac_current_a: Option<f64>,
    /// AC voltage, volts (phase A line-to-neutral).
    pub ac_voltage_v: Option<f64>,
    /// Grid frequency, hertz.
    pub frequency_hz: Option<f64>,
    /// DC power, watts.
    pub dc_power_w: Option<f64>,
    /// Lifetime energy produced, watt-hours.
    pub lifetime_energy_wh: Option<f64>,
    /// Cabinet temperature, degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Operating state.
    pub state: OperatingState,
}

impl InverterReading {
    /// Minimum payload length for the integer models (through `St` at 36).
    const INT_MIN_LEN: usize = 37;
    /// `St` register offset within the float models 111/112/113.
    const FLOAT_ST_OFFSET: usize = 38;
    /// Minimum payload length for the float models (through `St` at 38).
    const FLOAT_MIN_LEN: usize = 39;

    /// Decode an integer inverter model (101 / 102 / 103).
    ///
    /// # Errors
    /// [`DecodeError::UnsupportedModel`] if `model_id` is not 101/102/103,
    /// and [`DecodeError::OutOfBounds`] if the payload is too short.
    pub fn decode_integer(model_id: u16, payload: &[u16]) -> Result<Self, DecodeError> {
        let phase = match model_id {
            101..=103 => InverterPhase::from_model_id(model_id),
            _ => None,
        }
        .ok_or(DecodeError::UnsupportedModel { model_id })?;

        if payload.len() < Self::INT_MIN_LEN {
            return Err(DecodeError::OutOfBounds { offset: Self::INT_MIN_LEN, len: payload.len() });
        }

        let sf = |off| -> ScaleFactor {
            point::sunssf(payload, off)
                .ok()
                .flatten()
                .map_or_else(ScaleFactor::unity, ScaleFactor::new)
        };

        let a_sf = sf(4);
        let v_sf = sf(11);
        let w_sf = sf(13);
        let hz_sf = sf(15);
        let energy_sf = sf(24);
        let dcw_sf = sf(30);
        let tmp_sf = sf(35);

        let ac_current_a = point::uint16(payload, 0)?.map(|v| a_sf.apply_u16(v));
        let ac_voltage_v = point::uint16(payload, 8)?.map(|v| v_sf.apply_u16(v));
        let ac_power_w = point::int16(payload, 12)?.map(|v| w_sf.apply_i16(v));
        let frequency_hz = point::uint16(payload, 14)?.map(|v| hz_sf.apply_u16(v));
        let lifetime_energy_wh = point::acc32(payload, 22)?.map(|v| energy_sf.apply_u32(v));
        let dc_power_w = point::int16(payload, 29)?.map(|v| dcw_sf.apply_i16(v));
        let temperature_c = point::int16(payload, 31)?.map(|v| tmp_sf.apply_i16(v));
        let state = OperatingState::from_register(point::uint16(payload, 36)?.unwrap_or(0));

        Ok(Self {
            phase,
            ac_power_w,
            ac_current_a,
            ac_voltage_v,
            frequency_hz,
            dc_power_w,
            lifetime_energy_wh,
            temperature_c,
            state,
        })
    }

    /// Decode a float inverter model (111 / 112 / 113). Every measurement is
    /// an IEEE-754 `float32` already in physical units — no scale factors.
    ///
    /// # Errors
    /// [`DecodeError::UnsupportedModel`] if `model_id` is not 111/112/113,
    /// and [`DecodeError::OutOfBounds`] if the payload is too short.
    pub fn decode_float(model_id: u16, payload: &[u16]) -> Result<Self, DecodeError> {
        let phase = match model_id {
            111..=113 => InverterPhase::from_model_id(model_id),
            _ => None,
        }
        .ok_or(DecodeError::UnsupportedModel { model_id })?;

        if payload.len() < Self::FLOAT_MIN_LEN {
            return Err(DecodeError::OutOfBounds { offset: Self::FLOAT_MIN_LEN, len: payload.len() });
        }

        let f = |off| -> Result<Option<f64>, DecodeError> {
            Ok(point::float32(payload, off)?.map(f64::from))
        };

        let ac_current_a = f(0)?;
        let ac_voltage_v = f(8)?;
        let ac_power_w = f(12)?;
        let frequency_hz = f(14)?;
        let lifetime_energy_wh = f(22)?;
        let dc_power_w = f(29)?;
        let temperature_c = f(31)?;
        let state = OperatingState::from_register(
            point::uint16(payload, Self::FLOAT_ST_OFFSET)?.unwrap_or(0),
        );

        Ok(Self {
            phase,
            ac_power_w,
            ac_current_a,
            ac_voltage_v,
            frequency_hz,
            dc_power_w,
            lifetime_energy_wh,
            temperature_c,
            state,
        })
    }

    /// Lifetime energy in kilowatt-hours, if implemented.
    #[must_use]
    pub fn lifetime_energy_kwh(&self) -> Option<f64> {
        self.lifetime_energy_wh.map(|wh| wh / 1000.0)
    }

    /// AC power in kilowatts, if implemented.
    #[must_use]
    pub fn ac_power_kw(&self) -> Option<f64> {
        self.ac_power_w.map(|w| w / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a model-103 integer payload producing `power_w` watts at
    /// `230.x` V, `50.00` Hz, with a lifetime counter and cabinet temp.
    fn build_int_103() -> Vec<u16> {
        let mut p = vec![0u16; 50];
        p[0] = 326; // A raw
        p[4] = (-1i16) as u16; // A_SF -1 ⇒ 32.6 A
        p[8] = 2301; // PhVphA raw
        p[11] = (-1i16) as u16; // V_SF -1 ⇒ 230.1 V
        p[12] = 7500u16; // W raw
        p[13] = 0; // W_SF 0 ⇒ 7500 W
        p[14] = 5000; // Hz raw
        p[15] = (-2i16) as u16; // Hz_SF -2 ⇒ 50.00 Hz
        p[22] = 0x000F; // WH hi
        p[23] = 0x4240; // WH lo ⇒ 1_000_000 Wh
        p[24] = 0; // WH_SF 0
        p[29] = 7700u16; // DCW raw
        p[30] = 0; // DCW_SF 0 ⇒ 7700 W
        p[31] = 350; // TmpCab raw
        p[35] = (-1i16) as u16; // Tmp_SF -1 ⇒ 35.0 °C
        p[36] = 4; // St == MPPT
        p
    }

    #[test]
    fn decode_three_phase_integer() {
        let p = build_int_103();
        let r = InverterReading::decode_integer(103, &p).unwrap();
        assert_eq!(r.phase, InverterPhase::Three);
        assert!((r.ac_power_w.unwrap() - 7500.0).abs() < 1e-6);
        assert!((r.ac_current_a.unwrap() - 32.6).abs() < 1e-6);
        assert!((r.ac_voltage_v.unwrap() - 230.1).abs() < 1e-6);
        assert!((r.frequency_hz.unwrap() - 50.0).abs() < 1e-6);
        assert!((r.dc_power_w.unwrap() - 7700.0).abs() < 1e-6);
        assert!((r.lifetime_energy_wh.unwrap() - 1_000_000.0).abs() < 1e-6);
        assert!((r.lifetime_energy_kwh().unwrap() - 1000.0).abs() < 1e-6);
        assert!((r.temperature_c.unwrap() - 35.0).abs() < 1e-6);
        assert_eq!(r.state, OperatingState::Mppt);
        assert!(r.state.is_producing());
    }

    #[test]
    fn single_phase_model_101_topology() {
        let p = build_int_103();
        let r = InverterReading::decode_integer(101, &p).unwrap();
        assert_eq!(r.phase, InverterPhase::Single);
    }

    #[test]
    fn split_phase_model_102_topology() {
        let p = build_int_103();
        let r = InverterReading::decode_integer(102, &p).unwrap();
        assert_eq!(r.phase, InverterPhase::Split);
    }

    #[test]
    fn unsupported_model_id_rejected() {
        let p = vec![0u16; 50];
        assert!(matches!(
            InverterReading::decode_integer(124, &p),
            Err(DecodeError::UnsupportedModel { model_id: 124 })
        ));
    }

    #[test]
    fn short_payload_rejected_not_panicked() {
        let p = vec![0u16; 10];
        assert!(matches!(
            InverterReading::decode_integer(103, &p),
            Err(DecodeError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn unimplemented_temperature_is_none() {
        let mut p = build_int_103();
        p[31] = point::INT16_NA; // sentinel
        let r = InverterReading::decode_integer(103, &p).unwrap();
        assert!(r.temperature_c.is_none());
    }

    #[test]
    fn never_accumulated_energy_is_none() {
        let mut p = build_int_103();
        p[22] = 0;
        p[23] = 0;
        let r = InverterReading::decode_integer(103, &p).unwrap();
        assert!(r.lifetime_energy_wh.is_none());
    }

    #[test]
    fn negative_power_means_importing() {
        let mut p = build_int_103();
        p[12] = (-200i16) as u16; // W raw
        p[13] = 0;
        let r = InverterReading::decode_integer(103, &p).unwrap();
        assert!((r.ac_power_w.unwrap() + 200.0).abs() < 1e-6);
    }

    #[test]
    fn state_mapping_covers_sleeping_and_fault() {
        let mut p = build_int_103();
        p[36] = 2; // sleeping
        let r = InverterReading::decode_integer(103, &p).unwrap();
        assert_eq!(r.state, OperatingState::Sleeping);
        assert!(!r.state.is_producing());
        p[36] = 7; // fault
        let r = InverterReading::decode_integer(103, &p).unwrap();
        assert!(r.state.is_fault());
        p[36] = 99; // vendor / future
        let r = InverterReading::decode_integer(103, &p).unwrap();
        assert_eq!(r.state, OperatingState::Other(99));
    }

    /// Build a float model-111 payload (single phase) with float32 points.
    fn build_float_111() -> Vec<u16> {
        let mut p = vec![0u16; 60];
        let put = |p: &mut Vec<u16>, off: usize, v: f32| {
            let b = v.to_bits();
            p[off] = (b >> 16) as u16;
            p[off + 1] = (b & 0xFFFF) as u16;
        };
        put(&mut p, 0, 13.6); // A
        put(&mut p, 8, 240.2); // PhVphA
        put(&mut p, 12, 3200.0); // W
        put(&mut p, 14, 60.01); // Hz
        put(&mut p, 22, 250_000.0); // WH
        put(&mut p, 29, 3300.0); // DCW
        put(&mut p, 31, 41.5); // TmpCab
        p[38] = 4; // St == MPPT
        p
    }

    #[test]
    fn decode_float_model_111() {
        let p = build_float_111();
        let r = InverterReading::decode_float(111, &p).unwrap();
        assert_eq!(r.phase, InverterPhase::Single);
        assert!((r.ac_power_w.unwrap() - 3200.0).abs() < 1e-3);
        assert!((r.ac_voltage_v.unwrap() - 240.2).abs() < 1e-3);
        assert!((r.frequency_hz.unwrap() - 60.01).abs() < 1e-3);
        assert!((r.lifetime_energy_kwh().unwrap() - 250.0).abs() < 1e-3);
        assert_eq!(r.state, OperatingState::Mppt);
    }

    #[test]
    fn float_model_unimplemented_point_is_none() {
        let mut p = build_float_111();
        let nan = f32::NAN.to_bits();
        p[31] = (nan >> 16) as u16;
        p[32] = (nan & 0xFFFF) as u16;
        let r = InverterReading::decode_float(111, &p).unwrap();
        assert!(r.temperature_c.is_none());
    }

    #[test]
    fn float_model_wrong_id_rejected() {
        let p = build_float_111();
        assert!(matches!(
            InverterReading::decode_float(103, &p),
            Err(DecodeError::UnsupportedModel { model_id: 103 })
        ));
    }
}
