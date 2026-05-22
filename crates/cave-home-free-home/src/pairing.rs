// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: kingsleyadam/local-abbfreeathome@1f6e3ebc src/abbfreeathome/bin/pairing.py (subset)
// Source: Busch-Jaeger public free@home pairing table (free@home System
//         Manual §13 "Datapunkt-Schnittstelle"). The Pairing enum values
//         are public protocol identifiers; the integers below match what
//         the SysAP returns in its /api/rest/configuration JSON.
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! free@home pairing IDs.
//!
//! Each input/output on a SysAP channel is tagged with a "pairing ID" — an
//! integer that identifies the *role* of the datapoint (switch input vs.
//! switch state output vs. brightness output, etc.). These IDs are
//! defined by Busch-Jaeger and stable across firmware releases.
//!
//! Only the IDs used in Phase 1 are enumerated here. Numeric round-trip
//! is preserved so the full Busch-Jaeger pairing table can be added in
//! Phase 2 without API churn.

/// A subset of the free@home pairing-ID table — Phase 1 entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Pairing {
    // Switching
    AlSwitchOnOff = 0x0001,
    AlInfoOnOff = 0x0006,
    AlForced = 0x0002,
    AlInfoForce = 0x0007,
    // Dimming
    AlAbsoluteSetValue = 0x0011,
    AlInfoActualDimmingValue = 0x0015,
    // Cover / blinds
    AlMoveUpDown = 0x0020,
    AlInfoMoveUpDown = 0x0084,
    AlStopStepUpDown = 0x0021,
    AlSetAbsolutePositionBlinds = 0x0023,
    AlCurrentAbsolutePositionBlindsPercentage = 0x0085,
    // Sensors
    AlOutdoorTemperature = 0x0050,
    AlFrostAlarm = 0x0051,
    AlBrightnessLevel = 0x0052,
    AlMovementDetectorStatus = 0x0006_0000,
    // Climate (heating actuator on/off — same code as switching since
    // free@home maps it onto a binary actuator channel).
    AlControllerOnOff = 0x0056,
    // M-wire (1-wire-class binary actuator)
    AlMwireSwitchOnOff = 0x000A,
}

impl Pairing {
    /// Numeric pairing-ID value as exposed by the SysAP REST API.
    #[must_use]
    pub const fn value(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_pairing_ids_stable() {
        // The integers below are protocol-stable; pin them.
        assert_eq!(Pairing::AlSwitchOnOff.value(), 0x0001);
        assert_eq!(Pairing::AlInfoOnOff.value(), 0x0006);
        assert_eq!(Pairing::AlAbsoluteSetValue.value(), 0x0011);
        assert_eq!(Pairing::AlMoveUpDown.value(), 0x0020);
    }
}
