//! Port of `homeassistant.core.EventBus`.
//!
//! Two listener flavours: `listen(event_type)` keeps firing until
//! cancelled; `listen_once` fires exactly once. The wildcard
//! `MATCH_ALL` token routes every event to the listener.

use crate::event::Event;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub const MATCH_ALL: &str = "*";

pub type ListenerId = u64;

#[derive(Clone)]
pub struct Listener {
    pub id: ListenerId,
    pub event_type: String,
    once: bool,
    tx: UnboundedSender<Event>,
}

#[derive(Default)]
struct Inner {
    listeners: HashMap<String, Vec<Listener>>,
    next_id: AtomicU64,
}

#[derive(Clone, Default)]
pub struct EventBus {
    inner: Arc<RwLock<Inner>>,
}

impl EventBus {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn listen(&self, event_type: impl Into<String>) -> (ListenerId, UnboundedReceiver<Event>) {
        self.subscribe(event_type, false)
    }

    pub fn listen_once(&self, event_type: impl Into<String>) -> (ListenerId, UnboundedReceiver<Event>) {
        self.subscribe(event_type, true)
    }

    fn subscribe(&self, event_type: impl Into<String>, once: bool) -> (ListenerId, UnboundedReceiver<Event>) {
        let (tx, rx) = unbounded_channel();
        let mut guard = self.inner.write();
        let id = guard.next_id.fetch_add(1, Ordering::Relaxed);
        let key = event_type.into();
        guard.listeners.entry(key.clone()).or_default().push(Listener {
            id,
            event_type: key,
            once,
            tx,
        });
        (id, rx)
    }

    pub fn fire(&self, event: Event) {
        let drained = {
            let mut guard = self.inner.write();
            let mut drained: Vec<Listener> = Vec::new();
            for key in [event.event_type.as_str(), MATCH_ALL] {
                if let Some(bucket) = guard.listeners.get_mut(key) {
                    let mut still: Vec<Listener> = Vec::with_capacity(bucket.len());
                    for l in bucket.drain(..) {
                        if l.once {
                            drained.push(l);
                        } else {
                            drained.push(l.clone());
                            still.push(l);
                        }
                    }
                    *bucket = still;
                }
            }
            drained
        };
        for listener in drained {
            let _ = listener.tx.send(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn listener_and_wildcard_routing() {
        let bus = EventBus::new();
        let (_id, mut rx) = bus.listen("state_changed");
        let (_id2, mut rx_all) = bus.listen(MATCH_ALL);
        bus.fire(Event::local("state_changed", json!({"x": 1})));
        bus.fire(Event::local("other", json!({})));
        assert_eq!(rx.recv().await.expect("e1").event_type, "state_changed");
        assert_eq!(rx_all.recv().await.expect("a1").event_type, "state_changed");
        assert_eq!(rx_all.recv().await.expect("a2").event_type, "other");
    }

    #[tokio::test]
    async fn listen_once_fires_then_drops() {
        let bus = EventBus::new();
        let (_id, mut rx) = bus.listen_once("ping");
        bus.fire(Event::local("ping", json!({})));
        bus.fire(Event::local("ping", json!({})));
        assert!(rx.recv().await.is_some());
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn wildcard_listen_once_consumes_first_event_only() {
        let bus = EventBus::new();
        let (_id, mut rx) = bus.listen_once(MATCH_ALL);
        bus.fire(Event::local("first", json!({})));
        bus.fire(Event::local("second", json!({})));
        assert_eq!(rx.recv().await.expect("first").event_type, "first");
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn fire_with_no_listeners_and_dropped_receiver_is_silent() {
        let bus = EventBus::new();
        // no listeners at all — fire must not panic
        bus.fire(Event::local("orphan", json!({})));

        // a dropped receiver must not break later delivery to others
        let (_dead_id, dead_rx) = bus.listen("topic");
        drop(dead_rx);
        let (_live_id, mut live_rx) = bus.listen("topic");
        bus.fire(Event::local("topic", json!({"n": 1})));
        assert_eq!(live_rx.recv().await.expect("live").data["n"], 1);
    }

    #[tokio::test]
    async fn distinct_event_types_are_isolated() {
        let bus = EventBus::new();
        let (_id, mut rx) = bus.listen("only_this");
        bus.fire(Event::local("not_this", json!({})));
        bus.fire(Event::local("only_this", json!({"ok": true})));
        // the first matching event is the one we asked for
        let evt = rx.recv().await.expect("matched");
        assert_eq!(evt.event_type, "only_this");
        assert_eq!(evt.data["ok"], true);
    }
}
