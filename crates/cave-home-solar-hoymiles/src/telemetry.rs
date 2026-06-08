//! Real-time telemetry decode from an assembled payload.
//!
//! Clean-room (Charter §6.1 / ADR-002): the field layout and fixed-point
//! divisors are taken from the **public description of the Hoymiles real-time
//! data record** — big-endian 16-bit registers, each scaled by a documented
//! divisor (e.g. voltages /10, currents /100, energy in raw watt-hours). No
//! GPL `AhoyDTU` / `OpenDTU` source was read.
//!
//! # Payload layout (real-time data)
//!
//! The assembled payload is a sequence of big-endian `u16` registers followed
//! by a two-byte little-endian CRC-16/Modbus trailer over everything before
//! it. The register order, for an inverter with `N` DC channels, is:
//!
//! | Offset (registers) | Field                | Divisor | Unit  |
//! |--------------------|----------------------|---------|-------|
//! | per DC channel `k` |                      |         |       |
//! |   `4k + 0`         | PV voltage           | /10     | V     |
//! |   `4k + 1`         | PV current           | /100    | A     |
//! |   `4k + 2`         | PV power             | /10     | W     |
//! |   `4k + 3`         | reserved             | —       | —     |
//! | after `N` channels |                      |         |       |
//! |   `4N + 0`         | grid voltage         | /10     | V     |
//! |   `4N + 1`         | grid frequency       | /100    | Hz    |
//! |   `4N + 2`         | AC power             | /10     | W     |
//! |   `4N + 3`         | today's yield        | /1      | Wh    |
//! |   `4N + 4`,`+5`    | total yield (u32)    | /1      | Wh    |
//! |   `4N + 6`         | inverter temperature | /10     | °C    |
//!
//! Each register is two bytes, so the payload is
//! `(4*N + 7) * 2 + 2` (CRC) bytes long.

use crate::crc::crc16_modbus;
use crate::family::Family;

/// Live readings for a single solar panel (one DC channel).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelReading {
    /// Panel voltage, volts.
    pub voltage_v: f64,
    /// Panel current, amperes.
    pub current_a: f64,
    /// Panel power, watts.
    pub power_w: f64,
}

/// Decoded real-time telemetry for one inverter.
#[derive(Debug, Clone, PartialEq)]
pub struct Telemetry {
    /// Per-panel DC readings, in channel order.
    pub panels: Vec<PanelReading>,
    /// Grid (AC) voltage, volts.
    pub grid_voltage_v: f64,
    /// Grid frequency, hertz.
    pub grid_frequency_hz: f64,
    /// AC power being exported, watts.
    pub ac_power_w: f64,
    /// Energy produced so far today, watt-hours.
    pub today_wh: f64,
    /// Lifetime energy produced, watt-hours.
    pub total_wh: f64,
    /// Inverter internal temperature, degrees Celsius.
    pub temperature_c: f64,
}

impl Telemetry {
    /// Combined DC power across every panel, watts.
    #[must_use]
    pub fn dc_power_w(&self) -> f64 {
        self.panels.iter().map(|p| p.power_w).sum()
    }

    /// Conversion efficiency (AC out / DC in), as a fraction `0.0..=1.0`.
    ///
    /// Returns `0.0` when no DC power is being produced (night / no sun), so
    /// the caller never divides by zero.
    #[must_use]
    pub fn efficiency(&self) -> f64 {
        let dc = self.dc_power_w();
        if dc <= 0.0 {
            return 0.0;
        }
        (self.ac_power_w / dc).clamp(0.0, 1.0)
    }
}

