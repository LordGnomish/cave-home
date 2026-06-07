//! The cave-home MQTT broker decision core — I/O-free.
//!
//! Everything here is pure: packets in, [`Action`]s out. The async TCP /
//! WebSocket / TLS listeners (behind the `runtime` feature) drive this
//! core but contain no protocol logic, which keeps the broker fully
//! unit-testable without sockets.

pub mod auth;
pub mod retain;
pub mod session;
pub mod topic;

#[cfg(test)]
mod router_tests {
    use super::*;
    use crate::broker::auth::{AclAction, Authenticator};
    use crate::packet::QoS;
    use crate::v5::packet::*;
    use crate::v5::property::Property;
    use crate::v5::reason::ReasonCode;
    use bytes::Bytes;

    fn allow_all() -> Broker {
        let mut auth = Authenticator::default();
        auth.set_anonymous(true);
        auth.set_default_allow(true);
        Broker::new(BrokerConfig::default(), auth)
    }

    fn connect(id: &str, clean_start: bool) -> ConnectV5 {
        ConnectV5 {
            client_id: id.into(),
            clean_start,
            keep_alive_secs: 0,
            properties: vec![],
            will: None,
            username: None,
            password: None,
        }
    }

    fn subscribe(filter: &str, qos: QoS, packet_id: u16) -> PacketV5 {
        PacketV5::Subscribe(SubscribeV5 {
            packet_id,
            properties: vec![],
            subscriptions: vec![SubscriptionV5 {
                topic_filter: filter.into(),
                qos,
                no_local: false,
                retain_as_published: false,
                retain_handling: RetainHandling::SendOnSubscribe,
            }],
        })
    }

    fn publish(topic: &str, qos: QoS, packet_id: Option<u16>, retain: bool, body: &'static [u8]) -> PacketV5 {
        PacketV5::Publish(PublishV5 {
            topic: topic.into(),
            qos,
            retain,
            dup: false,
            packet_id,
            properties: vec![],
            payload: Bytes::from_static(body),
        })
    }

