// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Group telegrams — the unit of KNX group communication.
//!
//! A [`GroupTelegram`] is "device *X* tells group *Y* to do *Z*": a source
//! [`IndividualAddress`], a destination [`GroupAddress`], an application
//! [`GroupService`] (read / write / response), and a payload.
//!
//! This module models only the **application protocol data unit** (APDU) — the
//! service code plus payload, built from the public KNX framing rules. The
//! enclosing KNXnet/IP or cEMI link frame (with its UDP transport) is Phase-1b
//! and deferred (see `parity.manifest.toml`); here the addresses are carried
//! alongside the APDU in a plain struct so the codec is fully testable without
//! any network.
//!
//! ## APDU encoding
//!
//! ```text
//!   byte 0: TPCI(6) | APCI-high(2)         — TPCI = 0 for group data
//!   byte 1: APCI-low(2) | payload-low(6)
//!   byte 2.. : extra payload bytes (only when the payload is > 6 bits)
//! ```
//!
//! Small payloads (≤ 6 bits — booleans, dimming) live entirely in byte 1's low
//! 6 bits, marked by a leading length nibble of 0 (the "optimized" form). Large
//! payloads clear those 6 bits and append their bytes.

use crate::address::{GroupAddress, IndividualAddress};
use crate::apci::GroupService;
use crate::error::{KnxError, Result};

/// The data a group telegram carries: either a ≤6-bit small value packed into
/// the control byte, or a multi-byte payload appended after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// No payload — used by [`GroupService::Read`].
    None,
    /// A ≤6-bit value (`0..=63`) carried inside the control byte.
    Small(u8),
    /// A multi-byte payload appended after the two control bytes.
    Bytes(Vec<u8>),
}

impl Payload {
    /// Build a small payload, validating the 6-bit range.
    ///
    /// # Errors
    /// Returns a conversion error if `value > 63`.
    pub fn small(value: u8) -> Result<Self> {
        if value > 0x3F {
            return Err(KnxError::Conversion(format!(
                "small payload must be ≤ 6 bits (0..=63), got {value}"
            )));
        }
        Ok(Self::Small(value))
    }
}

/// A KNX group telegram: who, where, what service, and the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTelegram {
    /// The device that originated the telegram.
    pub source: IndividualAddress,
    /// The logical group it targets.
    pub destination: GroupAddress,
    /// The application service.
    pub service: GroupService,
    /// The value the service carries.
    pub payload: Payload,
}

