// SPDX-License-Identifier: Apache-2.0
//! Camera event sink — bridges detected motion / objects / recordings
//! into downstream consumers (`cave-home-automation::EventBus` once that
//! land lands; until then a recording sink for tests).
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/events/maintainer.py :: `EventMaintainer.run` (the Frigate
//! event loop drains a queue and publishes MQTT + database rows; we
//! abstract behind a trait so the binary wires whichever sink it wants).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::CameraResult;

/// Discrete event categories. Frigate fires `new` / `update` / `end` on
/// the MQTT bus; Phase 1 collapses those into one enum tagged with the
/// transition.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// First frame of contiguous motion.
    MotionStart,
    /// Last frame of contiguous motion (debounced).
    MotionEnd,
    /// A new tracked object appeared.
    ObjectNew {
        /// Object class label ("person", "car", ...).
        label: String,
        /// Tracker-assigned object id.
        object_id: u64,
        /// Detector confidence 0..=1.
        confidence: f32,
    },
    /// An existing tracked object was updated (new bbox / score).
    ObjectUpdate {
        /// Object class label.
        label: String,
        /// Tracker-assigned object id.
        object_id: u64,
        /// Detector confidence 0..=1.
        confidence: f32,
    },
    /// A tracked object disappeared (missed N consecutive frames).
    ObjectEnd {
        /// Object class label.
        label: String,
        /// Tracker-assigned object id.
        object_id: u64,
    },
    /// A new MP4 segment file was written to disk.
    SegmentClosed {
        /// Absolute path to the closed segment.
        path: String,
        /// Duration in seconds (segment_seconds).
        duration_s: u32,
    },
}

/// One event delivered to a sink.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CameraEvent {
    /// Camera the event came from.
    pub camera: String,
    /// Monotonic event millis since UNIX epoch.
    pub timestamp_ms: u128,
    /// Event payload.
    pub kind: EventKind,
}

impl CameraEvent {
    /// Convenience constructor that stamps `timestamp_ms` from the system
    /// clock. Tests should construct the struct directly so they can pin
    /// the timestamp.
    pub fn now(camera: impl Into<String>, kind: EventKind) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Self {
            camera: camera.into(),
            timestamp_ms,
            kind,
        }
    }
}

/// Sink trait — the camera pipeline emits `CameraEvent`s and the
/// concrete sink decides what to do with them (log, publish, persist).
#[async_trait]
pub trait CameraEventSink: Send + Sync {
    /// Receive one event.
    async fn publish(&self, event: CameraEvent) -> CameraResult<()>;
}

/// Sink that discards everything — used by binaries that don't yet wire
/// the automation bus. Not a stub: it's a real, intentional null impl.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullEventSink;

#[async_trait]
impl CameraEventSink for NullEventSink {
    async fn publish(&self, _event: CameraEvent) -> CameraResult<()> {
        Ok(())
    }
}

/// In-process recording sink — buffers events in memory for tests +
/// `cavehomectl camera events`. Production deployments substitute the
/// automation-bus sink.
#[derive(Clone, Debug, Default)]
pub struct RecordingEventSink {
    inner: Arc<Mutex<Vec<CameraEvent>>>,
}

impl RecordingEventSink {
    /// New, empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of all events received so far, in order.
    #[must_use]
    pub fn snapshot(&self) -> Vec<CameraEvent> {
        self.inner.lock().clone()
    }

    /// Number of events received.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// Whether the sink has seen any events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

#[async_trait]
impl CameraEventSink for RecordingEventSink {
    async fn publish(&self, event: CameraEvent) -> CameraResult<()> {
        self.inner.lock().push(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn null_sink_accepts_events() {
        let sink = NullEventSink;
        sink.publish(CameraEvent::now("front", EventKind::MotionStart))
            .await
            .expect("null sink never errors");
    }

    #[tokio::test]
    async fn recording_sink_keeps_events_in_order() {
        let sink = RecordingEventSink::new();
        for i in 0_u32..3 {
            let evt = CameraEvent {
                camera: "front".into(),
                timestamp_ms: u128::from(i),
                kind: EventKind::SegmentClosed {
                    path: format!("/tmp/{i}.mp4"),
                    duration_s: 10,
                },
            };
            sink.publish(evt).await.expect("recording sink ok");
        }
        let snap = sink.snapshot();
        assert_eq!(snap.len(), 3);
        for (i, evt) in snap.iter().enumerate() {
            assert_eq!(evt.timestamp_ms, u128::try_from(i).unwrap_or(0));
        }
    }
}
