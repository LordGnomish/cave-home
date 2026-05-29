//! `cave-home-integrations` — the integration registry & lifecycle engine.
//!
//! This crate is the **backend behind "add a device or service"**: the part of
//! cave-home that knows *what kinds of things* can be connected (a Hue bridge, a
//! Zigbee coordinator, an MQTT broker, a camera…), keeps track of *each one a
//! household has actually added*, and drives every added thing through its
//! life: get it set up, keep it running, retry when the connection drops, let
//! the household remove it again.
//!
//! It mirrors the Home Assistant *integrations / config-entries* model
//! (Charter §3, "Integrations layer") but is **pure logic**: no network, no
//! async, no other cave-home crate. The discovery transports (mDNS / SSDP /
//! DHCP), the async setup execution, the config-flow wizard backend and the
//! ADR-004 orchestration hand-off all *feed* this engine and are deferred to
//! Phase 1b (see `parity.manifest.toml`).
//!
//! # The pieces
//!
//! - [`integration`] — the [`Integration`] descriptor: what a kind of thing is,
//!   what capabilities it provides, what it depends on, how it's discovered.
//! - [`config_entry`] — the [`ConfigEntry`]: one thing a household added, with
//!   its lifecycle [`State`].
//! - [`resolver`] — orders integrations so dependencies set up first
//!   (topological sort), and reports dependency cycles & missing dependencies.
//! - [`discovery`] — matches a [`Discovered`] signal to the integrations that
//!   can handle it, and tells "already added" apart from "new" so the household
//!   is only ever offered genuinely new devices.
//! - [`lifecycle`] — the pure setup → loaded → unload state machine, including
//!   the transient-vs-permanent failure classification that decides whether we
//!   keep trying.
//! - [`capability`] — what a single integration, and the whole hub, *can do*.
//! - [`label`] — grandma-friendly EN / DE / TR messages (Charter §6.3): no
//!   "config entry", no "platform", no protocol jargon ever reaches a person.
//!
//! # Example
//!
//! ```
//! use cave_home_integrations::{
//!     Integration, Registry, Discovered, Capability, IotClass, Lang,
//! };
//!
//! // The hub knows about a "living-room hub" kind of thing.
//! let hub = Integration::new("livingroom_hub", "Living-room hub")
//!     .with_capability(Capability::Light)
//!     .with_iot_class(IotClass::LocalPush)
//!     .discoverable_by("_hue._tcp");
//! let mut reg = Registry::new();
//! reg.register(hub);
//!
//! // Something is found on the network.
//! let found = Discovered::mdns("_hue._tcp").with_property("id", "ABC123");
//! let matches = reg.match_discovery(&found);
//! assert_eq!(matches, vec!["livingroom_hub"]);
//!
//! // The household is told in plain language — never "config entry".
//! let msg = cave_home_integrations::label::found_new(Capability::Light, Lang::En);
//! assert_eq!(msg, "Found a new light — add it?");
//! ```

pub mod capability;
pub mod config_entry;
pub mod discovery;
pub mod integration;
pub mod label;
pub mod lifecycle;
pub mod resolver;

pub use capability::{Capability, HubCapabilities};
pub use config_entry::{ConfigEntry, DisabledBy};
pub use discovery::{Discovered, Transport};
pub use integration::{ConfigFlow, Integration, IotClass, Registry};
pub use label::Lang;
pub use lifecycle::{Failure, State, Transition, TransitionError};
pub use resolver::{resolve_setup_order, ResolveError};
