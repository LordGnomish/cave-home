// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec Model 1 — Common (manufacturer, model, version, serial).
//!
//! Source: SunSpec Information Model Specification, model 1 definition.
//!
//! Payload layout (register offsets from the start of the model payload):
//! ```text
//!     0    Mn  (manufacturer)            16 registers (32-byte ASCII)
//!    16    Md  (model)                   16 registers
//!    32    Opt (options)                  8 registers
//!    40    Vr  (version)                  8 registers
//!    48    SN  (serial number)           16 registers
//!    64    DA  (Modbus device address)    1 register
//! ```
//! Canonical payload length is 66 registers (id 1, length 66). cave-home
//! reads the first 65 it needs and tolerates the device-address word being
//! absent.

use crate::fault::DecodeError;
use crate::point;

/// Inverter family inferred from the Model 1 manufacturer string. cave-home
/// uses this only for vendor labelling and quirk selection; it never reaches
/// the end-user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InverterFamily {
    Sma,
    Fronius,
    SolarEdge,
    Huawei,
    Goodwe,
    Kostal,
    /// Any manufacturer not in the recognised set.
    Generic,
}

impl InverterFamily {
    /// Map a manufacturer string to a family. Case-insensitive substring
    /// match because manufacturer strings vary ("SMA Solar Technology AG",
    /// "Fronius International GmbH", …).
    #[must_use]
    pub fn from_manufacturer(mfr: &str) -> Self {
        let m = mfr.to_ascii_lowercase();
        if m.contains("sma") {
            Self::Sma
        } else if m.contains("fronius") {
            Self::Fronius
        } else if m.contains("solaredge") {
            Self::SolarEdge
        } else if m.contains("huawei") {
            Self::Huawei
        } else if m.contains("goodwe") {
            Self::Goodwe
        } else if m.contains("kostal") {
            Self::Kostal
        } else {
            Self::Generic
        }
    }
}

/// Decoded Model 1 (Common) block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonModel {
    pub manufacturer: String,
    pub model: String,
    pub options: String,
    pub version: String,
    pub serial_number: String,
    /// Modbus device address (`0` if the device omits the field).
    pub device_address: u16,
    pub family: InverterFamily,
}

impl CommonModel {
    /// SunSpec model id for the common block.
    pub const MODEL_ID: u16 = 1;
    /// Minimum payload length we require (through the version field at 48).
    pub const MIN_LENGTH: usize = 64;

    /// Decode from a model payload slice (the registers after the 2-word
    /// model header, i.e. [`crate::discovery::DiscoveredModel::payload`]).
    ///
    /// # Errors
    /// [`DecodeError::OutOfBounds`] if the payload is too short to hold the
    /// string fields, and [`DecodeError::InvalidString`] on non-UTF-8 text.
    pub fn decode(payload: &[u16]) -> Result<Self, DecodeError> {
        if payload.len() < Self::MIN_LENGTH {
            return Err(DecodeError::OutOfBounds { offset: Self::MIN_LENGTH, len: payload.len() });
        }
        let manufacturer = point::string(payload, 0, 16)?;
        let model = point::string(payload, 16, 16)?;
        let options = point::string(payload, 32, 8)?;
        let version = point::string(payload, 40, 8)?;
        let serial_number = point::string(payload, 48, 16)?;
        // Device address is optional in shorter implementations.
        let device_address = point::uint16(payload, 64).ok().flatten().unwrap_or(0);
        let family = InverterFamily::from_manufacturer(&manufacturer);
        Ok(Self {
            manufacturer,
            model,
            options,
            version,
            serial_number,
            device_address,
            family,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pack an ASCII string into `len_regs` registers, MSB-first, NUL-padded.
    fn ascii_to_regs(s: &str, len_regs: usize) -> Vec<u16> {
        let mut bytes: Vec<u8> = s.bytes().collect();
        bytes.resize(len_regs * 2, 0);
        bytes.chunks(2).map(|c| (u16::from(c[0]) << 8) | u16::from(c[1])).collect()
    }

    fn build_common(mfr: &str, model: &str, ver: &str, sn: &str, addr: u16) -> Vec<u16> {
        let mut regs = Vec::new();
        regs.extend(ascii_to_regs(mfr, 16));
        regs.extend(ascii_to_regs(model, 16));
        regs.extend(ascii_to_regs("", 8)); // options
        regs.extend(ascii_to_regs(ver, 8));
        regs.extend(ascii_to_regs(sn, 16));
        regs.push(addr);
        regs
    }

    #[test]
    fn decode_full_common_block() {
        let regs = build_common("Fronius", "Symo 8.2-3-M", "3.18.7-1", "12345678", 126);
        let m = CommonModel::decode(&regs).unwrap();
        assert_eq!(m.manufacturer, "Fronius");
        assert_eq!(m.model, "Symo 8.2-3-M");
        assert_eq!(m.version, "3.18.7-1");
        assert_eq!(m.serial_number, "12345678");
        assert_eq!(m.device_address, 126);
        assert_eq!(m.family, InverterFamily::Fronius);
    }

    #[test]
    fn short_payload_is_error() {
        let regs = [0u16; 10];
        assert!(matches!(
            CommonModel::decode(&regs),
            Err(DecodeError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn family_matrix() {
        for (mfr, expected) in [
            ("SMA Solar Technology AG", InverterFamily::Sma),
            ("Fronius International GmbH", InverterFamily::Fronius),
            ("SolarEdge", InverterFamily::SolarEdge),
            ("Huawei Technologies", InverterFamily::Huawei),
            ("GoodWe", InverterFamily::Goodwe),
            ("KOSTAL Solar Electric GmbH", InverterFamily::Kostal),
            ("Acme Inverter Co.", InverterFamily::Generic),
        ] {
            assert_eq!(InverterFamily::from_manufacturer(mfr), expected, "mfr={mfr}");
        }
    }

    #[test]
    fn device_address_optional_when_field_absent() {
        let mut regs = build_common("SMA", "Sunny Boy", "1.0", "SN1", 0);
        regs.pop(); // drop the device-address word
        let m = CommonModel::decode(&regs).unwrap();
        assert_eq!(m.device_address, 0);
        assert_eq!(m.family, InverterFamily::Sma);
    }
}