    /// Collect (client_id, packet) for every Send action.
    fn sends(actions: &[Action]) -> Vec<(String, PacketV5)> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Send { client_id, packet } => Some((client_id.clone(), packet.clone())),
                Action::Drop { .. } => None,
            })
            .collect()
    }

    fn find_publish_to<'a>(actions: &'a [Action], who: &str) -> Option<&'a PublishV5> {
        actions.iter().find_map(|a| match a {
            Action::Send { client_id, packet: PacketV5::Publish(p) } if client_id == who => Some(p),
            _ => None,
        })
    }

    #[test]
    fn connect_yields_connack_success() {
        let mut b = allow_all();
        let acts = b.connect(connect("c1", true));
        let s = sends(&acts);
        assert_eq!(s.len(), 1);
        match &s[0].1 {
            PacketV5::ConnAck(ack) => {
                assert_eq!(ack.reason_code, ReasonCode::Success);
                assert!(!ack.session_present);
            }
            other => panic!("expected CONNACK, got {other:?}"),
        }
        assert_eq!(b.session_count(), 1);
    }

    #[test]
    fn bad_credentials_are_refused() {
        let mut auth = Authenticator::default();
        auth.set_anonymous(false);
        auth.add_user("admin", b"pw");
        let mut b = Broker::new(BrokerConfig::default(), auth);
        let mut c = connect("c1", true);
        c.username = Some("admin".into());
        c.password = Some(Bytes::from_static(b"wrong"));
        let acts = b.connect(c);
        match &sends(&acts)[0].1 {
            PacketV5::ConnAck(ack) => assert_eq!(ack.reason_code, ReasonCode::BadUserNameOrPassword),
            other => panic!("expected CONNACK, got {other:?}"),
        }
        assert!(acts.iter().any(|a| matches!(a, Action::Drop { .. })));
        assert_eq!(b.session_count(), 0);
    }

    #[test]
    fn subscribe_grants_qos_and_qos0_publish_is_routed() {
        let mut b = allow_all();
        b.connect(connect("sub", true));
        let suback = b.handle("sub", subscribe("home/+/temp", QoS::ExactlyOnce, 1));
        match &sends(&suback)[0].1 {
            PacketV5::SubAck(s) => assert_eq!(s.reason_codes, vec![ReasonCode::GrantedQoS2]),
            other => panic!("expected SUBACK, got {other:?}"),
        }
        b.connect(connect("pub", true));
        let acts = b.handle("pub", publish("home/loft/temp", QoS::AtMostOnce, None, false, b"21"));
        let p = find_publish_to(&acts, "sub").expect("routed to sub");
        assert_eq!(p.topic, "home/loft/temp");
        assert_eq!(p.qos, QoS::AtMostOnce);
        assert_eq!(&p.payload[..], b"21");
    }

    #[test]
    fn qos1_publish_acks_publisher_and_delivers_then_clears_on_puback() {
        let mut b = allow_all();
        b.connect(connect("sub", true));
        b.handle("sub", subscribe("home/#", QoS::AtLeastOnce, 1));
        b.connect(connect("pub", true));
        let acts = b.handle("pub", publish("home/x", QoS::AtLeastOnce, Some(5), false, b"v"));
        // Publisher gets a PUBACK(5) Success.
        assert!(acts.iter().any(|a| matches!(a,
            Action::Send { client_id, packet: PacketV5::PubAck(ack) }
            if client_id == "pub" && ack.packet_id == 5 && ack.reason_code == ReasonCode::Success)));
        // Subscriber gets a PUBLISH with a server-assigned packet id.
        let p = find_publish_to(&acts, "sub").expect("delivered");
        let pid = p.packet_id.expect("qos1 needs id");
        assert_eq!(p.qos, QoS::AtLeastOnce);
        // Subscriber acks; inflight is cleared (no resend on takeover later).
        let after = b.handle("sub", PacketV5::PubAck(PubAckV5 {
            packet_id: pid,
            reason_code: ReasonCode::Success,
            properties: vec![],
        }));
        assert!(sends(&after).is_empty());
    }

    #[test]
    fn qos1_publish_with_no_subscribers_reports_no_matching() {
        let mut b = allow_all();
        b.connect(connect("pub", true));
        let acts = b.handle("pub", publish("nobody/here", QoS::AtLeastOnce, Some(9), false, b"v"));
        assert!(acts.iter().any(|a| matches!(a,
            Action::Send { packet: PacketV5::PubAck(ack), .. }
            if ack.packet_id == 9 && ack.reason_code == ReasonCode::NoMatchingSubscribers)));
    }

    #[test]
    fn qos2_full_handshake_both_directions() {
        let mut b = allow_all();
        b.connect(connect("sub", true));
        b.handle("sub", subscribe("home/#", QoS::ExactlyOnce, 1));
        b.connect(connect("pub", true));

        // Publisher → broker: PUBLISH qos2 ⇒ PUBREC to publisher + PUBLISH to sub.
        let acts = b.handle("pub", publish("home/x", QoS::ExactlyOnce, Some(7), false, b"v"));
        assert!(acts.iter().any(|a| matches!(a,
            Action::Send { client_id, packet: PacketV5::PubRec(r) }
            if client_id == "pub" && r.packet_id == 7)));
        let pid = find_publish_to(&acts, "sub").unwrap().packet_id.unwrap();

        // Publisher → broker: PUBREL(7) ⇒ PUBCOMP(7) to publisher.
        let comp = b.handle("pub", PacketV5::PubRel(PubRelV5 {
            packet_id: 7, reason_code: ReasonCode::Success, properties: vec![] }));
        assert!(comp.iter().any(|a| matches!(a,
            Action::Send { client_id, packet: PacketV5::PubComp(c) }
            if client_id == "pub" && c.packet_id == 7)));

        // Subscriber → broker: PUBREC(pid) ⇒ PUBREL(pid) to subscriber.
        let rel = b.handle("sub", PacketV5::PubRec(PubRecV5 {
            packet_id: pid, reason_code: ReasonCode::Success, properties: vec![] }));
        assert!(rel.iter().any(|a| matches!(a,
            Action::Send { client_id, packet: PacketV5::PubRel(r) }
            if client_id == "sub" && r.packet_id == pid)));

        // Subscriber → broker: PUBCOMP(pid) completes; nothing further.
        let done = b.handle("sub", PacketV5::PubComp(PubCompV5 {
            packet_id: pid, reason_code: ReasonCode::Success, properties: vec![] }));
        assert!(sends(&done).is_empty());
    }

    #[test]
    fn retained_message_delivered_on_new_subscription() {
        let mut b = allow_all();
        b.connect(connect("pub", true));
        b.handle("pub", publish("home/loft/temp", QoS::AtLeastOnce, Some(1), true, b"21.0"));
        assert_eq!(b.retained_count(), 1);

        b.connect(connect("sub", true));
        let acts = b.handle("sub", subscribe("home/#", QoS::AtLeastOnce, 2));
        let p = find_publish_to(&acts, "sub").expect("retained delivered");
        assert_eq!(p.topic, "home/loft/temp");
        assert!(p.retain, "retained delivery sets RETAIN=1");
        assert_eq!(&p.payload[..], b"21.0");
    }

    #[test]
    fn empty_retained_publish_clears_the_topic() {
        let mut b = allow_all();
        b.connect(connect("pub", true));
        b.handle("pub", publish("home/loft/temp", QoS::AtLeastOnce, Some(1), true, b"21.0"));
        b.handle("pub", publish("home/loft/temp", QoS::AtLeastOnce, Some(2), true, b""));
        assert_eq!(b.retained_count(), 0);
    }

    #[test]
    fn persistent_session_queues_offline_and_delivers_on_resume() {
        let mut b = allow_all();
        // Subscriber with a non-zero session expiry, clean_start = false.
        let mut sub_connect = connect("sub", false);
        sub_connect.properties = vec![Property::SessionExpiryInterval(3600)];
        b.connect(sub_connect.clone());
        b.handle("sub", subscribe("home/#", QoS::AtLeastOnce, 1));
        // Subscriber drops off the network (ungraceful) — session persists.
        b.network_disconnect("sub");

        b.connect(connect("pub", true));
        let while_offline = b.handle("pub", publish("home/x", QoS::AtLeastOnce, Some(5), false, b"v"));
        assert!(find_publish_to(&while_offline, "sub").is_none(), "offline: nothing sent");

        // Resume: CONNACK session_present=true, queued PUBLISH delivered.
        let resume = b.connect(sub_connect);
        assert!(resume.iter().any(|a| matches!(a,
            Action::Send { packet: PacketV5::ConnAck(ack), .. } if ack.session_present)));
        let p = find_publish_to(&resume, "sub").expect("queued msg delivered on resume");
        assert_eq!(p.topic, "home/x");
    }

    #[test]
    fn clean_start_discards_prior_session() {
        let mut b = allow_all();
        let mut c = connect("sub", false);
        c.properties = vec![Property::SessionExpiryInterval(3600)];
        b.connect(c);
        b.handle("sub", subscribe("home/#", QoS::AtLeastOnce, 1));
        b.network_disconnect("sub");
        // Reconnect clean: session_present must be false, subscription gone.
        let acts = b.connect(connect("sub", true));
        assert!(acts.iter().any(|a| matches!(a,
            Action::Send { packet: PacketV5::ConnAck(ack), .. } if !ack.session_present)));
    }

    #[test]
    fn last_will_is_published_on_ungraceful_disconnect() {
        let mut b = allow_all();
        b.connect(connect("watcher", true));
        b.handle("watcher", subscribe("home/+/status", QoS::AtMostOnce, 1));

        let mut dier = connect("dier", true);
        dier.will = Some(Will {
            topic: "home/dier/status".into(),
            payload: Bytes::from_static(b"offline"),
            qos: QoS::AtMostOnce,
            retain: false,
            properties: vec![],
        });
        b.connect(dier);

        let acts = b.network_disconnect("dier");
        let p = find_publish_to(&acts, "watcher").expect("will routed");
        assert_eq!(p.topic, "home/dier/status");
        assert_eq!(&p.payload[..], b"offline");
    }

    #[test]
    fn graceful_disconnect_suppresses_the_will() {
        let mut b = allow_all();
        b.connect(connect("watcher", true));
        b.handle("watcher", subscribe("home/+/status", QoS::AtMostOnce, 1));
        let mut dier = connect("dier", true);
        dier.will = Some(Will {
            topic: "home/dier/status".into(),
            payload: Bytes::from_static(b"offline"),
            qos: QoS::AtMostOnce,
            retain: false,
            properties: vec![],
        });
        b.connect(dier);
        // Normal DISCONNECT then network teardown: no will.
        b.handle("dier", PacketV5::Disconnect(DisconnectV5 {
            reason_code: ReasonCode::Success, properties: vec![] }));
        let acts = b.network_disconnect("dier");
        assert!(find_publish_to(&acts, "watcher").is_none());
    }

    #[test]
    fn acl_denied_publish_is_not_routed() {
        let mut auth = Authenticator::default();
        auth.set_anonymous(true);
        auth.set_default_allow(false);
        auth.allow_any(AclAction::Subscribe, "#");
        auth.allow_any(AclAction::Publish, "home/#");
        let mut b = Broker::new(BrokerConfig::default(), auth);
        b.connect(connect("sub", true));
        b.handle("sub", subscribe("#", QoS::AtLeastOnce, 1));
        b.connect(connect("pub", true));
        let acts = b.handle("pub", publish("factory/secret", QoS::AtLeastOnce, Some(3), false, b"x"));
        assert!(find_publish_to(&acts, "sub").is_none(), "denied publish not routed");
        assert!(acts.iter().any(|a| matches!(a,
            Action::Send { packet: PacketV5::PubAck(ack), .. }
            if ack.packet_id == 3 && ack.reason_code == ReasonCode::NotAuthorized)));
    }

    #[test]
    fn no_local_subscription_does_not_echo_to_publisher() {
        let mut b = allow_all();
        b.connect(connect("c", true));
        b.handle("c", PacketV5::Subscribe(SubscribeV5 {
            packet_id: 1,
            properties: vec![],
            subscriptions: vec![SubscriptionV5 {
                topic_filter: "home/#".into(),
                qos: QoS::AtMostOnce,
                no_local: true,
                retain_as_published: false,
                retain_handling: RetainHandling::SendOnSubscribe,
            }],
        }));
        let acts = b.handle("c", publish("home/x", QoS::AtMostOnce, None, false, b"v"));
        assert!(find_publish_to(&acts, "c").is_none(), "no-local must not echo");
    }

    #[test]
    fn second_connection_with_same_id_takes_over() {
        let mut b = allow_all();
        b.connect(connect("dup", true));
        let acts = b.connect(connect("dup", true));
        assert!(acts.iter().any(|a| matches!(a,
            Action::Drop { client_id, reason } if client_id == "dup" && *reason == ReasonCode::SessionTakenOver)));
        assert_eq!(b.session_count(), 1);
    }

    #[test]
    fn ping_is_answered() {
        let mut b = allow_all();
        b.connect(connect("c", true));
        let acts = b.handle("c", PacketV5::PingReq);
        assert!(matches!(sends(&acts)[0].1, PacketV5::PingResp));
    }
}
