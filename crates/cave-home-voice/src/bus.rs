// SPDX-License-Identifier: Apache-2.0
//! In-process voice message bus.
//!
//! # Upstream:
//! - `OpenVoiceOS/ovos-bus-client@a8f12bd:ovos_bus_client/client/client.py::MessageBusClient`
//!   — Python bus client publishes `Message(type, data, context)`
//!   envelopes over a websocket. The cave-home in-process bus uses
//!   the same envelope shape; the network transport will be a Phase
//!   1b concern (it will hook into `cave-home-automation::EventBus`).
//! - `OpenVoiceOS/ovos-bus-client@a8f12bd:ovos_bus_client/message.py::Message`
//!   — same fields (`type`, `data`, `context`).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::error::{VoiceError, VoiceResult};

/// Capacity of the broadcast channel. Mirrors `MessageBusClient`'s
/// in-flight backlog tolerance.
const DEFAULT_BUS_CAPACITY: usize = 256;

/// Voice-bus message envelope.
///
/// # Upstream:
/// `OpenVoiceOS/ovos-bus-client@a8f12bd:ovos_bus_client/message.py::Message`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceMessage {
    /// Dotted topic name (e.g. `voice.wake.detected`,
    /// `voice.stt.transcribed`, `voice.intent.matched`).
    pub topic: String,
    /// JSON payload (kept as `serde_json::Value` for parity with the
    /// upstream untyped dict).
    pub data: serde_json::Value,
    /// Context dict — caller-tracking metadata (session id, lang).
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

impl VoiceMessage {
    /// Construct a message with an empty context map.
    #[must_use]
    pub fn new<T: Into<String>>(topic: T, data: serde_json::Value) -> Self {
        Self {
            topic: topic.into(),
            data,
            context: HashMap::new(),
        }
    }

    /// Builder helper — set a single context entry.
    #[must_use]
    pub fn with_context<K: Into<String>>(mut self, key: K, value: serde_json::Value) -> Self {
        self.context.insert(key.into(), value);
        self
    }
}

/// Subscriber handle returned by [`VoiceBus::subscribe`].
pub type VoiceBusSubscription = broadcast::Receiver<VoiceMessage>;

/// In-process voice bus.
///
/// `tokio::sync::broadcast` mirrors OVOS's websocket fan-out: every
/// subscriber gets every message; lagging subscribers see
/// `RecvError::Lagged` (parity with `MessageBusClient`'s "client missed
/// frames" log).
#[derive(Debug, Clone)]
pub struct VoiceBus {
    tx: broadcast::Sender<VoiceMessage>,
    history: Arc<Mutex<Vec<VoiceMessage>>>,
    history_cap: usize,
}

impl VoiceBus {
    /// Build a bus with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BUS_CAPACITY)
    }

    /// Build a bus with an explicit broadcast capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            history: Arc::new(Mutex::new(Vec::with_capacity(capacity))),
            history_cap: capacity,
        }
    }

    /// Publish a message to every subscriber.
    ///
    /// # Errors
    /// Returns [`VoiceError::Bus`] when no subscribers are active. OVOS
    /// silently drops in this case; cave-home surfaces it so the
    /// pipeline can decide.
    pub fn publish(&self, message: VoiceMessage) -> VoiceResult<usize> {
        {
            let mut hist = self.history.lock();
            hist.push(message.clone());
            let cap = self.history_cap;
            if hist.len() > cap {
                let drop = hist.len() - cap;
                hist.drain(..drop);
            }
        }
        self.tx
            .send(message)
            .map_err(|e| VoiceError::Bus(format!("no live subscribers: {e}")))
    }

    /// Publish, ignoring "no subscribers" errors. Convenient for
    /// best-effort telemetry events.
    pub fn publish_best_effort(&self, message: VoiceMessage) {
        let _ = self.publish(message);
    }

    /// Subscribe a new listener.
    #[must_use]
    pub fn subscribe(&self) -> VoiceBusSubscription {
        self.tx.subscribe()
    }

    /// Snapshot of recent message history (capped, for debug pages).
    #[must_use]
    pub fn history(&self) -> Vec<VoiceMessage> {
        self.history.lock().clone()
    }
}

impl Default for VoiceBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Sink trait for binary integration with the cave-home automation
/// engine. `cave-home-automation::EventBus` will implement this in
/// Phase 1b, but the voice crate itself defers to [`VoiceBus`] today.
pub trait VoiceEventSink: Send + Sync {
    fn deliver(&self, message: &VoiceMessage);
}

impl VoiceEventSink for VoiceBus {
    fn deliver(&self, message: &VoiceMessage) {
        self.publish_best_effort(message.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn message_round_trips_through_bus() {
        let bus = VoiceBus::new();
        let mut sub = bus.subscribe();
        let msg = VoiceMessage::new(
            "voice.wake.detected",
            serde_json::json!({ "score": 0.93 }),
        );
        bus.publish(msg.clone()).expect("publish");
        let received = sub.recv().await.expect("recv");
        assert_eq!(received.topic, "voice.wake.detected");
    }

    #[tokio::test]
    async fn history_caps_at_capacity() {
        let bus = VoiceBus::with_capacity(2);
        let _sub = bus.subscribe(); // keep at least one subscriber alive
        for i in 0..5 {
            bus.publish_best_effort(VoiceMessage::new(
                format!("voice.test.{i}"),
                serde_json::json!({}),
            ));
        }
        let hist = bus.history();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].topic, "voice.test.3");
        assert_eq!(hist[1].topic, "voice.test.4");
    }

    #[test]
    fn publish_without_subscribers_yields_bus_error() {
        let bus = VoiceBus::new();
        let r = bus.publish(VoiceMessage::new(
            "voice.intent.matched",
            serde_json::json!({"intent": "lights_off"}),
        ));
        assert!(matches!(r, Err(VoiceError::Bus(_))));
    }

    #[test]
    fn with_context_attaches_session_metadata() {
        let m = VoiceMessage::new("voice.session.started", serde_json::json!({}))
            .with_context("session", serde_json::json!("abc-123"))
            .with_context("lang", serde_json::json!("tr"));
        assert_eq!(m.context["lang"], serde_json::json!("tr"));
        assert_eq!(m.context["session"], serde_json::json!("abc-123"));
    }
}
