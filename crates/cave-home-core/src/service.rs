//! Port of the *registry* portion of `homeassistant.core.ServiceRegistry`
//! (plus `ServiceCall` and the `Service` descriptor).
//!
//! HA's `ServiceRegistry` is a `domain -> {service -> Service}` map. Two
//! halves live in upstream:
//!
//!   * the **registry** — `register` / `remove` / `has_service` /
//!     `services()`, and the `EVENT_SERVICE_REGISTERED` /
//!     `EVENT_SERVICE_REMOVED` notifications fired on the bus when the map
//!     mutates. This is pure bookkeeping over a map and is ported here in
//!     full.
//!   * the **execution** — `async_call`, which schedules the registered
//!     coroutine/job on the event loop, applies the target/`entity_id`
//!     expansion, honours `blocking`/`return_response`, and threads the
//!     `Context`. That half is bound to the `HomeAssistant` async runtime
//!     and is deferred (see parity.manifest.toml). The data model it would
//!     consume — `ServiceCall` — is defined here so the execution layer can
//!     land against a recognisable surface.
//!
//! Service names use the same lowercase-`snake` grammar as the object-id
//! half of an entity id, so we reuse [`crate::state::EntityId`]'s validator
//! via [`is_valid_slug`].

use crate::context::Context;
use crate::event::{Event, EventOrigin};
use crate::event_bus::EventBus;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Fired on the bus when a service is registered (`homeassistant.const`).
pub const EVENT_SERVICE_REGISTERED: &str = "service_registered";
/// Fired on the bus when a service is removed (`homeassistant.const`).
pub const EVENT_SERVICE_REMOVED: &str = "service_removed";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServiceError {
    #[error("service name must be a lowercase snake slug: {0:?}")]
    InvalidName(String),
}

/// True if `s` is a non-empty `[a-z0-9_]+` slug — the grammar HA uses for
/// both a domain and a service name.
#[must_use]
pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Descriptor for a registered service — the metadata half of upstream's
/// `Service` object. The callable/job is intentionally omitted (it belongs
/// to the deferred execution layer); what remains is what the registry and
/// the websocket/`describe_services` API expose.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    /// Whether the service accepts a `target:` selector (entities/devices/
    /// areas). Mirrors upstream `Service.supports_response`-adjacent metadata.
    pub supports_target: bool,
}

impl Service {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_target(mut self, supports_target: bool) -> Self {
        self.supports_target = supports_target;
        self
    }
}

/// Port of `homeassistant.core.ServiceCall`.
///
/// The immutable record handed to a service handler: which `domain.service`
/// to run, the validated call `data`, and the [`Context`] threading
/// causality from whatever originated the call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServiceCall {
    pub domain: String,
    pub service: String,
    pub data: serde_json::Value,
    pub context: Context,
}

impl ServiceCall {
    /// Build a call, validating that `domain` and `service` are well-formed
    /// slugs (upstream rejects malformed names before dispatch).
    pub fn new(
        domain: impl Into<String>,
        service: impl Into<String>,
        data: serde_json::Value,
        context: Context,
    ) -> Result<Self, ServiceError> {
        let domain = domain.into();
        let service = service.into();
        if !is_valid_slug(&domain) {
            return Err(ServiceError::InvalidName(domain));
        }
        if !is_valid_slug(&service) {
            return Err(ServiceError::InvalidName(service));
        }
        Ok(Self {
            domain,
            service,
            data,
            context,
        })
    }
}

/// Port of the registry half of `homeassistant.core.ServiceRegistry`.
///
/// Cloneable handle over a shared `domain -> {service -> Service}` map. On
/// register/remove it fires `EVENT_SERVICE_REGISTERED` /
/// `EVENT_SERVICE_REMOVED` on the supplied [`EventBus`], matching upstream's
/// notification contract.
#[derive(Clone, Default)]
pub struct ServiceRegistry {
    inner: std::sync::Arc<parking_lot::RwLock<BTreeMap<String, BTreeMap<String, Service>>>>,
    bus: EventBus,
}

