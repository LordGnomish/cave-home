// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec meter models — integer (201/202/203) three-phase meters.
//!
//! Source: SunSpec Information Model Specification, meter models.
//!
//! Like the integer inverter models, every measurement is a raw integer paired
//! with a `sunssf` power-of-ten scale factor; the energy counters are `acc32`.
//! This module decodes Model 203 ("Wye-Connect Three Phase (Abcn) Meter") into
//! one [`MeterReading`], mirroring [`crate::inverter::InverterReading`].
//!
//! Integer-model payload layout (register offsets, 0-based within the payload):
//! ```text
//!    0  A         Total AC current               int16   (A_SF @ 4)
//!    4  A_SF      AC current scale factor         sunssf
//!   18  W         Total real power                int16   (W_SF @ 22)
//!   22  W_SF      Real power scale factor         sunssf
//!  152  TotWhExp  Total real energy exported      acc32 (2 regs, hi word first)
//!  154  TotWhImp  Total real energy imported      acc32 (2 regs, hi word first)
//!  158  TotWh_SF  Real energy scale factor        sunssf
//!  164  Evt       Meter event flags               bitfield32 (2 regs, hi first)
//! ```

use crate::fault::DecodeError;
use crate::point;
use crate::scale::ScaleFactor;

/// A decoded meter reading in physical units. All measurements have had their
/// scale factors applied. `None` means the device did not implement that point
/// (a SunSpec sentinel).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterReading {
    /// Total real (active) power, watts (signed; sign convention is the
    /// meter's).
    pub total_real_power_w: Option<f64>,
    /// Total real energy imported (consumed from the grid), watt-hours.
    pub total_energy_imported_wh: Option<f64>,
    /// Total real energy exported (fed to the grid), watt-hours.
    pub total_energy_exported_wh: Option<f64>,
    /// Whether any meter event flag is set.
    pub has_event: bool,
}

impl MeterReading {
    /// Minimum payload length to reach the event block. `Evt` occupies offsets
    /// 164 and 165, so 166 registers is the minimum useful payload.
    const INT_MIN_LEN: usize = 166;

    /// Decode an integer meter model. Currently only Model 203 is supported.
    ///
    /// # Errors
    /// [`DecodeError::UnsupportedModel`] if `model_id` is not 203, and
    /// [`DecodeError::OutOfBounds`] if the payload is too short to reach the
    /// event block.
    pub fn decode_integer(model_id: u16, payload: &[u16]) -> Result<Self, DecodeError> {
        if model_id != 203 {
            return Err(DecodeError::UnsupportedModel { model_id });
        }

        if payload.len() < Self::INT_MIN_LEN {
            return Err(DecodeError::OutOfBounds {
                offset: Self::INT_MIN_LEN,
                len: payload.len(),
            });
        }

        let sf = |off| -> ScaleFactor {
            point::sunssf(payload, off)
                .ok()
                .flatten()
                .map_or_else(ScaleFactor::unity, ScaleFactor::new)
        };

        let w_sf = sf(22);
        let energy_sf = sf(158);

        let total_real_power_w = point::int16(payload, 18)?.map(|v| w_sf.apply_i16(v));
        let total_energy_exported_wh = point::acc32(payload, 152)?.map(|v| energy_sf.apply_u32(v));
        let total_energy_imported_wh = point::acc32(payload, 154)?.map(|v| energy_sf.apply_u32(v));

        // Meter event flags: a bitfield32 (hi word first). The "no events"
        // value is all-zero, so read the raw 32 bits and test for any set bit.
        let evt_raw = point::uint32(payload, 164)?.unwrap_or_else(|| {
            // `uint32` returns None on the 0xFFFF_FFFF sentinel; for a bitfield
            // that still means "all bits set" => an event is present.
            u32::MAX
        });
        let has_event = evt_raw != 0;

        Ok(Self {
            total_real_power_w,
            total_energy_imported_wh,
            total_energy_exported_wh,
            has_event,
        })
    }
}

/// Decode any supported meter model from its payload, dispatching on the model
/// id. Currently only Model 203 is supported.
///
/// # Errors
/// [`DecodeError::UnsupportedModel`] if `model_id` is not a known meter model,
/// and [`DecodeError::OutOfBounds`] if the payload is too short.
pub fn decode_meter(model_id: u16, payload: &[u16]) -> Result<MeterReading, DecodeError> {
    match model_id {
        203 => MeterReading::decode_integer(model_id, payload),
        _ => Err(DecodeError::UnsupportedModel { model_id }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_meter_203() -> Vec<u16> {
        let mut p = vec![0u16; 166];
        p[18] = 1500u16; // W raw
        p[22] = 0; // W_SF
        p[152] = 0x0000;
        p[153] = 0x2710; // TotWhExp = 10_000
        p[154] = 0x0001;
        p[155] = 0x86A0; // TotWhImp = 100_000
        p[158] = 0; // TotWh_SF
        p[164] = 0;
        p[165] = 0;
        p
    }

    #[test]
    fn power_scaled() {
        let p = build_meter_203();
        let r = MeterReading::decode_integer(203, &p).unwrap();
        assert!((r.total_real_power_w.unwrap() - 1500.0).abs() < 1e-6);
    }

    #[test]
    fn unimplemented_power_is_none() {
        let mut p = build_meter_203();
        p[18] = point::INT16_NA;
        let r = MeterReading::decode_integer(203, &p).unwrap();
        assert!(r.total_real_power_w.is_none());
    }

    #[test]
    fn event_bit_present() {
        let mut p = build_meter_203();
        p[165] = 0x0004;
        let r = MeterReading::decode_integer(203, &p).unwrap();
        assert!(r.has_event);
    }

    #[test]
    fn wrong_model_rejected() {
        let p = build_meter_203();
        assert!(matches!(
            MeterReading::decode_integer(103, &p),
            Err(DecodeError::UnsupportedModel { model_id: 103 })
        ));
    }

    #[test]
    fn truncated_is_out_of_bounds() {
        let p = vec![0u16; 30];
        assert!(matches!(
            decode_meter(203, &p),
            Err(DecodeError::OutOfBounds { .. })
        ));
    }
}
