// SPDX-License-Identifier: Apache-2.0
//! Event bus — port of `homeassistant/core.py::EventBus`.
//!
//! This module defines:
//!
//! - [`Event`] / [`EventOrigin`] — port of HA's `Event` / `EventOrigin`.
//! - [`EventBus`] **trait** — the inter-crate substrate. H3 (matter),
//!   H4 (zwave), H5 (camera), H6 (voice) all consume this trait by
//!   importing `cave_home_automation::EventBus` and emitting their
//!   protocol-specific events into the shared bus.
//! - [`InMemoryEventBus`] — the default in-process implementation
//!   used by [`crate::state::StateMachine`] /
//!   [`crate::service::ServiceRegistry`] / the automation engine.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventBus

use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::context::Context;

// ---- canonical event type strings -----------------------------------------

/// Fired when state of an entity is added, removed, or changes value.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::EVENT_STATE_CHANGED
pub const EVENT_STATE_CHANGED: &str = "state_changed";

/// Fired when the state is re-reported without value/attribute change.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::EVENT_STATE_REPORTED
pub const EVENT_STATE_REPORTED: &str = "state_reported";

/// Fired when a service is called.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::EVENT_CALL_SERVICE
pub const EVENT_CALL_SERVICE: &str = "call_service";

/// Fired when a service is registered.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::EVENT_SERVICE_REGISTERED
pub const EVENT_SERVICE_REGISTERED: &str = "service_registered";

/// Fired when a service is removed.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::EVENT_SERVICE_REMOVED
pub const EVENT_SERVICE_REMOVED: &str = "service_removed";

/// Fired on startup completion.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::EVENT_HOMEASSISTANT_START
pub const EVENT_HASS_START: &str = "homeassistant_start";

/// Fired on shutdown.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::EVENT_HOMEASSISTANT_STOP
pub const EVENT_HASS_STOP: &str = "homeassistant_stop";

/// Wildcard event type — matches any event.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::MATCH_ALL
pub const MATCH_ALL: &str = "*";

/// Per HA `_verify_event_type_length_or_raise`, event type strings are
/// capped at 64 characters.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::MAX_LENGTH_EVENT_EVENT_TYPE
pub const MAX_LENGTH_EVENT_EVENT_TYPE: usize = 64;

/// Origin of a fired event.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventOrigin
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EventOrigin {
    Local,
    Remote,
}

impl Default for EventOrigin {
    fn default() -> Self {
        Self::Local
    }
}

/// Concrete event delivered to listeners.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_type: String,
    pub data: Value,
    pub origin: EventOrigin,
    pub time_fired: OffsetDateTime,
    pub context: Context,
}

impl Event {
    /// New event.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Event.__init__
    #[must_use]
    pub fn new(
        event_type: String,
        data: Value,
        origin: EventOrigin,
        context: Context,
    ) -> Self {
        Self {
            event_type,
            data,
            origin,
            time_fired: OffsetDateTime::now_utc(),
            context,
        }
    }

    /// JSON-ish dict representation.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::Event.as_dict
    #[must_use]
    pub fn as_dict(&self) -> Value {
        serde_json::json!({
            "event_type": self.event_type,
            "data": self.data,
            "origin": self.origin,
            "time_fired": self.time_fired.unix_timestamp(),
            "context": self.context,
        })
    }
}

/// Callback invoked when an event fires.
pub type ListenerFn = Arc<dyn Fn(&Event) + Send + Sync + 'static>;

/// Optional event filter — returns `true` if the listener should run.
pub type FilterFn = Arc<dyn Fn(&Event) -> bool + Send + Sync + 'static>;

/// Handle returned from [`EventBus::async_listen`]; dropping it removes
/// the listener.
///
/// Mirrors HA core's `CALLBACK_TYPE` (a closure that unsubscribes).
#[must_use = "dropping the handle unsubscribes the listener"]
pub struct ListenerHandle {
    remover: Option<Box<dyn FnOnce() + Send + Sync + 'static>>,
}

impl ListenerHandle {
    pub fn new<F>(remover: F) -> Self
    where
        F: FnOnce() + Send + Sync + 'static,
    {
        Self {
            remover: Some(Box::new(remover)),
        }
    }

    /// Explicit unsubscribe.
    pub fn remove(mut self) {
        if let Some(r) = self.remover.take() {
            r();
        }
    }
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        if let Some(r) = self.remover.take() {
            r();
        }
    }
}

impl std::fmt::Debug for ListenerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenerHandle")
            .field("active", &self.remover.is_some())
            .finish()
    }
}

/// **THE cross-crate event bus trait.** Used by every cave-home crate
/// that needs to publish or subscribe to home-state events.
///
/// Stability promise: the four required methods — [`fire`](Self::fire),
/// [`async_listen`](Self::async_listen), [`async_listen_filtered`](Self::async_listen_filtered),
/// and [`async_listeners`](Self::async_listeners) — are part of the
/// public API and won't break without a major-version bump.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventBus
pub trait EventBus: Send + Sync + std::fmt::Debug {
    /// Fire an event. Returns immediately; listeners run synchronously
    /// on the firing thread (matching HA core's `async_fire_internal`).
    fn fire(&self, event: Event);

