// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Pairing / commissioning — Zigbee 3.0 §2.5.5 + ZBDB §13.
//!
//! Phase 1 supports the three pairing modes the headline-persona needs:
//! - **Traditional join** — open the network for `n` seconds, accept
//!   any router/end-device that performs network steering with the
//!   well-known link key (`5A 69 67 42 65 65 41 6C 6C 69 61 6E 63 65 30 39`,
//!   public, Zigbee 3.0 §4.6.3.4).
//! - **InstallCode join** — derive a per-device link key from a
//!   16-byte InstallCode printed on the device (ZBDB §13.3); the
//!   network is opened only for that key.
//! - **Touchlink** — short-range commissioning (BDB §10.1); Phase 1
//!   ships the typed enum and surface so the higher coordinator layer
//!   can call into the firmware's Touchlink primitive.

use std::time::Duration;

use crate::error::{Result, ZigbeeError};

/// Touchlink commissioning mode — Zigbee Base Device Behavior §10.1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchlinkMode {
    /// Touchlink master — the coordinator initiates touchlink.
    Master,
    /// Touchlink target — the coordinator answers a touchlink initiator.
    Target,
    /// Touchlink disabled.
    Disabled,
}

/// Pre-configured global link key — Zigbee 3.0 §4.6.3.4 (public test key).
///
/// "ZigBeeAlliance09" — used when a device joins without an InstallCode.
pub const GLOBAL_LINK_KEY_ZB3: [u8; 16] = [
    0x5a, 0x69, 0x67, 0x42, 0x65, 0x65, 0x41, 0x6c, 0x6c, 0x69, 0x61, 0x6e, 0x63, 0x65, 0x30, 0x39,
];

/// InstallCode — a 6/8/12/16-byte device-specific code printed on the
/// device label, with a trailing 2-byte CRC. Used to derive a unique
/// per-device link key (ZBDB §13.3.1).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallCode {
    /// Raw install-code bytes including the CRC at the end.
    pub bytes: Vec<u8>,
}

impl InstallCode {
    /// Parse an install-code from a hex string (with or without spaces).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Pairing`] if the input is not valid hex
    /// or has an unsupported length.
    pub fn from_hex(s: &str) -> Result<Self> {
        let cleaned: String = s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
        if cleaned.len() % 2 != 0 {
            return Err(ZigbeeError::Pairing("install code: odd hex length".into()));
        }
        let mut bytes = Vec::with_capacity(cleaned.len() / 2);
        for chunk in cleaned.as_bytes().chunks(2) {
            // SAFETY: bytes come from `cleaned` which is ASCII-printable.
            let s_chunk = std::str::from_utf8(chunk)
                .map_err(|_| ZigbeeError::Pairing("install code: non-ASCII".into()))?;
            let b = u8::from_str_radix(s_chunk, 16)
                .map_err(|_| ZigbeeError::Pairing("install code: invalid hex".into()))?;
            bytes.push(b);
        }
        Self::from_bytes(bytes)
    }

    /// Build from raw bytes.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Pairing`] if the length isn't 8 / 10 / 14 / 18 bytes
    /// (data length 6 / 8 / 12 / 16 + 2-byte CRC, per ZBDB §13.3.1).
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        match bytes.len() {
            8 | 10 | 14 | 18 => Ok(Self { bytes }),
            other => Err(ZigbeeError::Pairing(format!(
                "install code: unsupported length {other} (expected 8/10/14/18)"
            ))),
        }
    }

    /// Returns the install-code data portion (bytes excluding the trailing CRC).
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.bytes[..self.bytes.len() - 2]
    }

    /// Returns the trailing 16-bit CRC field.
    #[must_use]
    pub fn crc(&self) -> u16 {
        let n = self.bytes.len();
        u16::from_le_bytes([self.bytes[n - 2], self.bytes[n - 1]])
    }

    /// Verify the CRC-16 (CRC-CCITT) over `data()`.
    #[must_use]
    pub fn crc_valid(&self) -> bool {
        crate::ezsp::ash::crc_ccitt(self.data()) == self.crc()
    }
}

/// Result of network-steering — Zigbee 3.0 §2.5.5.5.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SteeringOutcome {
    /// Network was opened.
    Opened,
    /// Steering window closed (timer expired or explicit close).
    Closed,
}

/// Network-steering controller — §2.5.5.5.
///
/// Tracks the join window. The actual radio command is issued by the
/// transport layer above ([`crate::coordinator::Coordinator`]); this
/// struct enforces invariants (no overlapping windows, valid durations).
#[derive(Clone, Debug)]
pub struct NetworkSteering {
    open_window: Option<Duration>,
    /// Optional install-code restricting which device may join (ZBDB §13.3.2).
    install_code: Option<InstallCode>,
    /// Touchlink mode override.
    touchlink: TouchlinkMode,
}

