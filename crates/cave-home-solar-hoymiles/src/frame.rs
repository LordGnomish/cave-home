//! Radio frame model: inverter addressing and request opcodes.
//!
//! Clean-room (Charter §6.1 / ADR-002): the framing below is built from the
//! **public description of the Hoymiles NRF24 / CMT radio protocol** — the
//! way the inverter serial number is used as a radio address and the command
//! byte that selects which telemetry the inverter returns. No GPL `AhoyDTU` /
//! `OpenDTU` source was read.
//!
//! A Hoymiles inverter is addressed by its printed serial number. The radio
//! uses the **last four bytes** of that serial as the device address, so this
//! module models the serial and derives that address. A request frame names a
//! command opcode (real-time data, device info, alarm data); the inverter
//! answers with one or more fragments (see [`crate::reassembly`]).

use core::fmt;

/// A Hoymiles inverter serial number.
///
/// The serial is printed as a 12-digit decimal string on the inverter label.
/// Internally it is a 6-byte BCD-style identifier; the radio addresses the
/// inverter by the **last four bytes**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InverterSerial {
    bytes: [u8; 6],
}

/// Why an [`InverterSerial`] could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    /// The printed serial was not exactly 12 decimal digits.
    WrongLength,
    /// The printed serial contained a non-digit character.
    NotDecimal,
}

impl fmt::Display for SerialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength => f.write_str("inverter serial must be 12 digits"),
            Self::NotDecimal => f.write_str("inverter serial must be all digits"),
        }
    }
}

impl std::error::Error for SerialError {}

impl InverterSerial {
    /// Build a serial from its six raw bytes (most-significant first).
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 6]) -> Self {
        Self { bytes }
    }

    /// Parse the 12-digit decimal serial printed on the inverter label.
    ///
    /// Each pair of decimal digits becomes one byte (packed BCD), matching how
    /// the serial appears on the wire.
    ///
    /// # Errors
    /// Returns [`SerialError`] if `printed` is not exactly 12 decimal digits.
    pub fn parse_printed(printed: &str) -> Result<Self, SerialError> {
        if printed.len() != 12 {
            return Err(SerialError::WrongLength);
        }
        let digits = printed.as_bytes();
        let mut bytes = [0u8; 6];
        let mut i = 0;
        while i < 6 {
            let hi = digits[i * 2];
            let lo = digits[i * 2 + 1];
            if !hi.is_ascii_digit() || !lo.is_ascii_digit() {
                return Err(SerialError::NotDecimal);
            }
            bytes[i] = (hi - b'0') * 10 + (lo - b'0');
            i += 1;
        }
        Ok(Self { bytes })
    }

    /// The full six-byte identifier.
    #[must_use]
    pub const fn bytes(&self) -> [u8; 6] {
        self.bytes
    }

    /// The four-byte radio address — the last four bytes of the serial, the
    /// portion the NRF24 / CMT radio uses to address this inverter.
    #[must_use]
    pub const fn radio_address(&self) -> [u8; 4] {
        [self.bytes[2], self.bytes[3], self.bytes[4], self.bytes[5]]
    }
}

/// A telemetry request opcode — which body of data the inverter should return.
///
/// Values match the documented Hoymiles command bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Live per-panel and grid measurements.
    RealTimeData,
    /// Static device info (firmware / hardware identifiers).
    DeviceInfo,
    /// Stored fault / alarm log.
    AlarmData,
}

impl Command {
    /// The on-wire command byte for this request.
    #[must_use]
    pub const fn opcode(self) -> u8 {
        match self {
            Self::RealTimeData => 0x0B,
            Self::DeviceInfo => 0x0F,
            Self::AlarmData => 0x11,
        }
    }

    /// Recover a [`Command`] from its on-wire opcode byte.
    #[must_use]
    pub const fn from_opcode(byte: u8) -> Option<Self> {
        match byte {
            0x0B => Some(Self::RealTimeData),
            0x0F => Some(Self::DeviceInfo),
            0x11 => Some(Self::AlarmData),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_printed_serial_as_packed_bcd() {
        let s = InverterSerial::parse_printed("114174123456").expect("valid serial");
        assert_eq!(s.bytes(), [11, 41, 74, 12, 34, 56]);
    }

    #[test]
    fn radio_address_is_last_four_bytes() {
        let s = InverterSerial::from_bytes([0x11, 0x41, 0x74, 0x12, 0x34, 0x56]);
        assert_eq!(s.radio_address(), [0x74, 0x12, 0x34, 0x56]);
    }

    #[test]
    fn rejects_wrong_length_serial() {
        assert_eq!(
            InverterSerial::parse_printed("12345"),
            Err(SerialError::WrongLength)
        );
    }

    #[test]
    fn rejects_non_decimal_serial() {
        assert_eq!(
            InverterSerial::parse_printed("11417412AB56"),
            Err(SerialError::NotDecimal)
        );
    }

    #[test]
    fn command_opcode_round_trips() {
        for cmd in [Command::RealTimeData, Command::DeviceInfo, Command::AlarmData] {
            assert_eq!(Command::from_opcode(cmd.opcode()), Some(cmd));
        }
    }

    #[test]
    fn unknown_opcode_is_none() {
        assert_eq!(Command::from_opcode(0x00), None);
        assert_eq!(Command::from_opcode(0xFF), None);
    }
}
