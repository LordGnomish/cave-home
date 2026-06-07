//! Broker metrics and Prometheus text-format exposition.

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
