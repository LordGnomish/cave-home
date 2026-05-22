// SPDX-License-Identifier: Apache-2.0
//! Event surface — sink trait the binary wires to the Automation bus.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/zwave-js/src/lib/driver/Driver.ts
//!
//! Upstream's driver fans events out through a typed `EventEmitter` (`"node added"`,
//! `"node ready"`, `"value updated"`, `"inclusion started"`, `"inclusion stopped"`,
//! `"controller ready"`). We port the same set as a small explicit trait so the
//! Z-Wave crate stays free of a hard dependency on the automation bus.
//!
//! Charter v2 / ADR-007 note: events here use English identifiers
//! (`device_paired`, `device_unpaired`, …) and the Portal/cavectl layers map
//! them to the grandma-friendly TR/EN/DE strings in `docs/ui-language.md`.

use std::sync::Arc;

use bytes::Bytes;

/// One Z-Wave-level event.
#[derive(Clone, Debug)]
pub enum ZwaveEvent {
    /// The driver has booted, talked to the controller and learned its Home ID.
    ControllerReady {
        /// 32-bit Home ID reported by `GetControllerId`.
        home_id: u32,
        /// Controller's own node ID (typically 1).
        own_node_id: u8,
    },
    /// A new device just finished inclusion. UI label: "Yeni cihaz eşlendi."
    DevicePaired {
        /// 8-bit node ID the controller assigned.
        node_id: u8,
        /// Security class the node accepted (S2 Access / S2 Authenticated /
        /// S2 Unauthenticated / S0 / None).
        security_class: crate::security::SecurityClass,
    },
    /// A device was excluded from the network. UI label: "Cihaz ağdan çıkarıldı."
    DeviceUnpaired {
        /// Node ID that was excluded.
        node_id: u8,
    },
    /// A new value report arrived from a node. UI maps value IDs to home-world
    /// labels ("Salon lambası açık", "Banyo nem %42").
    ValueUpdated {
        /// Source node ID.
        node_id: u8,
        /// Command-class identifier byte.
        command_class: u8,
        /// Property name (upstream uses string; we mirror).
        property: String,
        /// Raw bytes of the value as decoded by the command class.
        payload: Bytes,
    },
    /// An include or exclude run was rolled back / timed out.
    InclusionFailed {
        /// Human-readable reason (en).
        reason: String,
    },
}

/// Trait the binary wires to its event bus. The binary later attaches a real
/// `EventBus` implementation; tests pass a counting / collecting double.
pub trait ZwaveEventSink: Send + Sync {
    /// Receive one event. Must not block.
    fn emit(&self, event: ZwaveEvent);
}

/// In-memory sink that just collects events — used by tests + the cavectl
/// `list` subcommand when no live driver is attached.
#[derive(Debug, Default)]
pub struct MemoryEventSink {
    inner: parking_lot::Mutex<Vec<ZwaveEvent>>,
}

impl MemoryEventSink {
    /// Build an empty sink.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Drain all events recorded so far.
    pub fn drain(&self) -> Vec<ZwaveEvent> {
        let mut guard = self.inner.lock();
        std::mem::take(&mut *guard)
    }

    /// Current number of events, without draining.
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// Whether the sink is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl ZwaveEventSink for MemoryEventSink {
    fn emit(&self, event: ZwaveEvent) {
        self.inner.lock().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityClass;

    #[test]
    fn memory_sink_collects_and_drains() {
        let sink = MemoryEventSink::new();
        sink.emit(ZwaveEvent::ControllerReady {
            home_id: 0xdead_beef,
            own_node_id: 1,
        });
        sink.emit(ZwaveEvent::DevicePaired {
            node_id: 12,
            security_class: SecurityClass::S2Authenticated,
        });
        assert_eq!(sink.len(), 2);
        let drained = sink.drain();
        assert_eq!(drained.len(), 2);
        assert!(sink.is_empty());
    }
}
