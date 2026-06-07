//! Broker metrics and Prometheus text-format exposition.
//!
//! Counters are monotonic totals; gauges (`clients_connected`,
//! `retained_messages`, `subscriptions`) reflect current state. The
//! broker updates these as it processes packets; [`render_prometheus`]
//! emits the standard text exposition format for a `/metrics` endpoint.
//!
//! [`render_prometheus`]: BrokerMetrics::render_prometheus

use std::fmt::Write as _;

/// Snapshot of broker activity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrokerMetrics {
    /// Gauge — clients currently connected.
    pub clients_connected: u64,
    /// Counter — connections accepted over the broker's lifetime.
    pub clients_total: u64,
    /// Counter — PUBLISH packets received from clients.
    pub messages_received: u64,
    /// Counter — PUBLISH packets delivered to subscribers.
    pub messages_sent: u64,
    /// Counter — PUBLISH packets dropped (denied / no subscriber, QoS 0).
    pub messages_dropped: u64,
    /// Counter — payload bytes received.
    pub bytes_received: u64,
    /// Counter — payload bytes delivered.
    pub bytes_sent: u64,
    /// Gauge — retained messages currently stored.
    pub retained_messages: u64,
    /// Gauge — active subscriptions across all sessions.
    pub subscriptions: u64,
}

impl BrokerMetrics {
    pub fn on_client_connected(&mut self) {
        self.clients_connected += 1;
        self.clients_total += 1;
    }

    pub fn on_client_disconnected(&mut self) {
        self.clients_connected = self.clients_connected.saturating_sub(1);
    }

    pub fn on_message_received(&mut self, payload_len: usize) {
        self.messages_received += 1;
        self.bytes_received += payload_len as u64;
    }

    pub fn on_message_sent(&mut self, payload_len: usize) {
        self.messages_sent += 1;
        self.bytes_sent += payload_len as u64;
    }

    pub fn on_message_dropped(&mut self) {
        self.messages_dropped += 1;
    }

    pub fn set_retained(&mut self, n: u64) {
        self.retained_messages = n;
    }

    pub fn set_subscriptions(&mut self, n: u64) {
        self.subscriptions = n;
    }

    /// Render the Prometheus text exposition format (one HELP + TYPE +
    /// sample block per metric).
    #[must_use]
    pub fn render_prometheus(&self) -> String {
        const METRICS: &[(&str, &str, &str)] = &[
            ("mqtt_clients_connected", "gauge", "Clients currently connected."),
            ("mqtt_clients_total", "counter", "Connections accepted over the broker lifetime."),
            ("mqtt_messages_received_total", "counter", "PUBLISH packets received from clients."),
            ("mqtt_messages_sent_total", "counter", "PUBLISH packets delivered to subscribers."),
            ("mqtt_messages_dropped_total", "counter", "PUBLISH packets dropped."),
            ("mqtt_bytes_received_total", "counter", "Payload bytes received."),
            ("mqtt_bytes_sent_total", "counter", "Payload bytes delivered."),
            ("mqtt_retained_messages", "gauge", "Retained messages stored."),
            ("mqtt_subscriptions", "gauge", "Active subscriptions."),
        ];
        let value = |name: &str| match name {
            "mqtt_clients_connected" => self.clients_connected,
            "mqtt_clients_total" => self.clients_total,
            "mqtt_messages_received_total" => self.messages_received,
            "mqtt_messages_sent_total" => self.messages_sent,
            "mqtt_messages_dropped_total" => self.messages_dropped,
            "mqtt_bytes_received_total" => self.bytes_received,
            "mqtt_bytes_sent_total" => self.bytes_sent,
            "mqtt_retained_messages" => self.retained_messages,
            "mqtt_subscriptions" => self.subscriptions,
            _ => 0,
        };
        let mut out = String::new();
        for (name, kind, help) in METRICS {
            let _ = writeln!(out, "# HELP {name} {help}");
            let _ = writeln!(out, "# TYPE {name} {kind}");
            let _ = writeln!(out, "{name} {}", value(name));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_and_gauges_accumulate() {
        let mut m = BrokerMetrics::default();
        m.on_client_connected();
        m.on_client_connected();
        m.on_client_disconnected();
        m.on_message_received(10);
        m.on_message_sent(8);
        m.on_message_sent(8);
        m.set_retained(3);
        m.set_subscriptions(5);

        assert_eq!(m.clients_connected, 1); // 2 connected, 1 left
        assert_eq!(m.clients_total, 2);
        assert_eq!(m.messages_received, 1);
        assert_eq!(m.messages_sent, 2);
        assert_eq!(m.bytes_received, 10);
        assert_eq!(m.bytes_sent, 16);
        assert_eq!(m.retained_messages, 3);
        assert_eq!(m.subscriptions, 5);
    }

    #[test]
    fn prometheus_exposition_is_well_formed() {
        let mut m = BrokerMetrics::default();
        m.on_client_connected();
        m.on_message_received(42);
        let text = m.render_prometheus();
        // Every metric needs a HELP and TYPE line and a samevalue sample.
        assert!(text.contains("# HELP mqtt_clients_connected"));
        assert!(text.contains("# TYPE mqtt_clients_connected gauge"));
        assert!(text.contains("\nmqtt_clients_connected 1\n"));
        assert!(text.contains("# TYPE mqtt_messages_received_total counter"));
        assert!(text.contains("\nmqtt_messages_received_total 1\n"));
        assert!(text.contains("\nmqtt_bytes_received_total 42\n"));
        assert!(text.ends_with('\n'));
    }
}
