// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/telegram/telegram.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/telegram/apci.py (subset)
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! Module for KNX Telegrams.
//!
//! A `Telegram` is the data transfer object exchanged between higher-level
//! cave-home automation logic and the KNX/IP transport layer below. We
//! mirror the `Telegram` dataclass and the subset of APCI types we actually
//! encode/decode in Phase 1 (GroupValueRead / GroupValueWrite /
//! GroupValueResponse — DPT-1/5/9/14 traffic, the 99.9% case in real
//! installations).

use crate::address::{GroupAddress, IndividualAddress};

/// Direction of a telegram (mirror of `TelegramDirection` upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TelegramDirection {
    Incoming,
    #[default]
    Outgoing,
}

/// Destination of a KNX telegram. Internal group addresses are an xknx
/// convention; we keep the variant for parity but Phase 1 only uses
/// `Group` / `Individual`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TelegramDestination {
    Group(GroupAddress),
    Individual(IndividualAddress),
}

/// APCI payload subset we encode/decode today.
///
/// Codes derived from the public KNX Application Layer table (4-bit APCI
/// then 6-bit sub-code), already cited in the MIT-licensed xknx upstream's
/// `apci.py` header.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Apci {
    /// `A_GroupValue_Read` — APCI code 0b0000.
    GroupValueRead,
    /// `A_GroupValue_Write` — APCI code 0b0010. Carries 0..=14 bytes of data.
    GroupValueWrite(Vec<u8>),
    /// `A_GroupValue_Response` — APCI code 0b0001.
    GroupValueResponse(Vec<u8>),
}

impl Apci {
    /// Whether this APCI is structurally a 6-bit-data (small) payload —
    /// fits in the lower nibble of the APCI byte.
    #[must_use]
    pub fn is_small(&self) -> bool {
        match self {
            Self::GroupValueRead => true,
            Self::GroupValueWrite(data) | Self::GroupValueResponse(data) => data.len() == 1,
        }
    }
}

/// KNX Telegram — DTO between automation and KNX/IP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Telegram {
    pub destination_address: TelegramDestination,
    pub direction: TelegramDirection,
    pub payload: Option<Apci>,
    pub source_address: IndividualAddress,
}

impl Telegram {
    /// Convenience constructor matching the dataclass defaults upstream.
    #[must_use]
    pub fn new(destination_address: TelegramDestination, payload: Option<Apci>) -> Self {
        Self {
            destination_address,
            direction: TelegramDirection::Outgoing,
            payload,
            source_address: IndividualAddress::from_raw(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_telegram_is_outgoing() {
        let t = Telegram::new(
            TelegramDestination::Group(GroupAddress::parse("1/2/3").unwrap()),
            None,
        );
        assert_eq!(t.direction, TelegramDirection::Outgoing);
        assert!(t.payload.is_none());
    }

    #[test]
    fn small_apci_classification() {
        assert!(Apci::GroupValueRead.is_small());
        assert!(Apci::GroupValueWrite(vec![0x01]).is_small());
        assert!(!Apci::GroupValueWrite(vec![0x01, 0x02]).is_small());
    }
}
