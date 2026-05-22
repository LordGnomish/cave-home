// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec Model 1 — Common.
//!
//! Source: SunSpec Modbus Specification v1.7, model 1 definition.
//!
//! Layout (offsets from model header):
//! ```text
//!     0    Mn (manufacturer)         16 registers (32-byte ASCII)
//!    16    Md (model)                16 registers
//!    32    Opt (options)              8 registers
//!    40    Vr (version)               8 registers
//!    48    SN (serial number)        16 registers
//!    64    DA (Modbus device address) 1 register
//! ```

use crate::error::Result;
use crate::raw;
use serde::{Deserialize, Serialize};

/// Inverter family inferred from the Model 1 manufacturer string.
/// cave-home uses this to pick vendor-specific quirks (e.g. SolarEdge
/// reports power in W; SMA in W scaled by SF; Huawei sign convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InverterFamily {
    Sma,
    Fronius,
    SolarEdge,
    Huawei,
    Goodwe,
    Kostal,
    Generic,
}

impl InverterFamily {
    /// Map a manufacturer string from Model 1 to a family. Case-insensitive,
    /// matches the prefix because manufacturer strings vary
    /// ("SMA Solar Technology AG", "Fronius International GmbH", …).
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommonModel {
    pub manufacturer: String,
    pub model: String,
    pub options: String,
    pub version: String,
    pub serial_number: String,
    pub modbus_device_address: u16,
    pub family: InverterFamily,
}

impl CommonModel {
    pub const MODEL_ID: u16 = 1;
    pub const MODEL_LENGTH: u16 = 65;

    /// Parse from a register block that starts at the **payload**
    /// (immediately after the model header).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ShortRead`] if `regs` is shorter than
    /// `MODEL_LENGTH`. Returns [`crate::Error::InvalidString`] on
    /// non-UTF-8 ASCII fields.
    pub fn parse(regs: &[u16]) -> Result<Self> {
        if regs.len() < Self::MODEL_LENGTH as usize {
            return Err(crate::Error::ShortRead {
                expected: Self::MODEL_LENGTH,
                actual: regs.len() as u16,
            });
        }
        let manufacturer = raw::read_string(regs, 0, 16, "Mn")?;
        let model = raw::read_string(regs, 16, 16, "Md")?;
        let options = raw::read_string(regs, 32, 8, "Opt")?;
        let version = raw::read_string(regs, 40, 8, "Vr")?;
        let serial_number = raw::read_string(regs, 48, 16, "SN")?;
        let modbus_device_address = raw::read_u16(regs, 64).unwrap_or(0);
        let family = InverterFamily::from_manufacturer(&manufacturer);
        Ok(Self {
            manufacturer,
            model,
            options,
            version,
            serial_number,
            modbus_device_address,
            family,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad_to(buf: &mut Vec<u16>, target: usize) {
        while buf.len() < target {
            buf.push(0);
        }
    }

    fn ascii_to_regs(s: &str, register_words: usize) -> Vec<u16> {
        let mut bytes: Vec<u8> = s.bytes().collect();
        while bytes.len() < register_words * 2 {
            bytes.push(0);
        }
        bytes.chunks(2).map(|c| (u16::from(c[0]) << 8) | u16::from(c[1])).collect()
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
    fn parse_short_buffer_errs() {
        let regs = vec![0u16; 10];
        assert!(matches!(
            CommonModel::parse(&regs),
            Err(crate::Error::ShortRead { .. })
        ));
    }

    #[test]
    fn parse_full_block_roundtrip() {
        let mut regs = Vec::new();
        regs.extend(ascii_to_regs("Fronius", 16));
        regs.extend(ascii_to_regs("Symo 8.2-3-M", 16));
        regs.extend(ascii_to_regs("", 8));
        regs.extend(ascii_to_regs("3.18.7-1", 8));
        regs.extend(ascii_to_regs("12345678", 16));
        regs.push(126); // Modbus device address
        pad_to(&mut regs, CommonModel::MODEL_LENGTH as usize);

        let m = CommonModel::parse(&regs).unwrap();
        assert_eq!(m.manufacturer, "Fronius");
        assert_eq!(m.model, "Symo 8.2-3-M");
        assert_eq!(m.version, "3.18.7-1");
        assert_eq!(m.serial_number, "12345678");
        assert_eq!(m.modbus_device_address, 126);
        assert_eq!(m.family, InverterFamily::Fronius);
    }
}
