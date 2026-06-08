//! The [`Integration`] descriptor and the [`Registry`] that holds them.
//!
//! An [`Integration`] describes *a kind of thing* cave-home can connect: its
//! domain id, display name, the capabilities it provides, what it depends on,
//! how it's discovered and how it's set up. It is HA's `manifest.json` +
//! `Integration` rolled into a pure-data descriptor; nothing here touches the
//! network.

use crate::capability::Capability;
use crate::discovery::Discovered;

/// How "online" an integration is — mirrors HA's `iot_class`. Drives whether
/// the hub expects a live push connection or polls on a timer, which in turn
/// informs the retry policy ([`crate::lifecycle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IotClass {
    /// Talks locally and pushes updates (e.g. a local hub on the LAN).
    LocalPush,
    /// Talks locally but must be polled.
    LocalPoll,
    /// Talks to a vendor cloud and is pushed to.
    CloudPush,
    /// Talks to a vendor cloud and must be polled.
    CloudPoll,
}

impl IotClass {
    /// Whether reaching this thing crosses the internet — cloud classes do,
    /// local classes don't. (Used to phrase transient-failure expectations.)
    #[must_use]
    pub const fn is_cloud(self) -> bool {
        matches!(self, Self::CloudPush | Self::CloudPoll)
    }
}

/// How a household sets this integration up — mirrors HA config-flow shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFlow {
    /// Appears on its own once discovered on the network; the household just
    /// confirms.
    Discoverable,
    /// The household types in an address / details by hand.
    Manual,
    /// The household signs in to a vendor account.
    OAuth,
}

/// A descriptor for one kind of connectable thing.
///
/// Built fluently; every field has a sensible default so a minimal descriptor
/// is `Integration::new(domain, name)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Integration {
    domain: String,
    name: String,
    capabilities: Vec<Capability>,
    /// Other integrations that must be loaded for this one to work at all.
    dependencies: Vec<String>,
    /// Other integrations that should set up *before* this one if present, but
    /// are not strictly required (HA `after_dependencies`).
    after_dependencies: Vec<String>,
    iot_class: IotClass,
    config_flow: ConfigFlow,
    /// Discovery service types this integration answers to (e.g. `_hue._tcp`).
    discovery_keys: Vec<String>,
}

impl Integration {
    /// A minimal integration: a domain id and a display name.
    ///
    /// Defaults to a locally-polled, manually-configured integration with no
    /// capabilities, dependencies or discovery.
    #[must_use]
    pub fn new(domain: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            name: name.into(),
            capabilities: Vec::new(),
            dependencies: Vec::new(),
            after_dependencies: Vec::new(),
            iot_class: IotClass::LocalPoll,
            config_flow: ConfigFlow::Manual,
            discovery_keys: Vec::new(),
        }
    }

    /// Add a capability this integration provides.
    #[must_use]
    pub fn with_capability(mut self, cap: Capability) -> Self {
        if !self.capabilities.contains(&cap) {
            self.capabilities.push(cap);
        }
        self
    }

    /// Declare a hard dependency on another integration's domain.
    #[must_use]
    pub fn depends_on(mut self, domain: impl Into<String>) -> Self {
        self.dependencies.push(domain.into());
        self
    }

    /// Declare a soft load-order preference (set up after this domain *if* it's
    /// present).
    #[must_use]
    pub fn after(mut self, domain: impl Into<String>) -> Self {
        self.after_dependencies.push(domain.into());
        self
    }

    /// Set the iot-class.
    #[must_use]
    pub const fn with_iot_class(mut self, class: IotClass) -> Self {
        self.iot_class = class;
        self
    }

    /// Set the config-flow type.
    #[must_use]
    pub const fn with_config_flow(mut self, flow: ConfigFlow) -> Self {
        self.config_flow = flow;
        self
    }

    /// Register a discovery service type this integration handles. Implies a
    /// discoverable config flow unless overridden afterwards.
    #[must_use]
    pub fn discoverable_by(mut self, key: impl Into<String>) -> Self {
        self.discovery_keys.push(key.into());
        self.config_flow = ConfigFlow::Discoverable;
        self
    }

    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    #[must_use]
    pub fn dependencies(&self) -> &[String] {
        &self.dependencies
    }

    #[must_use]
    pub fn after_dependencies(&self) -> &[String] {
        &self.after_dependencies
    }

    #[must_use]
    pub const fn iot_class(&self) -> IotClass {
        self.iot_class
    }

    #[must_use]
    pub const fn config_flow(&self) -> ConfigFlow {
        self.config_flow
    }

    #[must_use]
    pub fn discovery_keys(&self) -> &[String] {
        &self.discovery_keys
    }

    /// Whether this integration can handle a given discovery signal: the
    /// signal's service key must be one we registered. Property matching is
    /// left to the caller / a future config-flow step.
    #[must_use]
    pub fn handles(&self, found: &Discovered) -> bool {
        self.discovery_keys.iter().any(|k| k == found.key())
    }
}