impl ServiceRegistry {
    #[must_use]
    pub fn new(bus: EventBus) -> Self {
        Self {
            inner: std::sync::Arc::new(parking_lot::RwLock::new(BTreeMap::new())),
            bus,
        }
    }

    /// Register `domain.service`. Re-registering an existing pair overwrites
    /// its descriptor (upstream behaviour) but still fires the event.
    /// Returns the previous descriptor if one was replaced.
    pub fn register(
        &self,
        domain: impl Into<String>,
        service: impl Into<String>,
        descriptor: Service,
    ) -> Result<Option<Service>, ServiceError> {
        let domain = domain.into();
        let service = service.into();
        if !is_valid_slug(&domain) {
            return Err(ServiceError::InvalidName(domain));
        }
        if !is_valid_slug(&service) {
            return Err(ServiceError::InvalidName(service));
        }
        let previous = {
            let mut guard = self.inner.write();
            guard
                .entry(domain.clone())
                .or_default()
                .insert(service.clone(), descriptor)
        };
        self.bus.fire(Event::new(
            EVENT_SERVICE_REGISTERED,
            serde_json::json!({ "domain": domain, "service": service }),
            EventOrigin::Local,
            Context::new(),
        ));
        Ok(previous)
    }

    /// True if `domain.service` is registered.
    #[must_use]
    pub fn has_service(&self, domain: &str, service: &str) -> bool {
        self.inner
            .read()
            .get(domain)
            .is_some_and(|svcs| svcs.contains_key(service))
    }

    /// Remove `domain.service`. Returns the removed descriptor, or `None` if
    /// it was not registered (in which case no event fires — upstream only
    /// notifies on an actual removal). An emptied domain bucket is dropped.
    pub fn remove(&self, domain: &str, service: &str) -> Option<Service> {
        let removed = {
            let mut guard = self.inner.write();
            let removed = guard.get_mut(domain).and_then(|svcs| svcs.remove(service));
            if guard.get(domain).is_some_and(BTreeMap::is_empty) {
                guard.remove(domain);
            }
            removed
        };
        if removed.is_some() {
            self.bus.fire(Event::new(
                EVENT_SERVICE_REMOVED,
                serde_json::json!({ "domain": domain, "service": service }),
                EventOrigin::Local,
                Context::new(),
            ));
        }
        removed
    }

    /// Snapshot of the registry as `domain -> {service -> Service}`.
    /// Mirrors HA's `ServiceRegistry.async_services()`.
    #[must_use]
    pub fn services(&self) -> BTreeMap<String, BTreeMap<String, Service>> {
        self.inner.read().clone()
    }

