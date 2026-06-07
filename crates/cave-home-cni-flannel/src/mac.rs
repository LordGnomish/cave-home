// SPDX-License-Identifier: Apache-2.0
//! VTEP MAC generation — port of flannel `pkg/mac/mac.go::NewHardwareAddr`.
//!
//! flannel gives every node's `flannel.<vni>` device a MAC that is *locally
//! administered* and *unicast*, then advertises it in the node's lease so peers
//! can install the matching FDB/ARP entries. Upstream draws six random bytes
//! from `crypto/rand` and forces the two low bits of the first octet:
//!
//! ```text
//! hardwareAddr[0] = (hardwareAddr[0] & 0xfe) | 0x02
//! ```
//!
//! `& 0xfe` clears the multicast bit (bit 0 → unicast); `| 0x02` sets the
//! locally-administered bit (bit 1). This crate forbids `unsafe` and pulls in
//! no RNG dependency, so the *entropy* is injected by the caller (from the OS
//! RNG at the daemon edge, or a deterministic seed in tests); the bit-twiddling
//! — the part that actually defines a valid VTEP MAC — is what we port here.

use crate::backend::MacAddr;

/// The first octet of a locally-administered unicast MAC, given a raw byte.
///
/// Clears the multicast bit and sets the locally-administered bit, exactly as
/// `NewHardwareAddr` does.
#[must_use]
pub const fn locally_administered_octet(first: u8) -> u8 {
    (first & 0xfe) | 0x02
}

/// Build a locally-administered unicast [`MacAddr`] from six bytes of entropy.
///
/// Mirrors `NewHardwareAddr`: take the random/seed bytes verbatim, then force
/// the first octet to be unicast + locally administered. With OS entropy this
/// is flannel's behaviour; with a fixed seed it is reproducible for tests and
/// the network simulator.
#[must_use]
pub const fn new_hardware_addr(entropy: [u8; 6]) -> MacAddr {
    let mut octets = entropy;
    octets[0] = locally_administered_octet(octets[0]);
    MacAddr::new(octets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_octet_is_unicast_and_locally_administered() {
        // Multicast bit (0x01) cleared, LA bit (0x02) set, for any input.
        for b in 0u8..=255 {
            let o = locally_administered_octet(b);
            assert_eq!(o & 0x01, 0x00, "unicast bit must be clear for {b:#x}");
            assert_eq!(o & 0x02, 0x02, "LA bit must be set for {b:#x}");
        }
    }

    #[test]
    fn preserves_the_other_five_octets() {
        let mac = new_hardware_addr([0xff, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let o = mac.octets();
        // 0xff -> (0xff & 0xfe) | 0x02 = 0xfe | 0x02 = 0xfe.
        assert_eq!(o[0], 0xfe);
        assert_eq!(&o[1..], &[0x11, 0x22, 0x33, 0x44, 0x55]);
    }

    #[test]
    fn already_la_unicast_octet_is_unchanged() {
        // 0x0a = 0000_1010: multicast clear, LA set already.
        assert_eq!(locally_administered_octet(0x0a), 0x0a);
    }

    #[test]
    fn generated_mac_is_distinct_per_seed() {
        let a = new_hardware_addr([1, 2, 3, 4, 5, 6]);
        let b = new_hardware_addr([7, 8, 9, 10, 11, 12]);
        assert_ne!(a, b);
    }
}