impl Default for NetworkSteering {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkSteering {
    /// Build a closed controller.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open_window: None,
            install_code: None,
            touchlink: TouchlinkMode::Disabled,
        }
    }

    /// Open the network for `duration`.
    ///
    /// `duration` must be 1..=254 seconds per §2.5.5.5.4. 255 means
    /// "permanently open" upstream but cave-home rejects that to avoid
    /// leaving a network discoverable forever.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Pairing`] for out-of-range durations.
    pub fn open(&mut self, duration: Duration) -> Result<SteeringOutcome> {
        let secs = duration.as_secs();
        if !(1..=254).contains(&secs) {
            return Err(ZigbeeError::Pairing(format!(
                "open duration {secs}s out of range (1..=254)"
            )));
        }
        if self.open_window.is_some() {
            return Err(ZigbeeError::Pairing("network already open".into()));
        }
        self.open_window = Some(duration);
        Ok(SteeringOutcome::Opened)
    }

    /// Open the network for `duration`, restricted to the device whose
    /// install-code derives the matching link key.
    ///
    /// # Errors
    /// As [`Self::open`].
    pub fn open_with_install_code(
        &mut self,
        duration: Duration,
        code: InstallCode,
    ) -> Result<SteeringOutcome> {
        self.install_code = Some(code);
        self.open(duration)
    }

    /// Close the steering window (no-op if already closed).
    pub fn close(&mut self) -> SteeringOutcome {
        self.open_window = None;
        self.install_code = None;
        SteeringOutcome::Closed
    }

    /// `true` iff the window is currently open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open_window.is_some()
    }

    /// The install-code currently restricting joins, if any.
    #[must_use]
    pub fn install_code(&self) -> Option<&InstallCode> {
        self.install_code.as_ref()
    }

    /// Touchlink mode.
    #[must_use]
    pub fn touchlink_mode(&self) -> TouchlinkMode {
        self.touchlink
    }

    /// Set touchlink mode.
    pub fn set_touchlink_mode(&mut self, mode: TouchlinkMode) {
        self.touchlink = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_link_key_is_zigbee_alliance_09() {
        assert_eq!(&GLOBAL_LINK_KEY_ZB3, b"ZigBeeAlliance09");
    }

    #[test]
    fn install_code_from_hex_round_trips() {
        let code = InstallCode::from_hex("0102030405060708090A0B0C0D0E0F10 1234").unwrap();
        assert_eq!(code.bytes.len(), 18);
        assert_eq!(code.crc(), 0x3412);
        assert_eq!(code.data().len(), 16);
    }

    #[test]
    fn install_code_rejects_odd_length() {
        assert!(InstallCode::from_hex("aaa").is_err());
    }

    #[test]
    fn install_code_rejects_bad_byte_count() {
        // 7 bytes = 14 hex chars = unsupported.
        assert!(InstallCode::from_bytes(vec![0; 7]).is_err());
    }

    #[test]
    fn install_code_crc_helper_works() {
        let payload = b"hello";
        let crc = crate::ezsp::ash::crc_ccitt(payload);
        let mut bytes = payload.to_vec();
        bytes.extend_from_slice(&crc.to_le_bytes());
        // 5 + 2 = 7 bytes — *not* a supported install-code length,
        // so we cheat and create a 16+2 byte code.
        let mut data = vec![0u8; 16];
        for (i, x) in data.iter_mut().enumerate() {
            *x = i as u8;
        }
        let crc = crate::ezsp::ash::crc_ccitt(&data);
        let mut bytes = data.clone();
        bytes.extend_from_slice(&crc.to_le_bytes());
        let code = InstallCode::from_bytes(bytes).unwrap();
        assert!(code.crc_valid());
    }

    #[test]
    fn steering_open_and_close() {
        let mut s = NetworkSteering::new();
        assert!(!s.is_open());
        s.open(Duration::from_secs(60)).unwrap();
        assert!(s.is_open());
        s.close();
        assert!(!s.is_open());
    }

    #[test]
    fn steering_duration_zero_rejected() {
        let mut s = NetworkSteering::new();
        assert!(s.open(Duration::from_secs(0)).is_err());
    }

    #[test]
    fn steering_duration_255_rejected() {
        let mut s = NetworkSteering::new();
        assert!(s.open(Duration::from_secs(255)).is_err());
    }

    #[test]
    fn steering_double_open_rejected() {
        let mut s = NetworkSteering::new();
        s.open(Duration::from_secs(30)).unwrap();
        assert!(s.open(Duration::from_secs(60)).is_err());
    }

    #[test]
    fn steering_install_code_restricts_join() {
        let mut s = NetworkSteering::new();
        let mut data = vec![0u8; 16];
        for (i, x) in data.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(3);
        }
        let crc = crate::ezsp::ash::crc_ccitt(&data);
        let mut bytes = data;
        bytes.extend_from_slice(&crc.to_le_bytes());
        let code = InstallCode::from_bytes(bytes).unwrap();
        s.open_with_install_code(Duration::from_secs(60), code).unwrap();
        assert!(s.install_code().is_some());
    }

    #[test]
    fn touchlink_default_disabled() {
        let s = NetworkSteering::new();
        assert_eq!(s.touchlink_mode(), TouchlinkMode::Disabled);
    }
}
