//! Port of the `homeassistant.core.HomeAssistant` god-object.
//!
//! Reduced to the handles every integration is given: the event bus, the state
//! machine, the service registry, and the entity/device/area registries.
//!
//! HA threads a single `hass` instance through `async_setup_entry`, every
//! entity platform, and every helper. [`CoreContext`] is that bundle. All its
//! fields are cheap `Arc`-backed handles, so a `CoreContext` is `Clone` and
//! every clone shares the same underlying state — exactly how `hass` behaves.

use crate::area_registry::AreaRegistry;
use crate::device_registry::DeviceRegistry;
use crate::entity_registry::EntityRegistry;
use crate::event_bus::EventBus;
use crate::service::ServiceRegistry;
use crate::state_machine::StateMachine;

/// The shared core handles passed to every integration (HA's `hass`).
#[derive(Clone)]
pub struct CoreContext {
    pub bus: EventBus,
    pub states: StateMachine,
    pub services: ServiceRegistry,
    pub entities: EntityRegistry,
    pub devices: DeviceRegistry,
    pub areas: AreaRegistry,
}

impl CoreContext {
    /// Build a fresh core: the state machine and service registry are wired to
    /// the same event bus, so state changes and service (un)registrations are
    /// observable on `bus`.
    #[must_use]
    pub fn new() -> Self {
        let bus = EventBus::new();
        let states = StateMachine::new(bus.clone());
        let services = ServiceRegistry::new(bus.clone());
        Self {
            bus,
            states,
            services,
            entities: EntityRegistry::new(),
            devices: DeviceRegistry::new(),
            areas: AreaRegistry::new(),
        }
    }
}

impl Default for CoreContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;
    use crate::state::{EntityId, StateAttributes};
    use crate::state_machine::EVENT_STATE_CHANGED;

    #[tokio::test]
    async fn state_changes_are_observable_on_the_shared_bus() {
        let ctx = CoreContext::new();
        let (_id, mut rx) = ctx.bus.listen(EVENT_STATE_CHANGED);
        ctx.states.set(
            EntityId::new("light", "kitchen").expect("id"),
            "on",
            StateAttributes::new(),
            Context::new(),
        );
        let evt = rx.recv().await.expect("state_changed");
        assert_eq!(evt.data["entity_id"], "light.kitchen");
    }

    #[test]
    fn clones_share_underlying_state() {
        let ctx = CoreContext::new();
        let clone = ctx.clone();
        ctx.states.set(
            EntityId::new("lock", "front").expect("id"),
            "locked",
            StateAttributes::new(),
            Context::new(),
        );
        // a clone sees the same state machine contents
        assert!(clone.states.is_state(&EntityId::new("lock", "front").expect("id"), "locked"));
        // and the same area registry
        let area = ctx.areas.create("Garage").expect("area");
        assert_eq!(clone.areas.get(&area.id).map(|a| a.name), Some("Garage".into()));
    }
}