    /// Convenience: fire an event built from parts.
    fn fire_parts(
        &self,
        event_type: &str,
        data: Value,
        origin: EventOrigin,
        context: Context,
    ) {
        self.fire(Event::new(event_type.to_owned(), data, origin, context));
    }

    /// Subscribe to events of the given type — or to ALL events when
    /// `event_type == MATCH_ALL`.
    ///
    /// Returns a [`ListenerHandle`] whose `Drop` impl removes the
    /// listener.
    fn async_listen(
        &self,
        event_type: &str,
        listener: impl Fn(&Event) + Send + Sync + 'static,
    ) -> ListenerHandle
    where
        Self: Sized,
    {
        self.async_listen_dyn(event_type, Arc::new(listener))
    }

    /// Type-erased subscribe used by [`Self::async_listen`] and
    /// downstream dynamic-dispatch consumers.
    fn async_listen_dyn(&self, event_type: &str, listener: ListenerFn) -> ListenerHandle;

    /// Subscribe with a `filter` that gates each event.
    fn async_listen_filtered(
        &self,
        event_type: &str,
        listener: ListenerFn,
        filter: FilterFn,
    ) -> ListenerHandle;

    /// Subscribe for exactly one event, then auto-unsubscribe.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventBus.async_listen_once
    fn async_listen_once_dyn(
        &self,
        event_type: &str,
        listener: ListenerFn,
    ) -> ListenerHandle;

    /// Subscribe for exactly one event (generic helper).
    fn async_listen_once(
        &self,
        event_type: &str,
        listener: impl Fn(&Event) + Send + Sync + 'static,
    ) -> ListenerHandle
    where
        Self: Sized,
    {
        self.async_listen_once_dyn(event_type, Arc::new(listener))
    }

    /// Snapshot of `event_type -> listener_count`.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventBus.async_listeners
    fn async_listeners(&self) -> Vec<(String, usize)>;
}

// ---- in-memory implementation ---------------------------------------------

#[derive(Clone)]
struct Listener {
    id: u64,
    cb: ListenerFn,
    filter: Option<FilterFn>,
}

#[derive(Default)]
struct BusInner {
    listeners: std::collections::HashMap<String, Vec<Listener>>,
    next_id: u64,
}

/// In-process synchronous event bus. The default implementation of
/// [`EventBus`] used by all four built-in subsystems.
pub struct InMemoryEventBus {
    inner: Arc<Mutex<BusInner>>,
}

impl std::fmt::Debug for InMemoryEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("InMemoryEventBus")
            .field(
                "listeners",
                &inner
                    .listeners
                    .iter()
                    .map(|(k, v)| (k.clone(), v.len()))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl InMemoryEventBus {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BusInner::default())),
        }
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}

fn verify_event_type(event_type: &str) {
    debug_assert!(
        event_type.len() <= MAX_LENGTH_EVENT_EVENT_TYPE,
        "event type exceeds {MAX_LENGTH_EVENT_EVENT_TYPE} chars: {event_type}"
    );
}

impl EventBus for InMemoryEventBus {
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventBus.async_fire_internal
    fn fire(&self, event: Event) {
        verify_event_type(&event.event_type);
        let listeners = {
            let inner = self.inner.lock();
            let mut combined: Vec<Listener> = Vec::new();
            if let Some(ls) = inner.listeners.get(&event.event_type) {
                combined.extend(ls.iter().cloned());
            }
            // Match-all listeners — but NOT for state_reported (HA's
            // EVENTS_EXCLUDED_FROM_MATCH_ALL).
            if event.event_type != EVENT_STATE_REPORTED {
                if let Some(ls) = inner.listeners.get(MATCH_ALL) {
                    combined.extend(ls.iter().cloned());
                }
            }
            combined
        };
        for listener in listeners {
            if let Some(filter) = listener.filter.as_ref() {
                if !filter(&event) {
                    continue;
                }
            }
            (listener.cb)(&event);
        }
    }

    fn async_listen_dyn(&self, event_type: &str, listener: ListenerFn) -> ListenerHandle {
        verify_event_type(event_type);
        let inner = self.inner.clone();
        let event_type_owned = event_type.to_owned();
        let id = {
            let mut g = inner.lock();
            g.next_id += 1;
            let id = g.next_id;
            g.listeners.entry(event_type_owned.clone()).or_default().push(Listener {
                id,
                cb: listener,
                filter: None,
            });
            id
        };
        let inner_for_drop = inner.clone();
        ListenerHandle::new(move || {
            let mut g = inner_for_drop.lock();
            if let Some(vec) = g.listeners.get_mut(&event_type_owned) {
                vec.retain(|l| l.id != id);
                if vec.is_empty() && event_type_owned != MATCH_ALL {
                    g.listeners.remove(&event_type_owned);
                }
            }
        })
    }

