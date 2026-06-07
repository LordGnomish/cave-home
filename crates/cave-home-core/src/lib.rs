//! cave-home-core — port of the home-assistant/core architectural foundation
//! (Apache-2.0).
//!
//! The bottom of HA core is the synchronous data spine: [`State`], [`Context`],
//! [`Event`] and the [`EventBus`] they flow through, with [`StateMachine`] the
//! authoritative store of every entity's current state and [`ServiceRegistry`]
//! the service catalogue.
//!
//! Layered over that spine, this crate ports the architecture every future
//! domain port lands against:
//!
//! * [`entity`] — the [`Entity`] trait + [`DeviceInfo`] / [`EntityCategory`].
//! * [`area_registry`] / [`device_registry`] / [`entity_registry`] — the three
//!   HA registries (areas, physical devices, `unique_id`→`entity_id`).
//! * [`template`] — the Jinja2 (minijinja) template engine with HA's
//!   state-access globals.
//! * [`automation`] — the trigger → condition → action chain.
//! * [`loader`] — the [`Integration`] plug-in seam + dependency-ordered setup,
//!   driven over [`CoreContext`] (the `hass` handle bundle).
//! * [`config`] — the voluptuous-style [`Schema`] validator + [`CoreConfig`].
//! * [`helpers`] — the zone / person / scene registries.
//!
//! Each module names its upstream source in its own docs; see
//! `parity.manifest.toml` and `HANDOFF-ha-core-foundation.md`.

#![doc(html_root_url = "https://docs.rs/cave-home-core")]

pub mod area_registry;
pub mod automation;
pub mod config;
pub mod context;
pub mod core_context;
pub mod device_registry;
pub mod entity;
pub mod entity_registry;
pub mod event;
pub mod event_bus;
pub mod helpers;
pub mod loader;
pub mod service;
pub mod state;
pub mod state_machine;
pub mod template;
pub mod util;

pub use area_registry::{AreaEntry, AreaRegistry};
pub use automation::{Action, AutomationEngine, AutomationRule, Condition, Trigger};
pub use config::{CoreConfig, Schema, UnitSystem};
pub use context::Context;
pub use core_context::CoreContext;
pub use device_registry::{DeviceEntry, DeviceRegistry};
pub use entity::{DeviceInfo, Entity, EntityCategory};
pub use entity_registry::{EntityRegistry, RegistryEntry};
pub use event::{Event, EventOrigin};
pub use event_bus::{EventBus, Listener};
pub use helpers::{Person, PersonRegistry, Scene, SceneRegistry, Zone, ZoneRegistry};
pub use loader::{Integration, IntegrationLoader, Manifest, SetupReport};
pub use template::TemplateEngine;
pub use service::{Service, ServiceCall, ServiceError, ServiceRegistry};
pub use state::{EntityId, State, StateAttributes};
pub use state_machine::{StateChange, StateMachine, StateMachineError};
