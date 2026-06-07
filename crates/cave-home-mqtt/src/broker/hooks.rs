//! Plugin hooks — the cave-home automation integration seam.
//!
//! A [`BrokerHook`] lets the cave-home automation engine observe broker
//! lifecycle events (connect / subscribe / disconnect) and authorize or
//! veto individual PUBLISHes without the broker depending on any
//! automation crate. The hook is synchronous and must not block.

use crate::packet::QoS;

/// A PUBLISH presented to a hook for inspection.
#[derive(Clone, Copy, Debug)]
pub struct PublishEvent<'a> {
    pub client_id: &'a str,
    pub topic: &'a str,
    pub qos: QoS,
    pub retain: bool,
    pub payload: &'a [u8],
}

/// A hook's verdict on a PUBLISH.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishDecision {
    /// Route the message normally.
    Accept,
    /// Drop the message (treated like an ACL denial).
    Reject,
}

/// Observer/interceptor implemented by the automation integration. All
/// methods have default no-op implementations; override what you need.
pub trait BrokerHook: Send + Sync {
    fn on_connect(&self, _client_id: &str) {}
    fn on_disconnect(&self, _client_id: &str) {}
    fn on_subscribe(&self, _client_id: &str, _topic_filter: &str) {}
    fn on_publish(&self, _event: &PublishEvent<'_>) -> PublishDecision {
        PublishDecision::Accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::auth::Authenticator;
    use crate::broker::{Action, Broker, BrokerConfig};
    use crate::v5::packet::*;
    use crate::v5::reason::ReasonCode;
    use bytes::Bytes;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<String>>,
        reject_topic: Option<String>,
    }

    impl BrokerHook for Recorder {
        fn on_connect(&self, client_id: &str) {
            self.events.lock().unwrap().push(format!("connect:{client_id}"));
        }
        fn on_disconnect(&self, client_id: &str) {
            self.events.lock().unwrap().push(format!("disconnect:{client_id}"));
        }
        fn on_subscribe(&self, client_id: &str, filter: &str) {
            self.events.lock().unwrap().push(format!("subscribe:{client_id}:{filter}"));
        }
        fn on_publish(&self, e: &PublishEvent<'_>) -> PublishDecision {
            self.events.lock().unwrap().push(format!("publish:{}:{}", e.client_id, e.topic));
            if self.reject_topic.as_deref() == Some(e.topic) {
                PublishDecision::Reject
            } else {
                PublishDecision::Accept
            }
        }
    }

    fn broker_with(hook: Arc<Recorder>) -> Broker {
        let mut auth = Authenticator::default();
        auth.set_anonymous(true);
        auth.set_default_allow(true);
        Broker::new(BrokerConfig::default(), auth).with_hook(hook)
    }

    fn connect(id: &str) -> ConnectV5 {
        ConnectV5 {
            client_id: id.into(),
            clean_start: true,
            keep_alive_secs: 0,
            properties: vec![],
            will: None,
            username: None,
            password: None,
        }
    }

    #[test]
    fn hook_observes_lifecycle_and_metrics_track_it() {
        let rec = Arc::new(Recorder::default());
        let mut b = broker_with(rec.clone());
        b.connect(connect("c"));
        b.handle("c", PacketV5::Subscribe(SubscribeV5 {
            packet_id: 1,
            properties: vec![],
            subscriptions: vec![SubscriptionV5 {
                topic_filter: "home/#".into(),
                qos: QoS::AtMostOnce,
                no_local: false,
                retain_as_published: false,
                retain_handling: RetainHandling::SendOnSubscribe,
            }],
        }));
        b.handle("c", PacketV5::Publish(PublishV5 {
            topic: "home/x".into(),
            qos: QoS::AtMostOnce,
            retain: false,
            dup: false,
            packet_id: None,
            properties: vec![],
            payload: Bytes::from_static(b"hello"),
        }));
        b.network_disconnect("c");

        let events = rec.events.lock().unwrap().clone();
        assert_eq!(events, vec![
            "connect:c",
            "subscribe:c:home/#",
            "publish:c:home/x",
            "disconnect:c",
        ]);

        let m = b.metrics();
        assert_eq!(m.clients_total, 1);
        assert_eq!(m.clients_connected, 0); // disconnected
        assert_eq!(m.messages_received, 1);
        assert_eq!(m.bytes_received, 5);
        // The no-local-free subscriber received the message.
        assert_eq!(m.messages_sent, 1);
    }

    #[test]
    fn hook_can_veto_a_publish() {
        let rec = Arc::new(Recorder { reject_topic: Some("home/secret".into()), ..Default::default() });
        let mut b = broker_with(rec);
        b.connect(connect("sub"));
        b.handle("sub", PacketV5::Subscribe(SubscribeV5 {
            packet_id: 1,
            properties: vec![],
            subscriptions: vec![SubscriptionV5 {
                topic_filter: "home/#".into(),
                qos: QoS::AtLeastOnce,
                no_local: false,
                retain_as_published: false,
                retain_handling: RetainHandling::SendOnSubscribe,
            }],
        }));
        b.connect(connect("pub"));
        let acts = b.handle("pub", PacketV5::Publish(PublishV5 {
            topic: "home/secret".into(),
            qos: QoS::AtLeastOnce,
            retain: false,
            dup: false,
            packet_id: Some(4),
            properties: vec![],
            payload: Bytes::from_static(b"x"),
        }));
        // Not routed to the subscriber; publisher gets NotAuthorized.
        assert!(!acts.iter().any(|a| matches!(a,
            Action::Send { client_id, packet: PacketV5::Publish(_) } if client_id == "sub")));
        assert!(acts.iter().any(|a| matches!(a,
            Action::Send { packet: PacketV5::PubAck(ack), .. }
            if ack.packet_id == 4 && ack.reason_code == ReasonCode::NotAuthorized)));
    }

    #[test]
    fn prometheus_snapshot_reflects_gauges() {
        let rec = Arc::new(Recorder::default());
        let mut b = broker_with(rec);
        b.connect(connect("c"));
        b.handle("c", PacketV5::Subscribe(SubscribeV5 {
            packet_id: 1,
            properties: vec![],
            subscriptions: vec![SubscriptionV5 {
                topic_filter: "home/#".into(),
                qos: QoS::AtMostOnce,
                no_local: false,
                retain_as_published: false,
                retain_handling: RetainHandling::SendOnSubscribe,
            }],
        }));
        b.handle("c", PacketV5::Publish(PublishV5 {
            topic: "home/keep".into(),
            qos: QoS::AtMostOnce,
            retain: true,
            dup: false,
            packet_id: None,
            properties: vec![],
            payload: Bytes::from_static(b"v"),
        }));
        let text = b.prometheus();
        assert!(text.contains("\nmqtt_clients_connected 1\n"));
        assert!(text.contains("\nmqtt_retained_messages 1\n"));
        assert!(text.contains("\nmqtt_subscriptions 1\n"));
    }
}
