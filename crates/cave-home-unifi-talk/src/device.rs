//! The intercom / phone device roster model.
//!
//! A [`TalkDevice`] is one physical endpoint in the home: a desk phone in the
//! study, a wall intercom panel, the front-door doorbell. Devices are
//! vendor-neutral here — the (deferred) UniFi Talk provisioning adapter maps
//! the controller's device list onto these types, and everything downstream
//! (extensions, routing) works off this model alone.

/// A stable identifier for a device. Small, copyable, comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub u32);

/// What kind of endpoint a device is. This drives the grandma-friendly phrasing
/// ("the front-door intercom is calling" vs "the study phone is calling").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// A handset on a desk or wall — the study phone, the kitchen phone.
    DeskPhone,
    /// A wall / room intercom panel.
    Intercom,
    /// A door station / doorbell with a call button.
    Doorbell,
}

impl DeviceKind {
    /// A short, household-level label for this kind of device (Charter §6.3 —
    /// no protocol or model jargon).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DeskPhone => "phone",
            Self::Intercom => "intercom",
            Self::Doorbell => "doorbell",
        }
    }
}

/// Whether a device is currently reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceState {
    /// Registered and reachable — can ring.
    Online,
    /// Not reachable — calls to it cannot ring and are skipped during routing.
    Offline,
}

/// One physical intercom / phone endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalkDevice {
    id: DeviceId,
    name: String,
    kind: DeviceKind,
    state: DeviceState,
}

impl TalkDevice {
    /// Register a device. A freshly modelled device starts [`DeviceState::Online`]
    /// unless [`TalkDevice::with_state`] says otherwise.
    #[must_use]
    pub fn new(id: DeviceId, name: impl Into<String>, kind: DeviceKind) -> Self {
        Self { id, name: name.into(), kind, state: DeviceState::Online }
    }

    /// Builder-style override of the initial reachability state.
    #[must_use]
    pub fn with_state(mut self, state: DeviceState) -> Self {
        self.state = state;
        self
    }

    #[must_use]
    pub const fn id(&self) -> DeviceId {
        self.id
    }

    /// The household-given name ("Study phone", "Front-door intercom").
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> DeviceKind {
        self.kind
    }

    #[must_use]
    pub const fn state(&self) -> DeviceState {
        self.state
    }

    /// Whether this device can currently ring.
    #[must_use]
    pub const fn is_online(&self) -> bool {
        matches!(self.state, DeviceState::Online)
    }

    /// Mark the device reachable / unreachable (the provisioning adapter calls
    /// this when the controller reports a presence change).
    pub fn set_state(&mut self, state: DeviceState) {
        self.state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_device_is_online_by_default() {
        let d = TalkDevice::new(DeviceId(1), "Study phone", DeviceKind::DeskPhone);
        assert!(d.is_online());
        assert_eq!(d.name(), "Study phone");
        assert_eq!(d.kind(), DeviceKind::DeskPhone);
    }

    #[test]
    fn with_state_overrides_default() {
        let d = TalkDevice::new(DeviceId(2), "Gate", DeviceKind::Doorbell)
            .with_state(DeviceState::Offline);
        assert!(!d.is_online());
        assert_eq!(d.state(), DeviceState::Offline);
    }

    #[test]
    fn set_state_flips_reachability() {
        let mut d = TalkDevice::new(DeviceId(3), "Hall panel", DeviceKind::Intercom);
        assert!(d.is_online());
        d.set_state(DeviceState::Offline);
        assert!(!d.is_online());
        d.set_state(DeviceState::Online);
        assert!(d.is_online());
    }

    #[test]
    fn device_kind_labels_are_household_words() {
        assert_eq!(DeviceKind::DeskPhone.label(), "phone");
        assert_eq!(DeviceKind::Intercom.label(), "intercom");
        assert_eq!(DeviceKind::Doorbell.label(), "doorbell");
    }

    #[test]
    fn device_ids_order_and_compare() {
        assert!(DeviceId(1) < DeviceId(2));
        assert_eq!(DeviceId(7), DeviceId(7));
    }
}
