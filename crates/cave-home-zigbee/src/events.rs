// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Outbound event stream.
//!
//! The coordinator emits events of type [`ZigbeeEvent`] over a Tokio
//! broadcast channel. Callers (Portal UI, MQTT bridge, automation
//! engine) subscribe with [`EventBus::subscribe`] and receive a
//! `tokio::sync::broadcast::Receiver`.

use tokio::sync::broadcast;

use crate::attribute_reporting::Reported;

/// All events the Phase 1 coordinator emits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZigbeeEvent {
    /// Coordinator finished initialising (transport up + NIB populated).
    CoordinatorReady {
        /// IEEE address of the coordinator (= NCP).
        coordinator_ieee: u64,
        /// PAN ID we're operating on.
        pan_id: u16,
        /// 802.15.4 channel.
        channel: u8,
    },
    /// Coordinator lost contact with its NCP.
    CoordinatorDown { reason: String },
    /// A new device joined the network.
    DeviceJoined {
        device_ieee: u64,
        device_short_address: u16,
    },
    /// A device left / was removed.
    DeviceLeft { device_ieee: u64 },
    /// An attribute was reported by a device.
    AttributeReported(Reported),
    /// OTA progress signal.
    OtaProgress {
        device_ieee: u64,
        manufacturer_code: u16,
        image_type: u16,
        new_file_version: u32,
    },
}

/// Broadcast event bus. Cheap to clone.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<ZigbeeEvent>,
}

impl EventBus {
    /// Build a new bus with the given channel buffer size.
    #[must_use]
    pub fn new(buffer: usize) -> Self {
        let (tx, _) = broadcast::channel(buffer);
        Self { tx }
    }

    /// Subscribe — every event published from this point on will arrive.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ZigbeeEvent> {
        self.tx.subscribe()
    }

    /// Number of currently-active subscribers.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Publish an event. Returns the number of receivers that got it,
    /// or 0 if there were none (no-op, not an error per `broadcast`'s contract).
    pub fn publish(&self, event: ZigbeeEvent) -> usize {
        self.tx.send(event).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_to_no_subscribers_is_zero() {
        let bus = EventBus::new(8);
        let n = bus.publish(ZigbeeEvent::CoordinatorDown {
            reason: "test".into(),
        });
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn subscriber_receives_published_event() {
        let bus = EventBus::new(8);
        let mut rx = bus.subscribe();
        bus.publish(ZigbeeEvent::DeviceJoined {
            device_ieee: 0xaa,
            device_short_address: 0x1234,
        });
        let got = rx.recv().await.unwrap();
        assert_eq!(
            got,
            ZigbeeEvent::DeviceJoined {
                device_ieee: 0xaa,
                device_short_address: 0x1234,
            }
        );
    }

    #[tokio::test]
    async fn multiple_subscribers_each_receive() {
        let bus = EventBus::new(8);
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);
        bus.publish(ZigbeeEvent::DeviceLeft {
            device_ieee: 0x42,
        });
        let ea = a.recv().await.unwrap();
        let eb = b.recv().await.unwrap();
        assert_eq!(ea, eb);
    }
}
