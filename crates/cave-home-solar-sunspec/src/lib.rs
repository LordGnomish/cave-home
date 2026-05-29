// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-solar-sunspec` — a vendor-agnostic SunSpec register-map decoder
//! for solar-inverter monitoring.
//!
//! # What this is
//!
//! SunSpec is an **open, public standard** maintained by the SunSpec
//! Alliance (<https://sunspec.org/sunspec-information-model-specifications/>).
//! It defines how solar inverters from SMA, Fronius, SolarEdge, Huawei,
//! Goodwe, Kostal and others expose their data as a chain of typed *models*
//! laid out in Modbus holding registers. This crate decodes that register map
//! into typed, physical-unit readings — and then into plain sentences a
//! household understands.
//!
//! Everything here is **pure decode logic over a `&[u16]`**: no network, no
//! serial port, no hardware. A caller reads the holding registers however it
//! likes (TCP, RTU, a test fixture) and hands the words to this crate. The
//! live transport and polling loop are deferred to Phase-1b — see the parity
//! manifest.
//!
//! # Pipeline
//!
//! 1. [`discovery`] — verify the `"SunS"` marker and walk the model chain to
//!    find each `(model_id, length)` block.
//! 2. [`point`] — decode SunSpec point types (`int16`, `uint16`, `int32`,
//!    `uint32`, `acc32`, `float32`, `sunssf`, `string`), honouring the
//!    not-implemented sentinels.
//! 3. [`scale`] — apply a `sunssf` power-of-ten scale factor to a raw point.
//! 4. [`common`] — decode Model 1 (manufacturer / model / version / serial).
//! 5. [`inverter`] — decode the inverter models (101/102/103 integer,
//!    111/112/113 float) into an [`InverterReading`].
//! 6. [`label`] — turn a reading into a grandma-friendly EN/DE/TR sentence.
//!
//! # Charter §6.3 grandma-friendly UX
//!
//! The decode layer speaks the protocol; the [`label`] layer speaks the home.
//! End-user strings say "Solar inverter is producing 3.2 kW", never a model
//! number or a register address.
//!
//! # Example
//!
//! ```
//! use cave_home_solar_sunspec::{discover, decode_inverter, describe, Lang};
//!
//! // A SunSpec block read from holding registers: the "SunS" marker, a
//! // model-103 (three-phase inverter) header + payload, then the end sentinel.
//! let mut block = vec![0x5375u16, 0x6e53]; // "SunS"
//! block.push(103);                          // model id
//! block.push(50);                           // payload length
//! let mut payload = vec![0u16; 50];
//! payload[12] = 7500;                       // W raw
//! payload[13] = 0;                          // W_SF = 0
//! payload[36] = 4;                          // St = MPPT (producing)
//! block.extend_from_slice(&payload);
//! block.push(0xFFFF);                        // end of chain
//!
//! let models = discover(&block).expect("valid SunSpec block");
//! let inv = models.iter().find(|m| m.model_id == 103).expect("an inverter");
//! let reading = decode_inverter(inv.model_id, inv.payload).expect("decodes");
//!
//! assert_eq!(reading.ac_power_kw(), Some(7.5));
//! assert_eq!(describe(&reading, Lang::En), "Solar inverter is producing 7.5 kW");
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]

pub mod common;
pub mod discovery;
pub mod fault;
pub mod inverter;
pub mod label;
pub mod point;
pub mod scale;

pub use common::{CommonModel, InverterFamily};
pub use discovery::{DiscoveredModel, SUNSPEC_END_MODEL_ID, SUNSPEC_MARKER, discover};
pub use fault::DecodeError;
pub use inverter::{InverterPhase, InverterReading, OperatingState};
pub use label::{Lang, SolarStatus, describe};
pub use scale::ScaleFactor;

/// Decode any supported inverter model — integer (101/102/103) or float
/// (111/112/113) — from its payload, dispatching on the model id.
///
/// # Errors
/// [`DecodeError::UnsupportedModel`] if `model_id` is not a known inverter
/// model, and [`DecodeError::OutOfBounds`] if the payload is too short.
pub fn decode_inverter(model_id: u16, payload: &[u16]) -> Result<InverterReading, DecodeError> {
    match model_id {
        101..=103 => InverterReading::decode_integer(model_id, payload),
        111..=113 => InverterReading::decode_float(model_id, payload),
        _ => Err(DecodeError::UnsupportedModel { model_id }),
    }
}

/// Well-known TCP base registers where a SunSpec device places its `"SunS"`
/// marker.
///
/// A live transport (Phase-1b) probes these in order; the decode layer is
/// address-agnostic. Source: SunSpec Modbus specification.
pub const SUNSPEC_BASE_REGISTERS: &[u16] = &[40_000, 50_000, 0];

#[cfg(test)]
mod tests {
    use super::*;

    fn build_block_with_inverter(model_id: u16, payload: &[u16]) -> Vec<u16> {
        let mut b = vec![(SUNSPEC_MARKER >> 16) as u16, (SUNSPEC_MARKER & 0xFFFF) as u16];
        // a common model first
        let common = vec![0u16; 65];
        b.push(1);
        b.push(common.len() as u16);
        b.extend_from_slice(&common);
        // then the inverter
        b.push(model_id);
        b.push(payload.len() as u16);
        b.extend_from_slice(payload);
        b.push(SUNSPEC_END_MODEL_ID);
        b
    }

    #[test]
    fn end_to_end_discover_then_decode() {
        let mut payload = vec![0u16; 50];
        payload[12] = 4200; // W
        payload[13] = 0; // W_SF
        payload[36] = 4; // MPPT
        let block = build_block_with_inverter(103, &payload);

        let models = discover(&block).unwrap();
        assert_eq!(models.len(), 2);
        let inv = models.iter().find(|m| m.model_id == 103).unwrap();
        let reading = decode_inverter(inv.model_id, inv.payload).unwrap();
        assert_eq!(reading.ac_power_w, Some(4200.0));
        assert_eq!(describe(&reading, Lang::En), "Solar inverter is producing 4.2 kW");
    }

    #[test]
    fn decode_inverter_rejects_non_inverter_model() {
        assert!(matches!(
            decode_inverter(1, &[0u16; 65]),
            Err(DecodeError::UnsupportedModel { model_id: 1 })
        ));
    }

    #[test]
    fn float_model_dispatches_to_float_decoder() {
        let mut p = vec![0u16; 60];
        let bits = 2500.0f32.to_bits();
        p[12] = (bits >> 16) as u16;
        p[13] = (bits & 0xFFFF) as u16;
        p[38] = 4; // MPPT
        let r = decode_inverter(111, &p).unwrap();
        assert!((r.ac_power_w.unwrap() - 2500.0).abs() < 1e-3);
    }
}
