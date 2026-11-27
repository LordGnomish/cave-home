//! Pinning tests for the broker's state holders: the retained-message
//! store (MQTT 5.0 §3.3.1.3), the per-client session with its packet
//! identifier allocator (§2.2.1, §4.1) and the metrics with their
//! Prometheus text exposition.

use bytes::Bytes;
use cave_home_mqtt::broker::retain::{RetainedMessage, RetainedStore};
use cave_home_mqtt::broker::session::Session;
use cave_home_mqtt::metrics::BrokerMetrics;
use cave_home_mqtt::packet::QoS;
use cave_home_mqtt::v5::packet::{PublishV5, RetainHandling, SubscriptionV5};
use cave_home_mqtt::v5::property::Property;

// ---- retained store -------------------------------------------------------------

fn retained(payload: &'static [u8], qos: QoS) -> RetainedMessage {
    RetainedMessage {
        payload: Bytes::from_static(payload),
        qos,
        properties: vec![Property::ContentType("text/plain".to_owned())],
    }
}

/// The matching topics with their messages, sorted by topic (the store
/// itself returns them in no particular order).
fn matching_sorted<'a>(
    store: &'a RetainedStore,
    filter: &str,
) -> Vec<(&'a str, &'a RetainedMessage)> {
    let mut hits = store.matching(filter);
    hits.sort_unstable_by_key(|(topic, _)| *topic);
    hits
}

#[test]
fn a_fresh_store_is_empty() {
    let store = RetainedStore::default();
    assert_eq!((store.len(), store.is_empty()), (0, true));
    assert_eq!(store.matching("#"), []);
}

#[test]
fn retained_messages_are_returned_whole_for_matching_filters() {
    let mut store = RetainedStore::default();
    let loft = retained(b"21.0", QoS::AtLeastOnce);
    let cellar = retained(b"12.0", QoS::ExactlyOnce);
    let sys = retained(b"up", QoS::AtMostOnce);
    store.apply("home/loft/temp", loft.clone());
    store.apply("home/cellar/temp", cellar.clone());
    store.apply("$SYS/broker", sys.clone());
    assert_eq!((store.len(), store.is_empty()), (3, false));

    assert_eq!(
        matching_sorted(&store, "home/+/temp"),
        [("home/cellar/temp", &cellar), ("home/loft/temp", &loft)]
    );
    assert_eq!(
        matching_sorted(&store, "home/loft/temp"),
        [("home/loft/temp", &loft)]
    );
    // §4.7.2: `#` does not reach the `$` topics.
    assert_eq!(
        matching_sorted(&store, "#"),
        [("home/cellar/temp", &cellar), ("home/loft/temp", &loft)]
    );
    assert_eq!(matching_sorted(&store, "$SYS/#"), [("$SYS/broker", &sys)]);
    assert_eq!(store.matching("garden/#"), []);
}

#[test]
fn the_last_retained_message_of_a_topic_wins() {
    let mut store = RetainedStore::default();
    store.apply("a/b", retained(b"first", QoS::AtMostOnce));
    let second = retained(b"second", QoS::ExactlyOnce);
    store.apply("a/b", second.clone());
    assert_eq!(store.len(), 1);
    assert_eq!(store.matching("a/b"), [("a/b", &second)]);
}

#[test]
fn an_empty_payload_removes_the_retained_message_and_stores_nothing() {
    let mut store = RetainedStore::default();
    store.apply("a/b", retained(b"x", QoS::AtLeastOnce));
    store.apply("a/c", retained(b"y", QoS::AtLeastOnce));
    store.apply("a/b", retained(b"", QoS::AtLeastOnce));
    assert_eq!(store.len(), 1);
    assert_eq!(store.matching("a/b"), []);
    // Nothing is stored for a topic that had no retained message either.
    store.apply("a/d", retained(b"", QoS::ExactlyOnce));
    assert_eq!(store.len(), 1);
    assert_eq!(store.matching("a/d"), []);
}