    fn async_listen_filtered(
        &self,
        event_type: &str,
        listener: ListenerFn,
        filter: FilterFn,
    ) -> ListenerHandle {
        verify_event_type(event_type);
        let inner = self.inner.clone();
        let event_type_owned = event_type.to_owned();
        let id = {
            let mut g = inner.lock();
            g.next_id += 1;
            let id = g.next_id;
            g.listeners.entry(event_type_owned.clone()).or_default().push(Listener {
                id,
                cb: listener,
                filter: Some(filter),
            });
            id
        };
        let inner_for_drop = inner.clone();
        ListenerHandle::new(move || {
            let mut g = inner_for_drop.lock();
            if let Some(vec) = g.listeners.get_mut(&event_type_owned) {
                vec.retain(|l| l.id != id);
                if vec.is_empty() && event_type_owned != MATCH_ALL {
                    g.listeners.remove(&event_type_owned);
                }
            }
        })
    }

    fn async_listen_once_dyn(
        &self,
        event_type: &str,
        listener: ListenerFn,
    ) -> ListenerHandle {
        verify_event_type(event_type);
        let inner = self.inner.clone();
        let event_type_owned = event_type.to_owned();
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_for_cb = fired.clone();
        let inner_for_cb = inner.clone();
        let event_type_for_cb = event_type_owned.clone();

        let id = {
            let mut g = inner.lock();
            g.next_id += 1;
            let id = g.next_id;
            let user_cb = listener;
            let wrapper: ListenerFn = Arc::new(move |evt: &Event| {
                if fired_for_cb.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                user_cb(evt);
                // Self-remove after firing.
                let mut g = inner_for_cb.lock();
                if let Some(vec) = g.listeners.get_mut(&event_type_for_cb) {
                    vec.retain(|l| l.id != id);
                    if vec.is_empty() && event_type_for_cb != MATCH_ALL {
                        g.listeners.remove(&event_type_for_cb);
                    }
                }
            });
            g.listeners.entry(event_type_owned.clone()).or_default().push(Listener {
                id,
                cb: wrapper,
                filter: None,
            });
            id
        };

        let inner_for_drop = inner;
        let fired_for_drop = fired;
        ListenerHandle::new(move || {
            if fired_for_drop.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let mut g = inner_for_drop.lock();
            if let Some(vec) = g.listeners.get_mut(&event_type_owned) {
                vec.retain(|l| l.id != id);
                if vec.is_empty() && event_type_owned != MATCH_ALL {
                    g.listeners.remove(&event_type_owned);
                }
            }
        })
    }

    fn async_listeners(&self) -> Vec<(String, usize)> {
        let g = self.inner.lock();
        g.listeners
            .iter()
            .map(|(k, v)| (k.clone(), v.len()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fire_simple(bus: &InMemoryEventBus, t: &str) {
        bus.fire_parts(
            t,
            Value::Null,
            EventOrigin::Local,
            Context::new(),
        );
    }

    /// Upstream: `tests/test_core.py::test_async_fire`
    #[test]
    fn async_fire_delivers_to_listener() {
        let bus = InMemoryEventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let _h = bus.async_listen("test_event", move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        fire_simple(&bus, "test_event");
        fire_simple(&bus, "test_event");
        fire_simple(&bus, "other_event");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    /// Upstream: `tests/test_core.py::test_async_listen_once`
    #[test]
    fn listen_once_fires_only_once() {
        let bus = InMemoryEventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let _h = bus.async_listen_once("boot", move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        fire_simple(&bus, "boot");
        fire_simple(&bus, "boot");
        fire_simple(&bus, "boot");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let listeners = bus.async_listeners();
        assert!(listeners.iter().all(|(k, _)| k != "boot"));
    }

    /// Upstream: `tests/test_core.py::test_async_listen_filter`
    #[test]
    fn listener_filter_short_circuits() {
        let bus = InMemoryEventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let cb: ListenerFn = Arc::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        let filter: FilterFn = Arc::new(|evt: &Event| {
            evt.data
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
        let _h = bus.async_listen_filtered("filtered_event", cb, filter);
        bus.fire_parts(
            "filtered_event",
            serde_json::json!({"ok": false}),
            EventOrigin::Local,
            Context::new(),
        );
        bus.fire_parts(
            "filtered_event",
            serde_json::json!({"ok": true}),
            EventOrigin::Local,
            Context::new(),
        );
        bus.fire_parts(
            "filtered_event",
            serde_json::json!({"ok": true}),
            EventOrigin::Local,
            Context::new(),
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn match_all_receives_every_non_reported_event() {
        let bus = InMemoryEventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let _h = bus.async_listen(MATCH_ALL, move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        fire_simple(&bus, "a");
        fire_simple(&bus, "b");
        // state_reported is intentionally excluded from match-all.
        fire_simple(&bus, EVENT_STATE_REPORTED);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn drop_handle_removes_listener() {
        let bus = InMemoryEventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let h = bus.async_listen("x", move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        fire_simple(&bus, "x");
        drop(h);
        fire_simple(&bus, "x");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn explicit_remove_unsubscribes() {
        let bus = InMemoryEventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let h = bus.async_listen("y", move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        h.remove();
        fire_simple(&bus, "y");
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
}
