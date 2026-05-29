// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Application service codes (APCI) for KNX group communication.
//!
//! On the bus, the application layer of a group telegram carries a 10-bit
//! Transport/Application control field spread across two bytes, followed by the
//! payload. For *group value* communication the three services we model are:
//!
//! | Service        | 4-bit APCI | meaning                                 |
//! |----------------|-----------|------------------------------------------|
//! | `Read`         | `0b0000`  | "what is the value?" (no payload)        |
//! | `Response`     | `0b0001`  | "the value is …" (answer to a Read)      |
//! | `Write`        | `0b0010`  | "set the value to …"                     |
//!
//! The two control bytes look like:
//!
//! ```text
//!   byte 0: bits 7..2 = TPCI (0 for group data), bits 1..0 = APCI high 2 bits
//!   byte 1: bits 7..6 = APCI low 2 bits, bits 5..0 = small payload (DPT ≤ 6 bit)
//! ```
//!
//! The **small-payload optimization**: when a datapoint is ≤ 6 bits (DPT 1.x
//! booleans, DPT 2.x, DPT 3.x dimming), the value rides in the low 6 bits of
//! byte 1 and *no* extra payload bytes follow. Larger datapoints clear those 6
//! bits and append their bytes after the two control bytes.

use crate::error::{KnxError, Result};

/// A group-value application service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupService {
    /// Ask a group for its current value. Carries no payload.
    Read,
    /// Answer to a [`GroupService::Read`], carrying the value.
    Response,
    /// Command a group to take a new value.
    Write,
}

impl GroupService {
    /// The 4-bit APCI code for this service.
    #[must_use]
    pub const fn apci_code(self) -> u16 {
        match self {
            Self::Read => 0b0000,
            Self::Response => 0b0001,
            Self::Write => 0b0010,
        }
    }

    /// Recover the service from a 4-bit APCI code.
    ///
    /// # Errors
    /// Returns a [`KnxError::Telegram`] for any code this Phase-1 codec does not
    /// model (memory/property/device services are out of scope).
    pub fn from_apci_code(code: u16) -> Result<Self> {
        match code {
            0b0000 => Ok(Self::Read),
            0b0001 => Ok(Self::Response),
            0b0010 => Ok(Self::Write),
            other => Err(KnxError::Telegram(format!(
                "unsupported application service code {other:#06b}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_codes_round_trip() {
        for s in [GroupService::Read, GroupService::Response, GroupService::Write] {
            assert_eq!(GroupService::from_apci_code(s.apci_code()).unwrap(), s);
        }
    }

    #[test]
    fn unknown_service_rejected() {
        assert!(GroupService::from_apci_code(0b1010).is_err());
    }
}