#[test]
fn clear_removes_one_topic_and_ignores_unknown_ones() {
    let mut store = RetainedStore::default();
    let kept = retained(b"y", QoS::AtLeastOnce);
    store.apply("a/b", retained(b"x", QoS::AtLeastOnce));
    store.apply("a/c", kept.clone());
    store.clear("a/b");
    store.clear("a/zzz");
    // `clear` takes a topic name, not a filter.
    store.clear("a/+");
    assert_eq!(matching_sorted(&store, "#"), [("a/c", &kept)]);
    store.clear("a/c");
    assert_eq!((store.len(), store.is_empty()), (0, true));
}

// ---- session --------------------------------------------------------------------

fn subscription(filter: &str, qos: QoS, no_local: bool) -> SubscriptionV5 {
    SubscriptionV5 {
        topic_filter: filter.to_owned(),
        qos,
        no_local,
        retain_as_published: false,
        retain_handling: RetainHandling::SendOnSubscribe,
    }
}

fn in_flight(packet_id: u16) -> PublishV5 {
    PublishV5 {
        topic: "t".to_owned(),
        qos: QoS::AtLeastOnce,
        retain: false,
        dup: false,
        packet_id: Some(packet_id),
        properties: vec![],
        payload: Bytes::new(),
    }
}

#[test]
fn a_new_session_is_offline_and_empty() {
    let session = Session::new("c1".to_owned(), 3600);
    assert_eq!(session.client_id, "c1");
    assert_eq!(session.username, None);
    assert_eq!(session.session_expiry_secs, 3600);
    assert!(!session.connected);
    assert_eq!(session.subscriptions, []);
    assert_eq!(session.outgoing_unacked.len(), 0);
    assert_eq!(session.outgoing_pubrel.len(), 0);
    assert_eq!(session.incoming_qos2.len(), 0);
    assert_eq!(session.queued.len(), 0);
}

#[test]
fn packet_identifiers_count_up_from_one_and_skip_zero_when_wrapping() {
    let mut session = Session::new("c".to_owned(), 0);
    for expected in 1..=u16::MAX {
        assert_eq!(session.next_id(), expected);
    }
    // §2.2.1: a packet identifier is never zero.
    assert_eq!(session.next_id(), 1);
    assert_eq!(session.next_id(), 2);
}

#[test]
fn packet_identifiers_skip_those_still_in_flight() {
    let mut session = Session::new("c".to_owned(), 0);
    session.outgoing_unacked.insert(1, in_flight(1));
    session.outgoing_unacked.insert(2, in_flight(2));
    session.outgoing_pubrel.insert(3);
    // Inbound QoS 2 identifiers belong to the client's own number space.
    session.incoming_qos2.insert(4);
    assert_eq!(session.next_id(), 4);
    assert_eq!(session.next_id(), 5);

    session
        .outgoing_unacked
        .insert(u16::MAX, in_flight(u16::MAX));
    session.outgoing_unacked.remove(&2);
    let mut last = 5;
    while last < u16::MAX - 1 {
        last = session.next_id();
    }
    assert_eq!(last, u16::MAX - 1);
    // 65535 and 1 are in flight, 0 is never used, 2 has been acknowledged.
    assert_eq!(session.next_id(), 2);
}

#[test]
fn subscriptions_are_replaced_in_place_by_topic_filter() {
    let mut session = Session::new("c".to_owned(), 0);
    assert!(!session.add_subscription(subscription("a/#", QoS::AtMostOnce, false)));
    assert!(!session.add_subscription(subscription("b/+", QoS::AtLeastOnce, false)));
    assert!(!session.add_subscription(subscription("c", QoS::ExactlyOnce, false)));
    // Same filter: the options are replaced, the position is kept.
    assert!(session.add_subscription(subscription("b/+", QoS::ExactlyOnce, true)));
    assert_eq!(
        session.subscriptions,
        [
            subscription("a/#", QoS::AtMostOnce, false),
            subscription("b/+", QoS::ExactlyOnce, true),
            subscription("c", QoS::ExactlyOnce, false),
        ]
    );

    assert!(session.remove_subscription("a/#"));
    assert!(!session.remove_subscription("a/#"));
    // Filters are compared as text, not matched.
    assert!(!session.remove_subscription("#"));
    assert!(!session.remove_subscription("b/x"));
    assert_eq!(
        session.subscriptions,
        [
            subscription("b/+", QoS::ExactlyOnce, true),
            subscription("c", QoS::ExactlyOnce, false),
        ]
    );
}