/// A collection of known integration descriptors, keyed by domain.
///
/// Insertion order is preserved so deterministic outputs (discovery matches,
/// resolver order for ties) are reproducible across runs.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    integrations: Vec<Integration>,
}

impl Registry {
    #[must_use]
    pub const fn new() -> Self {
        Self { integrations: Vec::new() }
    }

    /// Register (or replace, by domain) an integration descriptor.
    pub fn register(&mut self, integration: Integration) {
        if let Some(slot) = self
            .integrations
            .iter_mut()
            .find(|i| i.domain() == integration.domain())
        {
            *slot = integration;
        } else {
            self.integrations.push(integration);
        }
    }

    /// Look up an integration by domain.
    #[must_use]
    pub fn get(&self, domain: &str) -> Option<&Integration> {
        self.integrations.iter().find(|i| i.domain() == domain)
    }

    /// All registered integrations, in registration order.
    #[must_use]
    pub fn all(&self) -> &[Integration] {
        &self.integrations
    }

    /// Match a discovery signal to every integration domain that can handle it,
    /// in registration order. The discovery module layers dedupe on top.
    #[must_use]
    pub fn match_discovery(&self, found: &Discovered) -> Vec<String> {
        self.integrations
            .iter()
            .filter(|i| i.handles(found))
            .map(|i| i.domain().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_fields() {
        let i = Integration::new("hue", "Philips Hue")
            .with_capability(Capability::Light)
            .with_capability(Capability::Sensor)
            .depends_on("network")
            .after("mqtt")
            .with_iot_class(IotClass::LocalPush)
            .discoverable_by("_hue._tcp");
        assert_eq!(i.domain(), "hue");
        assert_eq!(i.name(), "Philips Hue");
        assert_eq!(i.capabilities(), &[Capability::Light, Capability::Sensor]);
        assert_eq!(i.dependencies(), &["network".to_string()]);
        assert_eq!(i.after_dependencies(), &["mqtt".to_string()]);
        assert_eq!(i.iot_class(), IotClass::LocalPush);
        // discoverable_by flips the flow.
        assert_eq!(i.config_flow(), ConfigFlow::Discoverable);
    }

    #[test]
    fn duplicate_capability_is_ignored() {
        let i = Integration::new("x", "X")
            .with_capability(Capability::Light)
            .with_capability(Capability::Light);
        assert_eq!(i.capabilities(), &[Capability::Light]);
    }

    #[test]
    fn iot_class_cloud_split() {
        assert!(IotClass::CloudPoll.is_cloud());
        assert!(IotClass::CloudPush.is_cloud());
        assert!(!IotClass::LocalPush.is_cloud());
        assert!(!IotClass::LocalPoll.is_cloud());
    }

    #[test]
    fn registry_register_and_get() {
        let mut r = Registry::new();
        r.register(Integration::new("a", "A"));
        r.register(Integration::new("b", "B"));
        assert_eq!(r.all().len(), 2);
        assert_eq!(r.get("a").map(Integration::name), Some("A"));
        assert!(r.get("missing").is_none());
    }

    #[test]
    fn registry_register_replaces_by_domain() {
        let mut r = Registry::new();
        r.register(Integration::new("a", "A old"));
        r.register(Integration::new("a", "A new"));
        assert_eq!(r.all().len(), 1);
        assert_eq!(r.get("a").map(Integration::name), Some("A new"));
    }
}