/// Why a payload could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The payload was shorter than the layout for this family requires.
    Truncated { expected: usize, got: usize },
    /// The CRC-16/Modbus trailer did not match the payload body.
    BadChecksum,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated { expected, got } => {
                write!(f, "payload too short: need {expected} bytes, got {got}")
            }
            Self::BadChecksum => f.write_str("payload checksum did not match"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Number of payload bytes (including the CRC-16 trailer) a real-time record
/// occupies for `family`.
#[must_use]
pub const fn payload_len(family: Family) -> usize {
    let n = family.panel_count();
    // (4 registers per panel + 7 AC/summary registers) * 2 bytes + 2 CRC.
    (4 * n + 7) * 2 + 2
}

// Read a big-endian u16 register at byte offset `at`.
fn reg(payload: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([payload[at], payload[at + 1]])
}

/// Decode a real-time telemetry payload for `family`.
///
/// # Errors
/// Returns [`DecodeError::Truncated`] if the payload is shorter than the
/// family's layout, or [`DecodeError::BadChecksum`] if the CRC-16/Modbus
/// trailer does not match the body. Never panics.
pub fn decode(payload: &[u8], family: Family) -> Result<Telemetry, DecodeError> {
    let expected = payload_len(family);
    if payload.len() < expected {
        return Err(DecodeError::Truncated { expected, got: payload.len() });
    }
    // The body is everything before the 2-byte CRC trailer.
    let body = &payload[..expected - 2];
    let want = u16::from_le_bytes([payload[expected - 2], payload[expected - 1]]);
    if crc16_modbus(body) != want {
        return Err(DecodeError::BadChecksum);
    }

    let n = family.panel_count();
    let mut panels = Vec::with_capacity(n);
    for k in 0..n {
        let base = (4 * k) * 2;
        panels.push(PanelReading {
            voltage_v: f64::from(reg(body, base)) / 10.0,
            current_a: f64::from(reg(body, base + 2)) / 100.0,
            power_w: f64::from(reg(body, base + 4)) / 10.0,
        });
    }

    // AC / summary registers begin right after the N panel channels.
    let ac = (4 * n) * 2;
    let total_hi = u32::from(reg(body, ac + 8));
    let total_lo = u32::from(reg(body, ac + 10));
    Ok(Telemetry {
        panels,
        grid_voltage_v: f64::from(reg(body, ac)) / 10.0,
        grid_frequency_hz: f64::from(reg(body, ac + 2)) / 100.0,
        ac_power_w: f64::from(reg(body, ac + 4)) / 10.0,
        today_wh: f64::from(reg(body, ac + 6)),
        total_wh: f64::from((total_hi << 16) | total_lo),
        temperature_c: f64::from(reg(body, ac + 12)) / 10.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a real-time payload for `family` from register values, appending
    // the correct CRC-16/Modbus trailer. `regs` must be the full register run
    // (4*N panel + 7 AC/summary) as big-endian u16s.
    fn build_payload(regs: &[u16]) -> Vec<u8> {
        let mut body = Vec::new();
        for r in regs {
            body.extend_from_slice(&r.to_be_bytes());
        }
        let crc = crc16_modbus(&body);
        body.extend_from_slice(&crc.to_le_bytes());
        body
    }

    // One-panel register run: [V, I, P, reserved, gridV, freq, acP, today,
    // totalHi, totalLo, temp].
    fn one_panel_regs() -> Vec<u16> {
        vec![
            340, // PV 34.0 V
            850, // PV 8.50 A
            2890, // PV 289.0 W
            0, // reserved
            2301, // grid 230.1 V
            5000, // 50.00 Hz
            2800, // AC 280.0 W
            123, // today 123 Wh
            0x0001, // total hi
            0x86A0, // total lo -> 0x000186A0 = 100000 Wh
            452, // temp 45.2 °C
        ]
    }

    #[test]
    fn decodes_one_panel_fields_and_scaling() {
        let payload = build_payload(&one_panel_regs());
        let t = decode(&payload, Family::OnePanel).expect("decodes");
        assert_eq!(t.panels.len(), 1);
        assert!((t.panels[0].voltage_v - 34.0).abs() < 1e-9);
        assert!((t.panels[0].current_a - 8.50).abs() < 1e-9);
        assert!((t.panels[0].power_w - 289.0).abs() < 1e-9);
        assert!((t.grid_voltage_v - 230.1).abs() < 1e-9);
        assert!((t.grid_frequency_hz - 50.0).abs() < 1e-9);
        assert!((t.ac_power_w - 280.0).abs() < 1e-9);
        assert!((t.today_wh - 123.0).abs() < 1e-9);
        assert!((t.total_wh - 100_000.0).abs() < 1e-9);
        assert!((t.temperature_c - 45.2).abs() < 1e-9);
    }

    #[test]
    fn efficiency_is_ac_over_dc() {
        let payload = build_payload(&one_panel_regs());
        let t = decode(&payload, Family::OnePanel).unwrap();
        // 280 AC / 289 DC.
        assert!((t.efficiency() - (280.0 / 289.0)).abs() < 1e-9);
    }

    #[test]
    fn efficiency_is_zero_at_night() {
        let mut regs = one_panel_regs();
        regs[2] = 0; // no PV power
        regs[6] = 0; // no AC power
        let payload = build_payload(&regs);
        let t = decode(&payload, Family::OnePanel).unwrap();
        assert_eq!(t.efficiency(), 0.0);
    }

    #[test]
    fn two_panel_has_two_panel_readings() {
        // 2 panels * 4 + 7 = 15 registers.
        let regs = vec![
            340, 850, 2890, 0, // panel 1
            300, 200, 600, 0, // panel 2
            2300, 5000, 3400, 50, 0, 5000, 410, // AC + summary
        ];
        let payload = build_payload(&regs);
        let t = decode(&payload, Family::TwoPanel).unwrap();
        assert_eq!(t.panels.len(), 2);
        assert!((t.panels[1].power_w - 60.0).abs() < 1e-9);
        assert!((t.dc_power_w() - (289.0 + 60.0)).abs() < 1e-9);
    }

    #[test]
    fn four_panel_has_four_panel_readings() {
        // 4 panels * 4 + 7 = 23 registers.
        let mut regs = vec![0u16; 23];
        for k in 0..4 {
            regs[4 * k + 2] = 1000; // 100.0 W each panel
        }
        let payload = build_payload(&regs);
        let t = decode(&payload, Family::FourPanel).unwrap();
        assert_eq!(t.panels.len(), 4);
        assert!((t.dc_power_w() - 400.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_truncated_payload() {
        let payload = build_payload(&one_panel_regs());
        let short = &payload[..payload.len() - 5];
        match decode(short, Family::OnePanel) {
            Err(DecodeError::Truncated { .. }) => {}
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut payload = build_payload(&one_panel_regs());
        payload[0] ^= 0xFF; // corrupt the body, CRC trailer no longer matches
        assert_eq!(decode(&payload, Family::OnePanel), Err(DecodeError::BadChecksum));
    }

    #[test]
    fn payload_len_matches_family() {
        assert_eq!(payload_len(Family::OnePanel), (4 + 7) * 2 + 2);
        assert_eq!(payload_len(Family::TwoPanel), (8 + 7) * 2 + 2);
        assert_eq!(payload_len(Family::FourPanel), (16 + 7) * 2 + 2);
    }
}
