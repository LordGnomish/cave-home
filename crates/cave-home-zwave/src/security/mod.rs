// SPDX-License-Identifier: Apache-2.0
//! Z-Wave security framework — S0 (Legacy) + S2 (CSA / CKD).
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/core/src/security/
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/core/src/definitions/SecurityClass.ts
//!
//! Phase 1 ports the cryptographic primitives + key-management surfaces. The
//! end-to-end include flow uses the surfaces here via [`crate::inclusion`].
//!
//! The user never sees any of this — Charter v2 / ADR-007 forbids "Security
//! Class", "DSK", "Network Key" from appearing in Portal or Mobile UIs.

pub mod crypto;
pub mod s0;
pub mod s2;

pub use s0::{S0Manager, generate_auth_key, generate_encryption_key};
pub use s2::{S2Keys, S2Manager, derive_s2_keys};

/// Security classes a node may negotiate with the controller.
///
/// # Upstream: `SecurityClass.ts::SecurityClass`
///
/// Wire encoding (S2 bootstrap on-the-wire byte) matches upstream — see the
/// `security_class_wire_bytes_match_upstream` test below.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum SecurityClass {
    /// Internal marker used during inclusion. Do not surface.
    Temporary,
    /// Node included without security. Default for legacy / Z-Wave Plus
    /// devices that don't request a key.
    None,
    /// S2 Unauthenticated (no out-of-band check).
    S2Unauthenticated,
    /// S2 Authenticated (5-digit PIN out-of-band check).
    S2Authenticated,
    /// S2 Access Control (full DSK out-of-band check).
    S2AccessControl,
    /// S0 Legacy.
    S0Legacy,
}

impl SecurityClass {
    /// On-the-wire byte for S2 bootstrap (`Security 2 Public Key Report`).
    /// Returns `None` for `Temporary` / `None` (which never appear on the
    /// wire).
    #[must_use]
    pub const fn wire_byte(self) -> Option<u8> {
        match self {
            Self::S2Unauthenticated => Some(0),
            Self::S2Authenticated => Some(1),
            Self::S2AccessControl => Some(2),
            Self::S0Legacy => Some(7),
            Self::Temporary | Self::None => None,
        }
    }

    /// Whether the class is in the S2 family.
    #[must_use]
    pub const fn is_s2(self) -> bool {
        matches!(
            self,
            Self::S2Unauthenticated | Self::S2Authenticated | Self::S2AccessControl
        )
    }
}

/// Highest-priority security class among `securityClassOrder`. Returns
/// [`SecurityClass::None`] if the slice is empty or only contains internal
/// markers.
///
/// # Upstream: `SecurityClass.ts::getHighestSecurityClass`
#[must_use]
pub fn highest_security_class(classes: &[SecurityClass]) -> SecurityClass {
    const ORDER: &[SecurityClass] = &[
        SecurityClass::S2AccessControl,
        SecurityClass::S2Authenticated,
        SecurityClass::S2Unauthenticated,
        SecurityClass::S0Legacy,
    ];
    for cls in ORDER {
        if classes.contains(cls) {
            return *cls;
        }
    }
    SecurityClass::None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire bytes are normative — they must match the bytes the controller
    /// puts on the radio.
    #[test]
    fn security_class_wire_bytes_match_upstream() {
        assert_eq!(SecurityClass::S2Unauthenticated.wire_byte(), Some(0));
        assert_eq!(SecurityClass::S2Authenticated.wire_byte(), Some(1));
        assert_eq!(SecurityClass::S2AccessControl.wire_byte(), Some(2));
        assert_eq!(SecurityClass::S0Legacy.wire_byte(), Some(7));
        assert_eq!(SecurityClass::None.wire_byte(), None);
        assert_eq!(SecurityClass::Temporary.wire_byte(), None);
    }

    #[test]
    fn highest_picks_access_control_over_others() {
        assert_eq!(
            highest_security_class(&[
                SecurityClass::S0Legacy,
                SecurityClass::S2Authenticated,
                SecurityClass::S2AccessControl,
            ]),
            SecurityClass::S2AccessControl
        );
    }

    #[test]
    fn highest_falls_back_to_none() {
        assert_eq!(highest_security_class(&[]), SecurityClass::None);
        assert_eq!(
            highest_security_class(&[SecurityClass::Temporary]),
            SecurityClass::None
        );
    }
}
