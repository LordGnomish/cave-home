//! cave-home-core — line-by-line port skeleton of home-assistant/core (Apache-2.0).
//!
//! The four primitives at the bottom of HA core are: `State`, `Context`,
//! `Event`, and the `EventBus` they flow through. The `StateMachine` is the
//! authoritative store of every entity's current state and is fed by, and
//! emits into, the event bus. This module ports those four into Rust with
//! the same names and broadly the same shape so subsequent ports
//! (helpers, services, automations) can land against a recognisable surface.

#![doc(html_root_url = "https://docs.rs/cave-home-core")]

pub mod context;
pub mod event;
pub mod event_bus;
pub mod service;
pub mod state;
pub mod state_machine;

pub use context::Context;
pub use event::{Event, EventOrigin};
pub use event_bus::{EventBus, Listener};
pub use service::{Service, ServiceCall, ServiceError, ServiceRegistry};
pub use state::{EntityId, State, StateAttributes};
pub use state_machine::{StateChange, StateMachine, StateMachineError};