// ---- metrics --------------------------------------------------------------------

#[test]
fn metrics_start_at_zero_and_count_each_event() {
    let mut metrics = BrokerMetrics::default();
    assert_eq!(
        metrics,
        BrokerMetrics {
            clients_connected: 0,
            clients_total: 0,
            messages_received: 0,
            messages_sent: 0,
            messages_dropped: 0,
            bytes_received: 0,
            bytes_sent: 0,
            retained_messages: 0,
            subscriptions: 0,
        }
    );
    metrics.on_client_connected();
    metrics.on_client_connected();
    metrics.on_client_disconnected();
    metrics.on_message_received(10);
    metrics.on_message_received(0);
    metrics.on_message_sent(8);
    metrics.on_message_sent(8);
    metrics.on_message_sent(1);
    metrics.on_message_dropped();
    metrics.set_retained(3);
    metrics.set_subscriptions(5);
    assert_eq!(
        metrics,
        BrokerMetrics {
            clients_connected: 1,
            clients_total: 2,
            messages_received: 2,
            messages_sent: 3,
            messages_dropped: 1,
            bytes_received: 10,
            bytes_sent: 17,
            retained_messages: 3,
            subscriptions: 5,
        }
    );
}

#[test]
fn the_connected_gauge_never_goes_below_zero() {
    let mut metrics = BrokerMetrics::default();
    metrics.on_client_disconnected();
    metrics.on_client_connected();
    metrics.on_client_disconnected();
    metrics.on_client_disconnected();
    assert_eq!((metrics.clients_connected, metrics.clients_total), (0, 1));
}

#[test]
fn prometheus_exposition_is_byte_exact() {
    let metrics = BrokerMetrics {
        clients_connected: 1,
        clients_total: 2,
        messages_received: 3,
        messages_sent: 4,
        messages_dropped: 5,
        bytes_received: 6,
        bytes_sent: 7,
        retained_messages: 8,
        subscriptions: u64::MAX,
    };
    let expected = "\
# HELP mqtt_clients_connected Clients currently connected.
# TYPE mqtt_clients_connected gauge
mqtt_clients_connected 1
# HELP mqtt_clients_total Connections accepted over the broker lifetime.
# TYPE mqtt_clients_total counter
mqtt_clients_total 2
# HELP mqtt_messages_received_total PUBLISH packets received from clients.
# TYPE mqtt_messages_received_total counter
mqtt_messages_received_total 3
# HELP mqtt_messages_sent_total PUBLISH packets delivered to subscribers.
# TYPE mqtt_messages_sent_total counter
mqtt_messages_sent_total 4
# HELP mqtt_messages_dropped_total PUBLISH packets dropped.
# TYPE mqtt_messages_dropped_total counter
mqtt_messages_dropped_total 5
# HELP mqtt_bytes_received_total Payload bytes received.
# TYPE mqtt_bytes_received_total counter
mqtt_bytes_received_total 6
# HELP mqtt_bytes_sent_total Payload bytes delivered.
# TYPE mqtt_bytes_sent_total counter
mqtt_bytes_sent_total 7
# HELP mqtt_retained_messages Retained messages stored.
# TYPE mqtt_retained_messages gauge
mqtt_retained_messages 8
# HELP mqtt_subscriptions Active subscriptions.
# TYPE mqtt_subscriptions gauge
mqtt_subscriptions 18446744073709551615
";
    assert_eq!(metrics.render_prometheus(), expected);
}
