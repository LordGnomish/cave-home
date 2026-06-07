//! Helper registries layered over the core primitives — ports of the
//! `homeassistant.components.{zone,person}` and
//! `homeassistant.components.homeassistant.scene` helpers that the frontend and
//! automations lean on.

pub mod person;
pub mod scene;
pub mod zone;

pub use person::{Person, PersonRegistry};
pub use scene::{Scene, SceneEntityState, SceneRegistry};
pub use zone::{Zone, ZoneRegistry};