    /// Every service name registered under `domain`, sorted.
    #[must_use]
    pub fn services_for_domain(&self, domain: &str) -> BTreeSet<String> {
        self.inner
            .read()
            .get(domain)
            .map(|svcs| svcs.keys().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn registry() -> (EventBus, ServiceRegistry) {
        let bus = EventBus::new();
        let reg = ServiceRegistry::new(bus.clone());
        (bus, reg)
    }

    #[test]
    fn slug_grammar_matches_entity_object_id() {
        assert!(is_valid_slug("turn_on"));
        assert!(is_valid_slug("set_temperature_2"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("Turn_On"));
        assert!(!is_valid_slug("turn-on"));
        assert!(!is_valid_slug("light.turn_on"));
    }

    #[test]
    fn service_call_rejects_malformed_names() {
        let ctx = Context::new();
        assert!(ServiceCall::new("light", "turn_on", json!({}), ctx.clone()).is_ok());
        assert_eq!(
            ServiceCall::new("Light", "turn_on", json!({}), ctx.clone()),
            Err(ServiceError::InvalidName("Light".into()))
        );
        assert_eq!(
            ServiceCall::new("light", "turn on", json!({}), ctx),
            Err(ServiceError::InvalidName("turn on".into()))
        );
    }

    #[test]
    fn service_call_threads_context_and_data() {
        let parent = Context::with_user("alice");
        let ctx = Context::child_of(&parent);
        let call = ServiceCall::new("light", "turn_on", json!({"brightness": 200}), ctx.clone())
            .expect("valid call");
        assert_eq!(call.domain, "light");
        assert_eq!(call.service, "turn_on");
        assert_eq!(call.data["brightness"], 200);
        assert_eq!(call.context.parent_id, ctx.parent_id);
        assert_eq!(call.context.user_id.as_deref(), Some("alice"));
    }

    #[test]
    fn register_has_remove_round_trip() {
        let (_bus, reg) = registry();
        assert!(!reg.has_service("light", "turn_on"));

        let prev = reg
            .register("light", "turn_on", Service::new().with_target(true))
            .expect("register");
        assert!(prev.is_none());
        assert!(reg.has_service("light", "turn_on"));
        assert!(!reg.has_service("light", "turn_off"));
        assert!(!reg.has_service("lock", "turn_on"));

        let removed = reg.remove("light", "turn_on").expect("removed descriptor");
        assert!(removed.supports_target);
        assert!(!reg.has_service("light", "turn_on"));
        // domain bucket is dropped once empty
        assert!(reg.services().get("light").is_none());
        // removing a missing service yields None
        assert!(reg.remove("light", "turn_on").is_none());
    }

    #[test]
    fn re_register_overwrites_and_returns_previous() {
        let (_bus, reg) = registry();
        reg.register("climate", "set_temperature", Service::new().with_target(false))
            .expect("first");
        let prev = reg
            .register("climate", "set_temperature", Service::new().with_target(true))
            .expect("second")
            .expect("previous descriptor");
        assert!(!prev.supports_target);
        assert!(reg.services_for_domain("climate").contains("set_temperature"));
    }

    #[test]
    fn register_rejects_malformed_names() {
        let (_bus, reg) = registry();
        assert_eq!(
            reg.register("Light", "turn_on", Service::new()),
            Err(ServiceError::InvalidName("Light".into()))
        );
        assert_eq!(
            reg.register("light", "TurnOn", Service::new()),
            Err(ServiceError::InvalidName("TurnOn".into()))
        );
        assert!(!reg.has_service("Light", "turn_on"));
    }

    #[test]
    fn services_snapshot_and_per_domain_listing() {
        let (_bus, reg) = registry();
        reg.register("light", "turn_on", Service::new()).expect("a");
        reg.register("light", "turn_off", Service::new()).expect("b");
        reg.register("lock", "lock", Service::new()).expect("c");

        let snap = reg.services();
        assert_eq!(snap["light"].len(), 2);
        assert_eq!(snap["lock"].len(), 1);

        let light_svcs = reg.services_for_domain("light");
        assert!(light_svcs.contains("turn_on"));
        assert!(light_svcs.contains("turn_off"));
        assert!(reg.services_for_domain("does_not_exist").is_empty());
    }

    #[tokio::test]
    async fn register_and_remove_fire_bus_events() {
        let (bus, reg) = registry();
        let (_id_reg, mut rx_reg) = bus.listen(EVENT_SERVICE_REGISTERED);
        let (_id_rem, mut rx_rem) = bus.listen(EVENT_SERVICE_REMOVED);

        reg.register("light", "turn_on", Service::new()).expect("register");
        let evt = rx_reg.recv().await.expect("registered event");
        assert_eq!(evt.data["domain"], "light");
        assert_eq!(evt.data["service"], "turn_on");

        reg.remove("light", "turn_on").expect("removed");
        let evt = rx_rem.recv().await.expect("removed event");
        assert_eq!(evt.data["domain"], "light");
        assert_eq!(evt.data["service"], "turn_on");

        // a no-op remove fires nothing
        assert!(reg.remove("light", "turn_on").is_none());
        assert!(rx_rem.try_recv().is_err());
    }
}
