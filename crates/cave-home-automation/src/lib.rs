// SPDX-License-Identifier: Apache-2.0
//! cave-home-automation — the heart of cave-home.
//!
//! This crate is a line-by-line port of the
//! [home-assistant/core](https://github.com/home-assistant/core) Python
//! codebase (Apache-2.0), pinned to tag `2026.5.2`
//! (commit `456202325ac48549bd3c895dc3e69ecd3e2ba6a4`) — see
//! `parity.manifest.toml` for the canonical mapping table.
//!
//! Phase 1 MVP scope (Charter golden rule 7, honest fill ratio 0.42 vs
//! the MVP slice):
//!
//! - [`state`] — `State`, `States`, `StateMachine`.
//! - [`event_bus`] — `Event`, `EventOrigin`, `EventBus` trait,
//!   `InMemoryEventBus`. The trait is the **inter-crate substrate**;
//!   H3 (matter), H4 (zwave), H5 (camera), H6 (voice) all emit events
//!   into the same bus.
//! - [`service`] — `Service`, `ServiceCall`, `ServiceRegistry`,
//!   `SupportsResponse`.
//! - [`automation`] — triggers, conditions, actions, engine.
//! - [`script`] — sequence runner (delay / choose / repeat / if /
//!   wait_for_trigger / service / scene).
//! - [`scene`] — snapshot + activate.
//! - [`template`] — minijinja-based template environment with HA's
//!   custom filters and functions (`is_state`, `states`, `state_attr`,
//!   `now`, `today_at`, `as_timestamp`).
//! - [`config_entry`] — config-entry and flow-handler abstractions.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod automation;
pub mod config_entry;
pub mod context;
pub mod error;
pub mod event_bus;
pub mod scene;
pub mod script;
pub mod service;
pub mod state;
pub mod template;

pub mod prelude {
    //! Convenience re-exports for downstream cave-home crates.
    pub use crate::automation::{Action, Automation, AutomationEngine, Condition, Trigger};
    pub use crate::context::Context;
    pub use crate::error::HassError;
    pub use crate::event_bus::{
        Event, EventBus, EventOrigin, InMemoryEventBus, ListenerHandle,
    };
    pub use crate::scene::Scene;
    pub use crate::script::Script;
    pub use crate::service::{Service, ServiceCall, ServiceRegistry, SupportsResponse};
    pub use crate::state::{State, StateMachine};
    pub use crate::template::Template;
}

pub use crate::context::Context;
pub use crate::error::{HassError, HassResult};
pub use crate::event_bus::{Event, EventBus, EventOrigin, InMemoryEventBus, ListenerHandle};
pub use crate::scene::Scene;
pub use crate::script::Script;
pub use crate::service::{Service, ServiceCall, ServiceRegistry, SupportsResponse};
pub use crate::state::{State, StateMachine};
pub use crate::template::Template;
