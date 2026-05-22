// SPDX-License-Identifier: Apache-2.0
//! `EventSource` trait + `MockEventSource`.
//!
//! Real apiserver Watch wiring is `[[unmapped]]` Phase 1b — implementations
//! will live in `cave-home-apiserver-rs` (workspace integration concern).
//! Tests use `MockEventSource`.

use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::api::WatchEvent;

/// Trait for an event-bus producing `WatchEvent`s. Implementors must be
/// safe to share across threads — the proxier reconciler `tokio::spawn`s
/// the consumer.
pub trait EventSource: Send + Sync {
    /// Returns a fresh receiver for the event stream. Calling twice yields
    /// two independent receivers (broadcast or fan-out semantics are NOT
    /// required — the proxier only ever takes one).
    fn stream(&self) -> mpsc::UnboundedReceiver<WatchEvent>;
}

/// In-memory event source used by all tests. Cheap to clone (Arc inside).
#[derive(Debug, Clone, Default)]
pub struct MockEventSource {
    inner: Arc<Mutex<MockState>>,
}

#[derive(Debug, Default)]
struct MockState {
    /// Events queued before any consumer subscribed. Once a consumer calls
    /// `stream()` we drain pending into the new sender and start streaming
    /// directly to the live sender.
    pending: Vec<WatchEvent>,
    sender: Option<mpsc::UnboundedSender<WatchEvent>>,
    closed: bool,
}

impl MockEventSource {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push an event. Delivered immediately if a consumer is attached;
    /// otherwise queued for the next `stream()` call.
    pub fn push(&self, ev: WatchEvent) {
        let mut st = self.inner.lock();
        if let Some(tx) = &st.sender {
            // If the receiver was dropped, drop the event silently —
            // matches real Watch behaviour where the apiserver doesn't
            // care about disconnected consumers.
            let _ = tx.send(ev);
        } else {
            st.pending.push(ev);
        }
    }

    /// Mark the stream as terminated. Causes any active receiver to return
    /// `None` once drained.
    pub fn close(&self) {
        let mut st = self.inner.lock();
        st.closed = true;
        st.sender = None;
    }
}

impl EventSource for MockEventSource {
    fn stream(&self) -> mpsc::UnboundedReceiver<WatchEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut st = self.inner.lock();
        for ev in st.pending.drain(..) {
            let _ = tx.send(ev);
        }
        if !st.closed {
            st.sender = Some(tx);
        }
        // Once `tx` is dropped (close() drops the stored Some) the rx will
        // observe end-of-stream.
        rx
    }
}
