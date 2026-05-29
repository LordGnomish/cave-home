//! Network-device model — the switches, access points and gateway.
//!
//! A [`NetworkDevice`] is the cave-home-neutral shape that a phase-1b `UniFi`
//! controller adapter (deferred — see the parity manifest) maps its JSON onto.
//! Everything downstream — control validation, the connectivity summary,
//! grandma-friendly labels — works off this model alone, never the wire format.

/// What kind of network device this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// A managed switch (has [`SwitchPort`]s, some PoE-capable).
    Switch,
    /// A Wi-Fi access point.
    AccessPoint,
    /// The gateway / router — the device with the internet uplink.
    Gateway,
}

impl DeviceKind {
    /// Plain-language device kind for the household (Charter §6.3).
    #[must_use]
    pub const fn household_word(self) -> &'static str {
        match self {
            Self::Switch => "switch",
            Self::AccessPoint => "Wi-Fi point",
            Self::Gateway => "internet box",
        }
    }
}

/// Whether the device is currently reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Online,
    Offline,
}

impl DeviceState {
    #[must_use]
    pub const fn is_online(self) -> bool {
        matches!(self, Self::Online)
    }
}

/// A single physical port on a [`DeviceKind::Switch`].
///
/// Ports are numbered from 1 (`UniFi`'s own convention). `poe_capable` records
/// whether the port can deliver Power-over-Ethernet at all — a non-PoE port
/// can never be switched to power a camera, and the control engine rejects
/// such an attempt up front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchPort {
    /// 1-based port number.
    pub number: u16,
    /// Whether this port can deliver `PoE`.
    pub poe_capable: bool,
    /// Whether the port is currently delivering power.
    pub poe_active: bool,
}

impl SwitchPort {
    #[must_use]
    pub const fn new(number: u16, poe_capable: bool) -> Self {
        Self { number, poe_capable, poe_active: false }
    }
}

/// A network device: a switch, an access point, or the gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkDevice {
    id: String,
    name: String,
    mac: String,
    kind: DeviceKind,
    state: DeviceState,
    model: String,
    /// The id of the device this one uplinks to, if any. The gateway has no
    /// uplink device (its uplink is the internet itself).
    uplink: Option<String>,
    ports: Vec<SwitchPort>,
}

impl NetworkDevice {
    /// Construct a device, online by default, with no ports and no uplink.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        mac: impl Into<String>,
        kind: DeviceKind,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            mac: mac.into(),
            kind,
            state: DeviceState::Online,
            model: String::new(),
            uplink: None,
            ports: Vec::new(),
        }
    }

    /// Builder: set the hardware model string.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Builder: mark the device offline.
    #[must_use]
    pub fn offline(mut self) -> Self {
        self.state = DeviceState::Offline;
        self
    }

    /// Builder: record the uplink device id.
    #[must_use]
    pub fn uplinked_to(mut self, device_id: impl Into<String>) -> Self {
        self.uplink = Some(device_id.into());
        self
    }

    /// Builder: attach switch ports.
    #[must_use]
    pub fn with_ports(mut self, ports: Vec<SwitchPort>) -> Self {
        self.ports = ports;
        self
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn mac(&self) -> &str {
        &self.mac
    }

    #[must_use]
    pub const fn kind(&self) -> DeviceKind {
        self.kind
    }

    #[must_use]
    pub const fn state(&self) -> DeviceState {
        self.state
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn uplink(&self) -> Option<&str> {
        self.uplink.as_deref()
    }

    #[must_use]
    pub fn ports(&self) -> &[SwitchPort] {
        &self.ports
    }

    #[must_use]
    pub const fn is_online(&self) -> bool {
        self.state.is_online()
    }

    /// Look up a port by its 1-based number.
    #[must_use]
    pub fn port(&self, number: u16) -> Option<&SwitchPort> {
        self.ports.iter().find(|p| p.number == number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_device_is_online_with_no_ports() {
        let d = NetworkDevice::new("sw-1", "Living-room switch", "aa:00", DeviceKind::Switch);
        assert!(d.is_online());
        assert_eq!(d.state(), DeviceState::Online);
        assert!(d.ports().is_empty());
        assert_eq!(d.uplink(), None);
        assert_eq!(d.kind(), DeviceKind::Switch);
    }

    #[test]
    fn builders_set_fields() {
        let d = NetworkDevice::new("ap-1", "Hallway AP", "bb:11", DeviceKind::AccessPoint)
            .with_model("U6-Lite")
            .offline()
            .uplinked_to("sw-1")
            .with_ports(vec![SwitchPort::new(1, true)]);
        assert_eq!(d.model(), "U6-Lite");
        assert!(!d.is_online());
        assert_eq!(d.uplink(), Some("sw-1"));
        assert_eq!(d.ports().len(), 1);
    }

    #[test]
    fn port_lookup_by_number() {
        let d = NetworkDevice::new("sw-1", "Switch", "aa:00", DeviceKind::Switch)
            .with_ports(vec![SwitchPort::new(1, true), SwitchPort::new(2, false)]);
        assert!(d.port(1).unwrap().poe_capable);
        assert!(!d.port(2).unwrap().poe_capable);
        assert!(d.port(99).is_none());
    }

    #[test]
    fn device_kind_household_words_are_plain() {
        assert_eq!(DeviceKind::Switch.household_word(), "switch");
        assert_eq!(DeviceKind::AccessPoint.household_word(), "Wi-Fi point");
        assert_eq!(DeviceKind::Gateway.household_word(), "internet box");
    }

    #[test]
    fn new_port_is_not_active() {
        let p = SwitchPort::new(7, true);
        assert!(p.poe_capable);
        assert!(!p.poe_active);
    }
}
