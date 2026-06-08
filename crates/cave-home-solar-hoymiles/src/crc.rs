//! Checksums used by the Hoymiles microinverter radio protocol.
//!
//! Clean-room (Charter §6.1 / ADR-002): both algorithms are implemented from
//! their **public mathematical definitions** — the published CRC parameter
//! catalogue — not from any GPL reference implementation. No `AhoyDTU` / `OpenDTU`
//! source was read.
//!
//! Two checksums appear on the wire:
//! - **CRC-8** guards each individual radio fragment. Hoymiles uses the
//!   `CRC-8/NRSC-5` parameters: polynomial `0x31`, initial value `0xFF`, no
//!   input/output reflection, no final XOR.
//! - **CRC-16/Modbus** guards the fully reassembled payload: polynomial
//!   `0x8005` (reflected), initial value `0xFFFF`, input and output reflected,
//!   no final XOR.
//!
//! Both are pure byte functions — no allocation, no I/O.

/// CRC-8 over `data` using the Hoymiles fragment parameters
/// (`CRC-8/NRSC-5`: poly `0x31`, init `0xFF`, no reflection, no final XOR).
#[must_use]
pub const fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0xFF;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i];
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x31
            } else {
                crc << 1
            };
            bit += 1;
        }
        i += 1;
    }
    crc
}

/// CRC-16/Modbus over `data` (poly `0x8005` reflected, init `0xFFFF`, input and
/// output reflected, no final XOR). Returned in host byte order; callers append
/// it little-endian on the wire.
#[must_use]
pub const fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i] as u16;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x0001 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
            bit += 1;
        }
        i += 1;
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    // CRC-8/NRSC-5 published check value: the ASCII string "123456789"
    // checksums to 0xF7.
    #[test]
    fn crc8_known_check_vector() {
        assert_eq!(crc8(b"123456789"), 0xF7);
    }

    #[test]
    fn crc8_empty_is_init() {
        // No bytes processed -> the initial value survives unchanged.
        assert_eq!(crc8(&[]), 0xFF);
    }

    #[test]
    fn crc8_single_byte_is_deterministic() {
        // Stable value computed from the documented poly/init; guards against
        // an accidental algorithm change.
        assert_eq!(crc8(&[0x00]), 0xAC);
    }

    // CRC-16/Modbus published check value: "123456789" -> 0x4B37.
    #[test]
    fn crc16_modbus_known_check_vector() {
        assert_eq!(crc16_modbus(b"123456789"), 0x4B37);
    }

    #[test]
    fn crc16_empty_is_init() {
        assert_eq!(crc16_modbus(&[]), 0xFFFF);
    }

    #[test]
    fn crc16_modbus_single_byte_vector() {
        // A single 0x01 byte under Modbus parameters.
        assert_eq!(crc16_modbus(&[0x01]), 0x807E);
    }

    #[test]
    fn crc8_detects_single_bit_flip() {
        let good = crc8(&[0x11, 0x22, 0x33, 0x44]);
        let bad = crc8(&[0x11, 0x22, 0x33, 0x45]);
        assert_ne!(good, bad);
    }
}