impl GroupTelegram {
    /// Build a "set this group's value" write telegram from a multi-byte
    /// payload (e.g. an encoded DPT 9 temperature).
    #[must_use]
    pub fn write(
        source: IndividualAddress,
        destination: GroupAddress,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            source,
            destination,
            service: GroupService::Write,
            payload: Payload::Bytes(bytes),
        }
    }

    /// Build a "set this group's value" write telegram from a small ≤6-bit
    /// value (e.g. a DPT 1 switch).
    ///
    /// # Errors
    /// Returns a conversion error if `value > 63`.
    pub fn write_small(
        source: IndividualAddress,
        destination: GroupAddress,
        value: u8,
    ) -> Result<Self> {
        Ok(Self {
            source,
            destination,
            service: GroupService::Write,
            payload: Payload::small(value)?,
        })
    }

    /// Build a "what is this group's value?" read telegram.
    #[must_use]
    pub fn read(source: IndividualAddress, destination: GroupAddress) -> Self {
        Self {
            source,
            destination,
            service: GroupService::Read,
            payload: Payload::None,
        }
    }

    /// Encode the application protocol data unit (APDU): the two control bytes
    /// plus any appended payload bytes.
    ///
    /// # Errors
    /// Returns a conversion error if a [`Payload::Small`] is out of the 6-bit
    /// range (which [`Payload::small`] already guards, but is re-checked here so
    /// hand-built payloads cannot smuggle an invalid value onto the wire).
    pub fn encode_apdu(&self) -> Result<Vec<u8>> {
        let apci = self.service.apci_code();
        // byte 0: TPCI = 0, APCI high 2 bits in bits 1..0.
        let byte0 = ((apci >> 2) & 0x03) as u8;
        // byte 1: APCI low 2 bits in bits 7..6, payload (if small) in bits 5..0.
        let apci_low = ((apci & 0x03) << 6) as u8;

        match &self.payload {
            Payload::None => Ok(vec![byte0, apci_low]),
            Payload::Small(v) => {
                if *v > 0x3F {
                    return Err(KnxError::Conversion(format!(
                        "small payload must be ≤ 6 bits, got {v}"
                    )));
                }
                Ok(vec![byte0, apci_low | (v & 0x3F)])
            }
            Payload::Bytes(bytes) => {
                let mut out = Vec::with_capacity(2 + bytes.len());
                out.push(byte0);
                out.push(apci_low);
                out.extend_from_slice(bytes);
                Ok(out)
            }
        }
    }

    /// Decode an APDU (the two control bytes plus payload) back into a service
    /// and payload, attaching the supplied source/destination addresses.
    ///
    /// The optimized small-payload form is recognised when the APDU is exactly
    /// two bytes; longer APDUs are decoded as an appended [`Payload::Bytes`].
    ///
    /// # Errors
    /// Returns a [`KnxError::Telegram`] if the APDU is shorter than the two
    /// control bytes or carries an application service this codec does not
    /// model.
    pub fn decode_apdu(
        source: IndividualAddress,
        destination: GroupAddress,
        apdu: &[u8],
    ) -> Result<Self> {
        if apdu.len() < 2 {
            return Err(KnxError::Telegram(format!(
                "APDU must be at least 2 bytes, got {}",
                apdu.len()
            )));
        }
        let apci = (u16::from(apdu[0] & 0x03) << 2) | u16::from(apdu[1] >> 6);
        let service = GroupService::from_apci_code(apci)?;

        let payload = match (service, apdu.len()) {
            (GroupService::Read, 2) => Payload::None,
            // Exactly two bytes => the value (if any) is the optimized 6-bit form.
            (_, 2) => {
                let small = apdu[1] & 0x3F;
                if small == 0 && service == GroupService::Read {
                    Payload::None
                } else {
                    Payload::Small(small)
                }
            }
            // More than two bytes => an appended multi-byte payload.
            (_, _) => Payload::Bytes(apdu[2..].to_vec()),
        };

        Ok(Self {
            source,
            destination,
            service,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dpt;

    fn src() -> IndividualAddress {
        IndividualAddress::parse("1.1.5").expect("source")
    }

    fn dst() -> GroupAddress {
        GroupAddress::parse("1/2/3").expect("destination")
    }

    #[test]
    fn read_telegram_has_two_byte_apdu() {
        let t = GroupTelegram::read(src(), dst());
        let apdu = t.encode_apdu().unwrap();
        assert_eq!(apdu.len(), 2);
        // Read APCI is all zero across both control bytes.
        assert_eq!(apdu, vec![0x00, 0x00]);
        let back = GroupTelegram::decode_apdu(src(), dst(), &apdu).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn write_small_switch_on_roundtrips() {
        // DPT 1 "on" packed as a 6-bit small payload.
        let value = dpt::dpt1::encode(true); // 1
        let t = GroupTelegram::write_small(src(), dst(), value).unwrap();
        let apdu = t.encode_apdu().unwrap();
        assert_eq!(apdu.len(), 2, "small payload stays in the control byte");
        // Write APCI = 0b0010 -> byte0 low2=0b00, byte1 high2=0b10 -> 0x80 | value.
        assert_eq!(apdu, vec![0x00, 0x80 | 1]);
        let back = GroupTelegram::decode_apdu(src(), dst(), &apdu).unwrap();
        assert_eq!(back.service, GroupService::Write);
        assert_eq!(back.payload, Payload::Small(1));
    }

    #[test]
    fn write_bytes_temperature_roundtrips() {
        // DPT 9 temperature is a 2-byte appended payload.
        let bytes = dpt::dpt9::encode(21.0).unwrap().to_vec();
        let t = GroupTelegram::write(src(), dst(), bytes.clone());
        let apdu = t.encode_apdu().unwrap();
        assert_eq!(apdu.len(), 4, "two control bytes + two payload bytes");
        assert_eq!(&apdu[2..], &bytes[..]);
        let back = GroupTelegram::decode_apdu(src(), dst(), &apdu).unwrap();
        assert_eq!(back.payload, Payload::Bytes(bytes));
        assert_eq!(back.service, GroupService::Write);
    }

    #[test]
    fn response_roundtrips() {
        let t = GroupTelegram {
            source: src(),
            destination: dst(),
            service: GroupService::Response,
            payload: Payload::Small(1),
        };
        let apdu = t.encode_apdu().unwrap();
        let back = GroupTelegram::decode_apdu(src(), dst(), &apdu).unwrap();
        assert_eq!(back.service, GroupService::Response);
        assert_eq!(back.payload, Payload::Small(1));
    }

    #[test]
    fn small_payload_range_is_enforced() {
        assert!(Payload::small(64).is_err());
        assert!(Payload::small(63).is_ok());
        assert!(GroupTelegram::write_small(src(), dst(), 100).is_err());
    }

    #[test]
    fn short_or_unknown_apdu_rejected() {
        assert!(GroupTelegram::decode_apdu(src(), dst(), &[0x00]).is_err());
        // APCI 0b1111 (memory write region) is not modelled.
        assert!(GroupTelegram::decode_apdu(src(), dst(), &[0x03, 0xC0]).is_err());
    }
}
