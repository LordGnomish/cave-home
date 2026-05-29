//! Extensions and ring groups.
//!
//! An [`Extension`] is a dialable destination — a number a household member
//! picks ("101 = kitchen", "200 = front door"), a display name, the device it
//! rings, and whether it falls back to voicemail. A [`CallGroup`] is a set of
//! extensions that ring together under a [`RingStrategy`] — "ring every phone
//! in the house", "try the study first then the kitchen", or share the load
//! round-robin.

use crate::device::DeviceId;

/// A dialable extension number. Stored as a small string so "101", "0", and a
/// leading-zero extension all round-trip exactly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionNumber(String);

impl ExtensionNumber {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ExtensionNumber {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why an extension or call group could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionError {
    /// The extension number was empty or not made only of decimal digits.
    BadNumber,
    /// A call group was given no members — there would be nobody to ring.
    EmptyGroup,
}

impl core::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadNumber => f.write_str("extension number must be digits only"),
            Self::EmptyGroup => f.write_str("a call group needs at least one member"),
        }
    }
}

impl std::error::Error for ExtensionError {}

/// One dialable extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    number: ExtensionNumber,
    display_name: String,
    device: DeviceId,
    voicemail_enabled: bool,
}

impl Extension {
    /// Define an extension. Voicemail starts enabled; use
    /// [`Extension::with_voicemail`] to turn it off.
    ///
    /// # Errors
    /// [`ExtensionError::BadNumber`] if `number` is empty or contains a
    /// non-digit character.
    pub fn new(
        number: &str,
        display_name: impl Into<String>,
        device: DeviceId,
    ) -> Result<Self, ExtensionError> {
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ExtensionError::BadNumber);
        }
        Ok(Self {
            number: ExtensionNumber(number.to_owned()),
            display_name: display_name.into(),
            device,
            voicemail_enabled: true,
        })
    }

    /// Builder-style override of whether this extension answers to voicemail
    /// when a ring goes unanswered.
    #[must_use]
    pub fn with_voicemail(mut self, enabled: bool) -> Self {
        self.voicemail_enabled = enabled;
        self
    }

    #[must_use]
    pub const fn number(&self) -> &ExtensionNumber {
        &self.number
    }

    /// The household-facing display name ("Kitchen", "Front door").
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The device this extension rings.
    #[must_use]
    pub const fn device(&self) -> DeviceId {
        self.device
    }

    #[must_use]
    pub const fn voicemail_enabled(&self) -> bool {
        self.voicemail_enabled
    }
}

/// How a [`CallGroup`] distributes an incoming call across its members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingStrategy {
    /// Ring every member at once — first to answer wins.
    RingAll,
    /// Ring members one at a time, in listed order, until one answers.
    Sequential,
    /// Ring members one at a time, but start after the member who took the last
    /// call, spreading the load evenly. The caller supplies the rotation offset.
    RoundRobin,
}

/// A set of extensions that ring together — "the whole house", "downstairs".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallGroup {
    name: String,
    strategy: RingStrategy,
    members: Vec<ExtensionNumber>,
}

impl CallGroup {
    /// Build a ring group.
    ///
    /// # Errors
    /// [`ExtensionError::EmptyGroup`] if `members` is empty.
    pub fn new(
        name: impl Into<String>,
        strategy: RingStrategy,
        members: Vec<ExtensionNumber>,
    ) -> Result<Self, ExtensionError> {
        if members.is_empty() {
            return Err(ExtensionError::EmptyGroup);
        }
        Ok(Self { name: name.into(), strategy, members })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn strategy(&self) -> RingStrategy {
        self.strategy
    }

    #[must_use]
    pub fn members(&self) -> &[ExtensionNumber] {
        &self.members
    }

    /// The order in which this group's members should be rung, given a
    /// rotation `offset` (only used by [`RingStrategy::RoundRobin`]; ignored by
    /// the others). The offset wraps, so any value is safe.
    ///
    /// - [`RingStrategy::RingAll`] returns every member (they ring at once, but
    ///   the order is still meaningful for tie-breaking the answer).
    /// - [`RingStrategy::Sequential`] returns the listed order unchanged.
    /// - [`RingStrategy::RoundRobin`] rotates the listed order so the member at
    ///   `offset` rings first.
    #[must_use]
    pub fn ring_order(&self, offset: usize) -> Vec<ExtensionNumber> {
        match self.strategy {
            RingStrategy::RingAll | RingStrategy::Sequential => self.members.clone(),
            RingStrategy::RoundRobin => {
                let n = self.members.len();
                let start = offset % n;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    out.push(self.members[(start + i) % n].clone());
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(n: &str) -> ExtensionNumber {
        Extension::new(n, "x", DeviceId(0)).unwrap().number().clone()
    }

    #[test]
    fn extension_rejects_non_digit_numbers() {
        assert_eq!(
            Extension::new("10A", "Kitchen", DeviceId(1)),
            Err(ExtensionError::BadNumber)
        );
        assert_eq!(Extension::new("", "Kitchen", DeviceId(1)), Err(ExtensionError::BadNumber));
    }

    #[test]
    fn extension_accepts_leading_zero_and_keeps_it() {
        let e = Extension::new("007", "Spy phone", DeviceId(1)).unwrap();
        assert_eq!(e.number().as_str(), "007");
        assert_eq!(e.display_name(), "Spy phone");
    }

    #[test]
    fn voicemail_defaults_on_and_can_be_disabled() {
        let e = Extension::new("101", "Kitchen", DeviceId(1)).unwrap();
        assert!(e.voicemail_enabled());
        let e = e.with_voicemail(false);
        assert!(!e.voicemail_enabled());
    }

    #[test]
    fn empty_call_group_is_rejected() {
        assert_eq!(
            CallGroup::new("Nobody", RingStrategy::RingAll, vec![]),
            Err(ExtensionError::EmptyGroup)
        );
    }

    #[test]
    fn ring_all_returns_every_member_in_order() {
        let g = CallGroup::new(
            "House",
            RingStrategy::RingAll,
            vec![ext("101"), ext("102"), ext("103")],
        )
        .unwrap();
        let order = g.ring_order(0);
        assert_eq!(order, vec![ext("101"), ext("102"), ext("103")]);
    }

    #[test]
    fn sequential_keeps_listed_order_regardless_of_offset() {
        let g = CallGroup::new(
            "Downstairs",
            RingStrategy::Sequential,
            vec![ext("101"), ext("102")],
        )
        .unwrap();
        assert_eq!(g.ring_order(5), vec![ext("101"), ext("102")]);
    }

    #[test]
    fn round_robin_rotates_to_start_at_offset() {
        let g = CallGroup::new(
            "Support",
            RingStrategy::RoundRobin,
            vec![ext("101"), ext("102"), ext("103")],
        )
        .unwrap();
        assert_eq!(g.ring_order(0), vec![ext("101"), ext("102"), ext("103")]);
        assert_eq!(g.ring_order(1), vec![ext("102"), ext("103"), ext("101")]);
        assert_eq!(g.ring_order(2), vec![ext("103"), ext("101"), ext("102")]);
    }

    #[test]
    fn round_robin_offset_wraps_safely() {
        let g = CallGroup::new("Pair", RingStrategy::RoundRobin, vec![ext("101"), ext("102")])
            .unwrap();
        // offset 7 % 2 == 1 -> starts at second member.
        assert_eq!(g.ring_order(7), vec![ext("102"), ext("101")]);
    }
}
