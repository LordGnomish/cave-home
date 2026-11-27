//! Pinning tests for the broker decision core: the exact [`Action`]s that
//! `connect`, `handle` and `network_disconnect` return for the three
//! quality-of-service flows, retained messages, session persistence, wills,
//! the ACL and the automation hook.
//!
//! Section numbers refer to OASIS MQTT 5.0. Where a test name says the
//! broker "currently" does something, it records present behaviour that the
//! specification does not ask for, so that a refactor cannot change it
//! unnoticed.

use bytes::Bytes;
use cave_home_mqtt::broker::auth::{AclAction, Authenticator};
use cave_home_mqtt::broker::hooks::{BrokerHook, PublishDecision, PublishEvent};
use cave_home_mqtt::broker::{Action, Broker, BrokerConfig};
use cave_home_mqtt::metrics::BrokerMetrics;
use cave_home_mqtt::packet::QoS;
use cave_home_mqtt::v5::packet::{
    AuthV5, ConnAckV5, ConnectV5, DisconnectV5, PacketV5, PubAckV5, PubCompV5, PubRecV5, PubRelV5,
    PublishV5, RetainHandling, SubAckV5, SubscribeV5, SubscriptionV5, UnsubAckV5, UnsubscribeV5,
    Will,
};
use cave_home_mqtt::v5::property::Property;
use cave_home_mqtt::v5::reason::ReasonCode;
use std::sync::{Arc, Mutex, PoisonError};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ---- builders -------------------------------------------------------------------

fn open_auth() -> Authenticator {
    let mut auth = Authenticator::default();
    auth.set_anonymous(true);
    auth.set_default_allow(true);
    auth
}

/// A broker that lets anonymous clients do everything.
fn broker() -> Broker {
    Broker::new(BrokerConfig::default(), open_auth())
}

fn connect(client_id: &str, clean_start: bool, session_expiry: u32) -> ConnectV5 {
    ConnectV5 {
        client_id: client_id.to_owned(),
        clean_start,
        keep_alive_secs: 0,
        properties: if session_expiry == 0 {
            vec![]
        } else {
            vec![Property::SessionExpiryInterval(session_expiry)]
        },
        will: None,
        username: None,
        password: None,
    }
}

fn login(client_id: &str, username: &str, password: &'static [u8]) -> ConnectV5 {
    ConnectV5 {
        username: Some(username.to_owned()),
        password: Some(Bytes::from_static(password)),
        ..connect(client_id, true, 0)
    }
}

fn sub(filter: &str, qos: QoS) -> SubscriptionV5 {
    SubscriptionV5 {
        topic_filter: filter.to_owned(),
        qos,
        no_local: false,
        retain_as_published: false,
        retain_handling: RetainHandling::SendOnSubscribe,
    }
}

const fn subscribe(packet_id: u16, subscriptions: Vec<SubscriptionV5>) -> PacketV5 {
    PacketV5::Subscribe(SubscribeV5 {
        packet_id,
        properties: vec![],
        subscriptions,
    })
}

/// A PUBLISH as a client sends it.
fn publish(topic: &str, qos: QoS, packet_id: Option<u16>, payload: &'static [u8]) -> PublishV5 {
    PublishV5 {
        topic: topic.to_owned(),
        qos,
        retain: false,
        dup: false,
        packet_id,
        properties: vec![],
        payload: Bytes::from_static(payload),
    }
}

fn retained(topic: &str, qos: QoS, packet_id: Option<u16>, payload: &'static [u8]) -> PacketV5 {
    PacketV5::Publish(PublishV5 {
        retain: true,
        ..publish(topic, qos, packet_id, payload)
    })
}

const fn disconnect(reason_code: ReasonCode) -> PacketV5 {
    PacketV5::Disconnect(DisconnectV5 {
        reason_code,
        properties: vec![],
    })
}

// ---- expected actions -------------------------------------------------------------

fn send(client_id: &str, packet: PacketV5) -> Action {
    Action::Send {
        client_id: client_id.to_owned(),
        packet,
    }
}

fn connack(client_id: &str, session_present: bool) -> Action {
    send(
        client_id,
        PacketV5::ConnAck(ConnAckV5 {
            session_present,
            reason_code: ReasonCode::Success,
            properties: vec![],
        }),
    )
}

fn suback(client_id: &str, packet_id: u16, reason_codes: &[ReasonCode]) -> Action {
    send(
        client_id,
        PacketV5::SubAck(SubAckV5 {
            packet_id,
            properties: vec![],
            reason_codes: reason_codes.to_vec(),
        }),
    )
}

fn puback(client_id: &str, packet_id: u16, reason_code: ReasonCode) -> Action {
    send(
        client_id,
        PacketV5::PubAck(PubAckV5 {
            packet_id,
            reason_code,
            properties: vec![],
        }),
    )
}

fn pubrec(client_id: &str, packet_id: u16, reason_code: ReasonCode) -> Action {
    send(
        client_id,
        PacketV5::PubRec(PubRecV5 {
            packet_id,
            reason_code,
            properties: vec![],
        }),
    )
}

/// A PUBLISH as the broker forwards it to `client_id`.
fn deliver(client_id: &str, message: PublishV5) -> Action {
    send(client_id, PacketV5::Publish(message))
}

/// The `Send` actions ordered by receiving client; fan-out to several
/// sessions has no defined order.
fn by_client(mut actions: Vec<Action>) -> Vec<Action> {
    actions.sort_by_key(|action| match action {
        Action::Send { client_id, .. } | Action::Drop { client_id, .. } => client_id.clone(),
    });
    actions
}

// ---- §3.1 CONNECT / §3.2 CONNACK ---------------------------------------------------

#[test]
fn connect_is_acknowledged_and_creates_a_session() {
    let mut b = broker();
    assert_eq!(b.session_count(), 0);
    assert_eq!(b.connect(connect("c1", true, 0)), [connack("c1", false)]);
    // Nothing to resume without clean start either.
    assert_eq!(b.connect(connect("c2", false, 0)), [connack("c2", false)]);
    assert_eq!(b.session_count(), 2);
    assert_eq!(
        (b.metrics().clients_connected, b.metrics().clients_total),
        (2, 2)
    );
}

#[test]
fn an_empty_client_id_is_replaced_by_a_numbered_one() {
    let assigned = |id: &str| {
        send(
            id,
            PacketV5::ConnAck(ConnAckV5 {
                session_present: false,
                reason_code: ReasonCode::Success,
                properties: vec![Property::AssignedClientIdentifier(id.to_owned())],
            }),
        )
    };
    let mut b = broker();
    // §3.1.3.1 / §3.2.2.3.7: the assigned identifier is reported back.
    assert_eq!(b.connect(connect("", true, 0)), [assigned("auto-1")]);
    assert_eq!(
        b.connect(connect("named", true, 0)),
        [connack("named", false)]
    );
    assert_eq!(b.connect(connect("", false, 0)), [assigned("auto-2")]);
    assert_eq!(b.session_count(), 3);
}

#[test]
fn bad_credentials_are_refused_without_creating_a_session() {
    let refused = |id: &str| {
        [
            send(
                id,
                PacketV5::ConnAck(ConnAckV5 {
                    session_present: false,
                    reason_code: ReasonCode::BadUserNameOrPassword,
                    properties: vec![],
                }),
            ),
            Action::Drop {
                client_id: id.to_owned(),
                reason: ReasonCode::BadUserNameOrPassword,
            },
        ]
    };
    let mut auth = Authenticator::default();
    auth.add_user("admin", b"pw");
    auth.set_default_allow(true);
    let mut b = Broker::new(BrokerConfig::default(), auth);

    assert_eq!(b.connect(login("c1", "admin", b"wrong")), refused("c1"));
    assert_eq!(b.connect(login("c1", "ghost", b"pw")), refused("c1"));
    // Anonymous access is off.
    assert_eq!(b.connect(connect("c1", true, 0)), refused("c1"));
    // Without a client id the refusal is addressed to a placeholder.
    assert_eq!(
        b.connect(login("", "admin", b"wrong")),
        refused("<unauthenticated>")
    );
    assert_eq!(b.session_count(), 0);
    assert_eq!(b.metrics(), &BrokerMetrics::default());

    // A refused CONNECT does not use up an assigned-identifier number.
    assert_eq!(
        b.connect(login("", "admin", b"pw")),
        [send(
            "auto-1",
            PacketV5::ConnAck(ConnAckV5 {
                session_present: false,
                reason_code: ReasonCode::Success,
                properties: vec![Property::AssignedClientIdentifier("auto-1".to_owned())],
            }),
        )]
    );
    assert_eq!(
        b.connect(login("c1", "admin", b"pw")),
        [connack("c1", false)]
    );
}

#[test]
fn a_second_connection_with_the_same_client_id_takes_the_session_over() {
    let taken_over = Action::Drop {
        client_id: "dup".to_owned(),
        reason: ReasonCode::SessionTakenOver,
    };
    let mut b = broker();
    assert_eq!(b.connect(connect("dup", true, 0)), [connack("dup", false)]);
    // §3.1.4.3: the old connection is dropped first, then the new one is
    // acknowledged.
    assert_eq!(
        b.connect(connect("dup", true, 0)),
        [taken_over.clone(), connack("dup", false)]
    );
    // Without clean start the session of the live connection is resumed.
    assert_eq!(
        b.connect(connect("dup", false, 0)),
        [taken_over, connack("dup", true)]
    );
    assert_eq!(b.session_count(), 1);
}

#[test]
fn a_takeover_currently_counts_as_one_more_connected_client() {
    let mut b = broker();
    b.connect(connect("dup", true, 0));
    b.connect(connect("dup", true, 0));
    assert_eq!(
        (
            b.session_count(),
            b.metrics().clients_connected,
            b.metrics().clients_total
        ),
        (1, 2, 2)
    );
    assert_eq!(b.network_disconnect("dup"), []);
    assert_eq!((b.session_count(), b.metrics().clients_connected), (0, 1));
}

// ---- §4.1 session state --------------------------------------------------------------

#[test]
fn a_session_without_expiry_ends_with_the_connection() {
    let mut b = broker();
    b.connect(connect("net", true, 0));
    b.connect(connect("bye", true, 0));
    assert_eq!(b.network_disconnect("net"), []);
    assert_eq!(b.session_count(), 1);
    assert_eq!(b.handle("bye", disconnect(ReasonCode::Success)), []);
    assert_eq!(b.session_count(), 0);
    // Tearing the connection down afterwards, or twice, changes nothing.
    assert_eq!(b.network_disconnect("bye"), []);
    assert_eq!(b.network_disconnect("net"), []);
    assert_eq!(b.network_disconnect("never-connected"), []);
    assert_eq!(
        (b.metrics().clients_connected, b.metrics().clients_total),
        (0, 2)
    );
}

#[test]
fn a_persistent_session_queues_qos1_and_qos2_while_offline() {
    let mut b = broker();
    assert_eq!(
        b.connect(connect("sub", false, 3600)),
        [connack("sub", false)]
    );
    assert_eq!(
        b.handle("sub", subscribe(1, vec![sub("home/#", QoS::AtLeastOnce)])),
        [suback("sub", 1, &[ReasonCode::GrantedQoS1])]
    );
    assert_eq!(b.network_disconnect("sub"), []);
    assert_eq!(b.session_count(), 1);

    b.connect(connect("pub", true, 0));
    // Queued messages count as delivered: the publisher sees Success.
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("home/a", QoS::AtLeastOnce, Some(5), b"one"))
        ),
        [puback("pub", 5, ReasonCode::Success)]
    );
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("home/b", QoS::ExactlyOnce, Some(6), b"two"))
        ),
        [pubrec("pub", 6, ReasonCode::Success)]
    );
    // QoS 0 is not queued for an offline session.
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("home/c", QoS::AtMostOnce, None, b"zero"))
        ),
        []
    );
    // All three are currently counted as sent, the dropped QoS 0 included.
    assert_eq!((b.metrics().messages_sent, b.metrics().bytes_sent), (3, 10));

    // §3.2.2.1.1 Session Present, then the queue in order, at the granted
    // QoS, with the identifiers assigned when the messages were queued.
    assert_eq!(
        b.connect(connect("sub", false, 3600)),
        [
            connack("sub", true),
            deliver("sub", publish("home/a", QoS::AtLeastOnce, Some(1), b"one")),
            deliver("sub", publish("home/b", QoS::AtLeastOnce, Some(2), b"two")),
        ]
    );
    // The queue has been drained.
    assert_eq!(b.network_disconnect("sub"), []);
    assert_eq!(
        b.connect(connect("sub", false, 3600)),
        [connack("sub", true)]
    );
}

#[test]
fn clean_start_discards_the_stored_session() {
    let mut b = broker();
    b.connect(connect("sub", false, 3600));
    b.handle("sub", subscribe(1, vec![sub("home/#", QoS::AtLeastOnce)]));
    b.network_disconnect("sub");
    b.connect(connect("pub", true, 0));
    b.handle(
        "pub",
        PacketV5::Publish(publish("home/a", QoS::AtLeastOnce, Some(1), b"x")),
    );

    // §3.1.2.4: no Session Present, no queued message, no subscription.
    assert_eq!(
        b.connect(connect("sub", true, 3600)),
        [connack("sub", false)]
    );
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("home/a", QoS::AtLeastOnce, Some(2), b"x"))
        ),
        [puback("pub", 2, ReasonCode::NoMatchingSubscribers)]
    );
}

#[test]
fn the_session_expiry_of_the_latest_connect_applies() {
    let mut b = broker();
    b.connect(connect("c", false, 3600));
    b.network_disconnect("c");
    assert_eq!(b.session_count(), 1);
    // Resumed with no expiry: the session now ends with the connection.
    assert_eq!(b.connect(connect("c", false, 0)), [connack("c", true)]);
    b.network_disconnect("c");
    assert_eq!(b.session_count(), 0);
}

// ---- §3.8 SUBSCRIBE / §3.10 UNSUBSCRIBE ---------------------------------------------

#[test]
fn suback_reports_one_reason_code_per_filter_in_order() {
    let mut auth = Authenticator::default();
    auth.set_anonymous(true);
    auth.allow_any(AclAction::Subscribe, "home/#");
    auth.allow_any(AclAction::Publish, "#");
    let config = BrokerConfig {
        max_qos: QoS::AtLeastOnce,
        retain_available: true,
    };
    let mut b = Broker::new(config, auth);
    b.connect(connect("c", true, 0));
    assert_eq!(
        b.handle(
            "c",
            subscribe(
                7,
                vec![
                    sub("home/+/temp", QoS::ExactlyOnce),
                    sub("home/#/temp", QoS::AtLeastOnce),
                    sub("secret/x", QoS::AtMostOnce),
                    sub("home/x", QoS::AtMostOnce),
                    sub("home/y", QoS::AtLeastOnce),
                ]
            )
        ),
        [suback(
            "c",
            7,
            &[
                // Granted QoS is capped by the broker's maximum.
                ReasonCode::GrantedQoS1,
                ReasonCode::TopicFilterInvalid,
                ReasonCode::NotAuthorized,
                ReasonCode::Success,
                ReasonCode::GrantedQoS1,
            ]
        )]
    );
    assert!(b.prometheus().contains("\nmqtt_subscriptions 3\n"));

    // The subscription is stored with the granted QoS.
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("home/loft/temp", QoS::ExactlyOnce, Some(9), b"21"))
        ),
        [
            deliver(
                "c",
                publish("home/loft/temp", QoS::AtLeastOnce, Some(1), b"21")
            ),
            pubrec("c", 9, ReasonCode::Success),
        ]
    );
}

#[test]
fn unsuback_reports_whether_each_filter_was_subscribed() {
    let mut b = broker();
    b.connect(connect("c", true, 0));
    b.handle(
        "c",
        subscribe(
            1,
            vec![sub("a/#", QoS::AtMostOnce), sub("b", QoS::AtMostOnce)],
        ),
    );
    let unsubscribe = |packet_id: u16, filters: &[&str]| {
        PacketV5::Unsubscribe(UnsubscribeV5 {
            packet_id,
            properties: vec![],
            topic_filters: filters.iter().map(|f| (*f).to_owned()).collect(),
        })
    };
    let unsuback = |client: &str, packet_id: u16, reason_codes: &[ReasonCode]| {
        send(
            client,
            PacketV5::UnsubAck(UnsubAckV5 {
                packet_id,
                properties: vec![],
                reason_codes: reason_codes.to_vec(),
            }),
        )
    };
    assert_eq!(
        b.handle("c", unsubscribe(2, &["a/#", "a/+", "a/#"])),
        [unsuback(
            "c",
            2,
            &[
                ReasonCode::Success,
                ReasonCode::NoSubscriptionExisted,
                ReasonCode::NoSubscriptionExisted,
            ]
        )]
    );
    // Only "b" is left.
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("a/x", QoS::AtMostOnce, None, b"x"))
        ),
        []
    );
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("b", QoS::AtMostOnce, None, b"x"))
        ),
        [deliver("c", publish("b", QoS::AtMostOnce, None, b"x"))]
    );
    // A client the broker has no session for is answered all the same.
    assert_eq!(
        b.handle("ghost", unsubscribe(3, &["b"])),
        [unsuback("ghost", 3, &[ReasonCode::NoSubscriptionExisted])]
    );
    assert_eq!(
        b.handle("ghost", subscribe(4, vec![sub("b", QoS::ExactlyOnce)])),
        [suback("ghost", 4, &[ReasonCode::GrantedQoS2])]
    );
}

// ---- §3.3.1.3 retained messages ------------------------------------------------------

#[test]
fn retained_messages_follow_the_retain_handling_option() {
    let with_handling = |filter: &str, qos: QoS, retain_handling: RetainHandling| SubscriptionV5 {
        retain_handling,
        ..sub(filter, qos)
    };
    let stored = |qos: QoS, packet_id: Option<u16>| PublishV5 {
        retain: true,
        properties: vec![Property::ContentType("text/plain".to_owned())],
        ..publish("home/t", qos, packet_id, b"21")
    };
    let mut b = broker();
    b.connect(connect("pub", true, 0));
    assert_eq!(
        b.handle("pub", PacketV5::Publish(stored(QoS::AtLeastOnce, Some(1)))),
        [puback("pub", 1, ReasonCode::NoMatchingSubscribers)]
    );
    assert_eq!(b.retained_count(), 1);

    b.connect(connect("sub", true, 0));
    // Retain Handling 0: sent on every SUBSCRIBE, after the SUBACK, with
    // RETAIN set, its properties, and the lower of the two QoS levels.
    assert_eq!(
        b.handle("sub", subscribe(2, vec![sub("home/#", QoS::ExactlyOnce)])),
        [
            suback("sub", 2, &[ReasonCode::GrantedQoS2]),
            deliver("sub", stored(QoS::AtLeastOnce, Some(1))),
        ]
    );
    // Retain Handling 1: not sent again for an existing subscription ...
    assert_eq!(
        b.handle(
            "sub",
            subscribe(
                3,
                vec![with_handling(
                    "home/#",
                    QoS::AtMostOnce,
                    RetainHandling::SendIfNew
                )]
            )
        ),
        [suback("sub", 3, &[ReasonCode::Success])]
    );
    // ... but sent for a new one.
    assert_eq!(
        b.handle(
            "sub",
            subscribe(
                4,
                vec![with_handling(
                    "home/t",
                    QoS::AtMostOnce,
                    RetainHandling::SendIfNew
                )]
            )
        ),
        [
            suback("sub", 4, &[ReasonCode::Success]),
            deliver("sub", stored(QoS::AtMostOnce, None)),
        ]
    );
    // Retain Handling 2: never sent.
    assert_eq!(
        b.handle(
            "sub",
            subscribe(
                5,
                vec![with_handling(
                    "home/+",
                    QoS::AtLeastOnce,
                    RetainHandling::DoNotSend
                )]
            )
        ),
        [suback("sub", 5, &[ReasonCode::GrantedQoS1])]
    );
    assert_eq!(
        b.handle("sub", subscribe(6, vec![sub("home/#", QoS::AtLeastOnce)])),
        [
            suback("sub", 6, &[ReasonCode::GrantedQoS1]),
            deliver("sub", stored(QoS::AtLeastOnce, Some(2))),
        ]
    );
}

#[test]
fn every_matching_retained_message_is_sent_once_per_filter() -> TestResult {
    let mut b = broker();
    b.connect(connect("pub", true, 0));
    b.handle("pub", retained("home/a", QoS::AtLeastOnce, Some(1), b"A"));
    b.handle("pub", retained("home/b", QoS::AtLeastOnce, Some(2), b"B"));
    b.handle("pub", retained("garden/c", QoS::AtLeastOnce, Some(3), b"C"));
    assert_eq!(b.retained_count(), 3);

    b.connect(connect("sub", true, 0));
    let actions = b.handle("sub", subscribe(9, vec![sub("home/+", QoS::AtLeastOnce)]));
    let (first, rest) = actions.split_first().ok_or("no SUBACK")?;
    assert_eq!(first, &suback("sub", 9, &[ReasonCode::GrantedQoS1]));
    // The two messages come in no particular order, and so do their ids.
    let mut messages = Vec::new();
    for action in rest {
        let Action::Send {
            client_id,
            packet: PacketV5::Publish(p),
        } = action
        else {
            return Err(format!("not a PUBLISH: {action:?}").into());
        };
        assert_eq!(client_id, "sub");
        assert_eq!((p.qos, p.retain, p.dup), (QoS::AtLeastOnce, true, false));
        messages.push((p.topic.as_str(), &p.payload[..], p.packet_id));
    }
    messages.sort_unstable();
    let mut ids: Vec<Option<u16>> = messages.iter().map(|m| m.2).collect();
    ids.sort_unstable();
    assert_eq!(ids, [Some(1), Some(2)]);
    let topics: Vec<(&str, &[u8])> = messages.iter().map(|m| (m.0, m.1)).collect();
    assert_eq!(topics, [("home/a", &b"A"[..]), ("home/b", &b"B"[..])]);
    Ok(())
}

#[test]
fn an_empty_retained_publish_clears_the_topic_and_is_still_routed() {
    let mut b = broker();
    b.connect(connect("c", true, 0));
    b.handle("c", subscribe(1, vec![sub("home/#", QoS::AtMostOnce)]));
    b.handle("c", retained("home/t", QoS::AtMostOnce, None, b"21"));
    assert_eq!(b.retained_count(), 1);
    assert_eq!(
        b.handle("c", retained("home/t", QoS::AtMostOnce, None, b"")),
        [deliver("c", publish("home/t", QoS::AtMostOnce, None, b""))]
    );
    assert_eq!(b.retained_count(), 0);
}

#[test]
fn live_publishes_keep_retain_only_for_retain_as_published_subscriptions() {
    let mut b = broker();
    b.connect(connect("plain", true, 0));
    b.connect(connect("rap", true, 0));
    b.handle("plain", subscribe(1, vec![sub("home/#", QoS::AtMostOnce)]));
    b.handle(
        "rap",
        subscribe(
            1,
            vec![SubscriptionV5 {
                retain_as_published: true,
                ..sub("home/#", QoS::AtMostOnce)
            }],
        ),
    );
    b.connect(connect("pub", true, 0));
    // §3.8.3.1 Retain As Published / §3.3.1.3.
    assert_eq!(
        by_client(b.handle("pub", retained("home/t", QoS::AtMostOnce, None, b"21"))),
        [
            deliver("plain", publish("home/t", QoS::AtMostOnce, None, b"21")),
            deliver(
                "rap",
                PublishV5 {
                    retain: true,
                    ..publish("home/t", QoS::AtMostOnce, None, b"21")
                }
            ),
        ]
    );
    // A publish without RETAIN stays without it.
    assert_eq!(
        by_client(b.handle(
            "pub",
            PacketV5::Publish(publish("home/t", QoS::AtMostOnce, None, b"22"))
        )),
        [
            deliver("plain", publish("home/t", QoS::AtMostOnce, None, b"22")),
            deliver("rap", publish("home/t", QoS::AtMostOnce, None, b"22")),
        ]
    );
}

#[test]
fn a_broker_without_retain_support_routes_but_does_not_store() {
    let config = BrokerConfig {
        max_qos: QoS::ExactlyOnce,
        retain_available: false,
    };
    let mut b = Broker::new(config, open_auth());
    b.connect(connect("c", true, 0));
    b.handle("c", subscribe(1, vec![sub("home/#", QoS::AtMostOnce)]));
    assert_eq!(
        b.handle("c", retained("home/t", QoS::AtMostOnce, None, b"21")),
        [deliver(
            "c",
            publish("home/t", QoS::AtMostOnce, None, b"21")
        )]
    );
    assert_eq!(b.retained_count(), 0);

    // The same goes for a retained will.
    b.connect(ConnectV5 {
        will: Some(Will {
            topic: "home/w".to_owned(),
            payload: Bytes::from_static(b"gone"),
            qos: QoS::AtMostOnce,
            retain: true,
            properties: vec![],
        }),
        ..connect("dier", true, 0)
    });
    assert_eq!(
        b.network_disconnect("dier"),
        [deliver(
            "c",
            publish("home/w", QoS::AtMostOnce, None, b"gone")
        )]
    );
    assert_eq!(b.retained_count(), 0);
}

// ---- §3.3 PUBLISH routing -------------------------------------------------------------

#[test]
fn a_publish_is_delivered_before_its_acknowledgement() {
    let mut b = broker();
    b.connect(connect("sub", true, 0));
    b.handle("sub", subscribe(1, vec![sub("home/#", QoS::ExactlyOnce)]));
    b.connect(connect("pub", true, 0));

    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("home/x", QoS::AtMostOnce, None, b"0"))
        ),
        [deliver(
            "sub",
            publish("home/x", QoS::AtMostOnce, None, b"0")
        )]
    );
    // The subscriber's packet identifiers are the broker's own, counting up.
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("home/x", QoS::AtLeastOnce, Some(500), b"1"))
        ),
        [
            deliver("sub", publish("home/x", QoS::AtLeastOnce, Some(1), b"1")),
            puback("pub", 500, ReasonCode::Success),
        ]
    );
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("home/x", QoS::ExactlyOnce, Some(501), b"2"))
        ),
        [
            deliver("sub", publish("home/x", QoS::ExactlyOnce, Some(2), b"2")),
            pubrec("pub", 501, ReasonCode::Success),
        ]
    );
    // Nobody listens on this topic.
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("garden/x", QoS::AtMostOnce, None, b"0"))
        ),
        []
    );
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("garden/x", QoS::AtLeastOnce, Some(502), b"1"))
        ),
        [puback("pub", 502, ReasonCode::NoMatchingSubscribers)]
    );
    assert_eq!(
        b.handle(
            "pub",
            PacketV5::Publish(publish("garden/x", QoS::ExactlyOnce, Some(503), b"2"))
        ),
        [pubrec("pub", 503, ReasonCode::NoMatchingSubscribers)]
    );
}

#[test]
fn the_delivery_qos_is_the_lower_of_publish_and_subscription() {
    let mut b = broker();
    b.connect(connect("q0", true, 0));
    b.connect(connect("q1", true, 0));
    b.connect(connect("q2", true, 0));
    b.handle("q0", subscribe(1, vec![sub("t", QoS::AtMostOnce)]));
    b.handle("q1", subscribe(1, vec![sub("t", QoS::AtLeastOnce)]));
    b.handle("q2", subscribe(1, vec![sub("t", QoS::ExactlyOnce)]));
    b.connect(connect("pub", true, 0));

    let fan_out = |b: &mut Broker, qos: QoS, packet_id: Option<u16>| {
        by_client(b.handle("pub", PacketV5::Publish(publish("t", qos, packet_id, b"x"))))
    };
    assert_eq!(
        fan_out(&mut b, QoS::AtMostOnce, None),
        [
            deliver("q0", publish("t", QoS::AtMostOnce, None, b"x")),
            deliver("q1", publish("t", QoS::AtMostOnce, None, b"x")),
            deliver("q2", publish("t", QoS::AtMostOnce, None, b"x")),
        ]
    );
    assert_eq!(
        fan_out(&mut b, QoS::AtLeastOnce, Some(7)),
        [
            puback("pub", 7, ReasonCode::Success),
            deliver("q0", publish("t", QoS::AtMostOnce, None, b"x")),
            deliver("q1", publish("t", QoS::AtLeastOnce, Some(1), b"x")),
            deliver("q2", publish("t", QoS::AtLeastOnce, Some(1), b"x")),
        ]
    );
    assert_eq!(
        fan_out(&mut b, QoS::ExactlyOnce, Some(8)),
        [
            pubrec("pub", 8, ReasonCode::Success),
            deliver("q0", publish("t", QoS::AtMostOnce, None, b"x")),
            deliver("q1", publish("t", QoS::AtLeastOnce, Some(2), b"x")),
            deliver("q2", publish("t", QoS::ExactlyOnce, Some(2), b"x")),
        ]
    );
    assert_eq!(
        (
            b.metrics().messages_received,
            b.metrics().messages_sent,
            b.metrics().bytes_sent
        ),
        (3, 9, 9)
    );
}

#[test]
fn overlapping_subscriptions_deliver_once_at_the_highest_matching_qos() {
    let mut b = broker();
    b.connect(connect("c", true, 0));
    b.handle(
        "c",
        subscribe(
            1,
            vec![
                sub("home/#", QoS::AtMostOnce),
                sub("home/+", QoS::ExactlyOnce),
                sub("home/x", QoS::AtLeastOnce),
                sub("garden/#", QoS::ExactlyOnce),
            ],
        ),
    );
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("home/x", QoS::ExactlyOnce, Some(7), b"v"))
        ),
        [
            deliver("c", publish("home/x", QoS::ExactlyOnce, Some(1), b"v")),
            pubrec("c", 7, ReasonCode::Success),
        ]
    );
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("home/x/y", QoS::ExactlyOnce, Some(8), b"v"))
        ),
        [
            deliver("c", publish("home/x/y", QoS::AtMostOnce, None, b"v")),
            pubrec("c", 8, ReasonCode::Success),
        ]
    );
}

#[test]
fn no_local_keeps_a_clients_own_publishes_away_from_it() {
    let mut b = broker();
    b.connect(connect("c", true, 0));
    b.connect(connect("other", true, 0));
    b.handle(
        "c",
        subscribe(
            1,
            vec![SubscriptionV5 {
                no_local: true,
                ..sub("home/#", QoS::AtLeastOnce)
            }],
        ),
    );
    // §3.8.3.1: not echoed, and therefore no matching subscriber.
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("home/x", QoS::AtLeastOnce, Some(1), b"v"))
        ),
        [puback("c", 1, ReasonCode::NoMatchingSubscribers)]
    );
    assert_eq!(
        b.handle(
            "other",
            PacketV5::Publish(publish("home/x", QoS::AtLeastOnce, Some(1), b"v"))
        ),
        [
            deliver("c", publish("home/x", QoS::AtLeastOnce, Some(1), b"v")),
            puback("other", 1, ReasonCode::Success),
        ]
    );
}

#[test]
fn message_properties_and_payload_are_forwarded_unchanged() {
    let properties = vec![
        Property::PayloadFormatIndicator(1),
        Property::UserProperty("k".to_owned(), "v".to_owned()),
        Property::TopicAlias(4),
    ];
    let mut b = broker();
    b.connect(connect("c", true, 0));
    b.handle("c", subscribe(1, vec![sub("#", QoS::AtMostOnce)]));
    let message = PublishV5 {
        properties,
        // DUP is per hop and is not forwarded.
        dup: true,
        ..publish("home/x", QoS::AtMostOnce, None, b"\x00\xff payload")
    };
    assert_eq!(
        b.handle("c", PacketV5::Publish(message.clone())),
        [deliver(
            "c",
            PublishV5 {
                dup: false,
                ..message
            }
        )]
    );
}

#[test]
fn a_publish_to_an_invalid_topic_name_is_refused_before_anything_else() {
    let mut b = broker();
    b.connect(connect("c", true, 0));
    b.handle("c", subscribe(1, vec![sub("#", QoS::AtMostOnce)]));
    for topic in ["", "home/+", "home/#", "a\0b"] {
        assert_eq!(
            b.handle(
                "c",
                PacketV5::Publish(publish(topic, QoS::AtMostOnce, None, b"x"))
            ),
            [],
            "{topic:?}"
        );
        assert_eq!(
            b.handle(
                "c",
                PacketV5::Publish(publish(topic, QoS::AtLeastOnce, Some(1), b"x"))
            ),
            [puback("c", 1, ReasonCode::TopicNameInvalid)],
            "{topic:?}"
        );
        assert_eq!(
            b.handle(
                "c",
                PacketV5::Publish(publish(topic, QoS::ExactlyOnce, Some(2), b"x"))
            ),
            [pubrec("c", 2, ReasonCode::TopicNameInvalid)],
            "{topic:?}"
        );
        assert_eq!(
            b.handle("c", retained(topic, QoS::AtMostOnce, None, b"x")),
            []
        );
    }
    // Neither counted as received nor stored.
    assert_eq!(b.metrics().messages_received, 0);
    assert_eq!(b.retained_count(), 0);
}

#[test]
fn a_publish_above_qos0_without_packet_id_is_routed_but_not_acknowledged() {
    let mut b = broker();
    b.connect(connect("c", true, 0));
    b.handle("c", subscribe(1, vec![sub("#", QoS::AtMostOnce)]));
    for qos in [QoS::AtLeastOnce, QoS::ExactlyOnce] {
        assert_eq!(
            b.handle("c", PacketV5::Publish(publish("t", qos, None, b"x"))),
            [deliver("c", publish("t", QoS::AtMostOnce, None, b"x"))]
        );
    }
}

// ---- §4.3 QoS 1 and QoS 2 acknowledgements ------------------------------------------

#[test]
fn a_qos2_publish_is_routed_once_until_its_pubrel() {
    let pubrel = |packet_id: u16| {
        PacketV5::PubRel(PubRelV5 {
            packet_id,
            reason_code: ReasonCode::Success,
            properties: vec![],
        })
    };
    let pubcomp = |client: &str, packet_id: u16| {
        send(
            client,
            PacketV5::PubComp(PubCompV5 {
                packet_id,
                reason_code: ReasonCode::Success,
                properties: vec![],
            }),
        )
    };
    let mut b = broker();
    b.connect(connect("sub", true, 0));
    b.handle("sub", subscribe(1, vec![sub("t", QoS::AtMostOnce)]));
    b.connect(connect("pub", true, 0));
    let message = || PacketV5::Publish(publish("t", QoS::ExactlyOnce, Some(7), b"x"));
    let forwarded = || deliver("sub", publish("t", QoS::AtMostOnce, None, b"x"));

    assert_eq!(
        b.handle("pub", message()),
        [forwarded(), pubrec("pub", 7, ReasonCode::Success)]
    );
    // §4.3.3: a repeated PUBLISH with the same identifier is only
    // acknowledged again.
    assert_eq!(
        b.handle("pub", message()),
        [pubrec("pub", 7, ReasonCode::Success)]
    );
    assert_eq!(b.handle("pub", pubrel(7)), [pubcomp("pub", 7)]);
    // The identifier is free again: this is a new message.
    assert_eq!(
        b.handle("pub", message()),
        [forwarded(), pubrec("pub", 7, ReasonCode::Success)]
    );
    // PUBREL is answered for identifiers and clients the broker never saw.
    assert_eq!(b.handle("pub", pubrel(99)), [pubcomp("pub", 99)]);
    assert_eq!(b.handle("ghost", pubrel(1)), [pubcomp("ghost", 1)]);
    // The duplicate was counted as received but not sent on.
    assert_eq!(
        (b.metrics().messages_received, b.metrics().messages_sent),
        (3, 2)
    );
}

#[test]
fn a_repeated_qos2_publish_is_acknowledged_with_success_even_without_subscribers() {
    let mut b = broker();
    b.connect(connect("pub", true, 0));
    let message = || PacketV5::Publish(publish("t", QoS::ExactlyOnce, Some(7), b"x"));
    assert_eq!(
        b.handle("pub", message()),
        [pubrec("pub", 7, ReasonCode::NoMatchingSubscribers)]
    );
    assert_eq!(
        b.handle("pub", message()),
        [pubrec("pub", 7, ReasonCode::Success)]
    );
}

#[test]
fn acknowledgements_from_subscribers_are_answered_per_the_qos2_flow() {
    let ack = |packet_id: u16| PubAckV5 {
        packet_id,
        reason_code: ReasonCode::Success,
        properties: vec![],
    };
    let pubrec_from_client = |packet_id: u16| {
        PacketV5::PubRec(PubRecV5 {
            packet_id,
            reason_code: ReasonCode::Success,
            properties: vec![],
        })
    };
    let pubrel = |client: &str, packet_id: u16| {
        send(
            client,
            PacketV5::PubRel(PubRelV5 {
                packet_id,
                reason_code: ReasonCode::Success,
                properties: vec![],
            }),
        )
    };
    let pubcomp_from_client = |packet_id: u16| {
        PacketV5::PubComp(PubCompV5 {
            packet_id,
            reason_code: ReasonCode::Success,
            properties: vec![],
        })
    };
    let mut b = broker();
    b.connect(connect("sub", true, 0));
    b.handle("sub", subscribe(1, vec![sub("t", QoS::ExactlyOnce)]));
    b.handle(
        "sub",
        PacketV5::Publish(publish("t", QoS::ExactlyOnce, Some(40), b"x")),
    );

    // PUBACK and PUBCOMP end a flow: nothing is sent back.
    assert_eq!(b.handle("sub", PacketV5::PubAck(ack(1))), []);
    assert_eq!(b.handle("sub", pubcomp_from_client(1)), []);
    // PUBREC is always answered with PUBREL, known identifier or not.
    assert_eq!(b.handle("sub", pubrec_from_client(1)), [pubrel("sub", 1)]);
    assert_eq!(b.handle("sub", pubrec_from_client(1)), [pubrel("sub", 1)]);
    assert_eq!(b.handle("sub", pubrec_from_client(77)), [pubrel("sub", 77)]);
    assert_eq!(
        b.handle("ghost", pubrec_from_client(5)),
        [pubrel("ghost", 5)]
    );
    assert_eq!(b.handle("ghost", PacketV5::PubAck(ack(5))), []);
    assert_eq!(b.handle("ghost", pubcomp_from_client(5)), []);
}

#[test]
fn ping_is_answered_and_server_bound_strays_are_ignored() {
    let mut b = broker();
    b.connect(connect("c", true, 0));
    assert_eq!(
        b.handle("c", PacketV5::PingReq),
        [send("c", PacketV5::PingResp)]
    );
    assert_eq!(
        b.handle("ghost", PacketV5::PingReq),
        [send("ghost", PacketV5::PingResp)]
    );
    let strays = [
        PacketV5::Connect(connect("c", true, 0)),
        PacketV5::ConnAck(ConnAckV5 {
            session_present: false,
            reason_code: ReasonCode::Success,
            properties: vec![],
        }),
        PacketV5::SubAck(SubAckV5 {
            packet_id: 1,
            properties: vec![],
            reason_codes: vec![],
        }),
        PacketV5::UnsubAck(UnsubAckV5 {
            packet_id: 1,
            properties: vec![],
            reason_codes: vec![],
        }),
        PacketV5::PingResp,
        PacketV5::Auth(AuthV5 {
            reason_code: ReasonCode::Success,
            properties: vec![],
        }),
    ];
    for packet in strays {
        assert_eq!(b.handle("c", packet), []);
    }
    assert_eq!(b.session_count(), 1);
}

// ---- access control ------------------------------------------------------------------

#[test]
fn the_acl_is_applied_with_the_user_name_of_the_connection() {
    let mut auth = Authenticator::default();
    auth.add_user("alice", b"a");
    auth.add_user("bob", b"b");
    auth.allow("alice", AclAction::Publish, "home/#");
    auth.allow_any(AclAction::Subscribe, "#");
    let mut b = Broker::new(BrokerConfig::default(), auth);
    b.connect(login("a", "alice", b"a"));
    b.connect(login("b", "bob", b"b"));
    assert_eq!(
        b.handle("b", subscribe(1, vec![sub("#", QoS::AtMostOnce)])),
        [suback("b", 1, &[ReasonCode::Success])]
    );

    assert_eq!(
        b.handle(
            "a",
            PacketV5::Publish(publish("home/x", QoS::AtLeastOnce, Some(1), b"v"))
        ),
        [
            deliver("b", publish("home/x", QoS::AtMostOnce, None, b"v")),
            puback("a", 1, ReasonCode::Success),
        ]
    );
    // Bob may not publish; a refused QoS 0 publish is dropped silently.
    assert_eq!(
        b.handle(
            "b",
            PacketV5::Publish(publish("home/x", QoS::AtMostOnce, None, b"v"))
        ),
        []
    );
    assert_eq!(
        b.handle(
            "b",
            PacketV5::Publish(publish("home/x", QoS::AtLeastOnce, Some(2), b"v"))
        ),
        [puback("b", 2, ReasonCode::NotAuthorized)]
    );
    assert_eq!(
        b.handle(
            "b",
            PacketV5::Publish(publish("home/x", QoS::ExactlyOnce, Some(3), b"v"))
        ),
        [pubrec("b", 3, ReasonCode::NotAuthorized)]
    );
    // A refused retained publish is not stored.
    assert_eq!(
        b.handle("b", retained("home/x", QoS::AtMostOnce, None, b"v")),
        []
    );
    assert_eq!(b.retained_count(), 0);
    assert_eq!(
        (
            b.metrics().messages_received,
            b.metrics().messages_dropped,
            b.metrics().messages_sent
        ),
        (5, 4, 1)
    );
}

// ---- wills (§3.1.2.5, §3.14) -----------------------------------------------------------

fn dier(will_qos: QoS, will_retain: bool) -> ConnectV5 {
    ConnectV5 {
        will: Some(Will {
            topic: "home/dier/status".to_owned(),
            payload: Bytes::from_static(b"offline"),
            qos: will_qos,
            retain: will_retain,
            properties: vec![Property::ContentType("text/plain".to_owned())],
        }),
        ..connect("dier", true, 0)
    }
}

/// The will of [`dier`] as it reaches an at-least-once subscriber.
fn will_message(packet_id: u16) -> PublishV5 {
    PublishV5 {
        properties: vec![Property::ContentType("text/plain".to_owned())],
        ..publish(
            "home/dier/status",
            QoS::AtLeastOnce,
            Some(packet_id),
            b"offline",
        )
    }
}

fn watched_broker() -> Broker {
    let mut b = broker();
    b.connect(connect("watcher", true, 0));
    b.handle(
        "watcher",
        subscribe(1, vec![sub("home/+/status", QoS::AtLeastOnce)]),
    );
    b
}

#[test]
fn the_will_is_published_when_the_network_connection_is_lost() {
    let mut b = watched_broker();
    b.connect(dier(QoS::ExactlyOnce, true));
    assert_eq!(
        b.network_disconnect("dier"),
        [deliver("watcher", will_message(1))]
    );
    // A retained will is stored like a retained publish ...
    assert_eq!(b.retained_count(), 1);
    assert_eq!(
        b.handle(
            "watcher",
            subscribe(2, vec![sub("home/#", QoS::AtMostOnce)])
        ),
        [
            suback("watcher", 2, &[ReasonCode::Success]),
            deliver(
                "watcher",
                PublishV5 {
                    retain: true,
                    qos: QoS::AtMostOnce,
                    packet_id: None,
                    ..will_message(0)
                }
            ),
        ]
    );
    // ... and published only once.
    assert_eq!(b.network_disconnect("dier"), []);
}

#[test]
fn a_normal_disconnect_discards_the_will() {
    let mut b = watched_broker();
    b.connect(dier(QoS::AtLeastOnce, false));
    assert_eq!(b.handle("dier", disconnect(ReasonCode::Success)), []);
    assert_eq!(b.network_disconnect("dier"), []);
    assert_eq!(b.retained_count(), 0);
}

#[test]
fn disconnect_with_will_keeps_the_will_for_the_network_teardown() {
    let mut b = watched_broker();
    b.connect(dier(QoS::AtLeastOnce, false));
    // §3.14.2.1 reason 0x04: nothing is published by the DISCONNECT itself.
    assert_eq!(
        b.handle("dier", disconnect(ReasonCode::DisconnectWithWill)),
        []
    );
    assert_eq!(b.session_count(), 1);
    assert_eq!(
        b.network_disconnect("dier"),
        [deliver("watcher", will_message(1))]
    );
}

#[test]
fn a_disconnect_with_any_other_reason_currently_discards_the_will_too() {
    for reason in [
        ReasonCode::UnspecifiedError,
        ReasonCode::ProtocolError,
        ReasonCode::KeepAliveTimeout,
        ReasonCode::SessionTakenOver,
    ] {
        let mut b = watched_broker();
        b.connect(dier(QoS::AtLeastOnce, false));
        assert_eq!(b.handle("dier", disconnect(reason)), [], "{reason:?}");
        assert_eq!(b.network_disconnect("dier"), [], "{reason:?}");
    }
}

#[test]
fn a_reconnect_replaces_or_removes_the_will() {
    let mut b = watched_broker();
    b.connect(dier(QoS::AtLeastOnce, false));
    // Taken over by a connection without a will.
    b.connect(connect("dier", true, 0));
    assert_eq!(b.network_disconnect("dier"), []);

    b.connect(dier(QoS::AtLeastOnce, false));
    b.connect(ConnectV5 {
        will: Some(Will {
            topic: "home/dier/status".to_owned(),
            payload: Bytes::from_static(b"second"),
            qos: QoS::AtMostOnce,
            retain: false,
            properties: vec![],
        }),
        ..connect("dier", true, 0)
    });
    assert_eq!(
        b.network_disconnect("dier"),
        [deliver(
            "watcher",
            publish("home/dier/status", QoS::AtMostOnce, None, b"second")
        )]
    );
}

// ---- automation hook -------------------------------------------------------------------

struct Recorder {
    events: Mutex<Vec<String>>,
    reject_topic: Option<&'static str>,
}

impl Recorder {
    fn record(&self, event: String) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }

    /// The events recorded so far; the log starts over.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl BrokerHook for Recorder {
    fn on_connect(&self, client_id: &str) {
        self.record(format!("connect {client_id}"));
    }

    fn on_disconnect(&self, client_id: &str) {
        self.record(format!("disconnect {client_id}"));
    }

    fn on_subscribe(&self, client_id: &str, topic_filter: &str) {
        self.record(format!("subscribe {client_id} {topic_filter}"));
    }

    fn on_publish(&self, event: &PublishEvent<'_>) -> PublishDecision {
        self.record(format!(
            "publish {} {} {:?} retain={} {:?}",
            event.client_id, event.topic, event.qos, event.retain, event.payload
        ));
        if self.reject_topic == Some(event.topic) {
            PublishDecision::Reject
        } else {
            PublishDecision::Accept
        }
    }
}

fn hooked_broker(
    auth: Authenticator,
    reject_topic: Option<&'static str>,
) -> (Broker, Arc<Recorder>) {
    let recorder = Arc::new(Recorder {
        events: Mutex::default(),
        reject_topic,
    });
    let broker = Broker::new(BrokerConfig::default(), auth).with_hook(recorder.clone());
    (broker, recorder)
}

#[test]
fn the_hook_sees_the_lifecycle_of_a_client_in_order() {
    let (mut b, recorder) = hooked_broker(open_auth(), None);
    b.connect(connect("c", true, 0));
    b.handle(
        "c",
        subscribe(
            1,
            vec![
                sub("home/#", QoS::AtMostOnce),
                sub("bad/#/filter", QoS::AtMostOnce),
            ],
        ),
    );
    b.handle("c", retained("home/x", QoS::AtLeastOnce, Some(1), b"hi"));
    // An invalid topic name never reaches the hook.
    b.handle(
        "c",
        PacketV5::Publish(publish("home/+", QoS::AtMostOnce, None, b"x")),
    );
    b.handle("c", disconnect(ReasonCode::Success));
    // The teardown after a DISCONNECT is not a second disconnect.
    b.network_disconnect("c");
    assert_eq!(
        recorder.take(),
        [
            "connect c",
            "subscribe c home/#",
            "publish c home/x AtLeastOnce retain=true [104, 105]",
            "disconnect c",
        ]
    );
}

#[test]
fn the_hook_sees_a_takeover_as_a_second_connect() {
    let (mut b, recorder) = hooked_broker(open_auth(), None);
    b.connect(connect("dup", true, 0));
    b.connect(connect("dup", true, 0));
    b.network_disconnect("dup");
    b.network_disconnect("dup");
    assert_eq!(
        recorder.take(),
        ["connect dup", "connect dup", "disconnect dup"]
    );
    // A refused CONNECT is not reported.
    let (mut b, recorder) = hooked_broker(Authenticator::default(), None);
    b.connect(connect("c", true, 0));
    assert_eq!(recorder.take(), [""; 0]);
}

#[test]
fn a_hook_veto_drops_the_publish_like_an_acl_denial() {
    let (mut b, recorder) = hooked_broker(open_auth(), Some("home/secret"));
    b.connect(connect("c", true, 0));
    b.handle("c", subscribe(1, vec![sub("home/#", QoS::AtMostOnce)]));
    recorder.take();

    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("home/secret", QoS::AtMostOnce, None, b"x"))
        ),
        []
    );
    assert_eq!(
        b.handle(
            "c",
            retained("home/secret", QoS::AtLeastOnce, Some(4), b"x")
        ),
        [puback("c", 4, ReasonCode::NotAuthorized)]
    );
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("home/secret", QoS::ExactlyOnce, Some(5), b"x"))
        ),
        [pubrec("c", 5, ReasonCode::NotAuthorized)]
    );
    assert_eq!(b.retained_count(), 0);
    assert_eq!(
        (
            b.metrics().messages_received,
            b.metrics().messages_dropped,
            b.metrics().messages_sent
        ),
        (3, 3, 0)
    );
    assert_eq!(
        b.handle(
            "c",
            PacketV5::Publish(publish("home/open", QoS::AtMostOnce, None, b"x"))
        ),
        [deliver(
            "c",
            publish("home/open", QoS::AtMostOnce, None, b"x")
        )]
    );
    assert_eq!(recorder.take().len(), 4);
}

#[test]
fn the_hook_is_asked_before_the_acl_and_before_duplicate_detection() {
    let mut auth = Authenticator::default();
    auth.set_anonymous(true);
    auth.allow_any(AclAction::Publish, "open/#");
    let (mut b, recorder) = hooked_broker(auth, None);
    b.connect(connect("c", true, 0));
    recorder.take();

    // Denied by the ACL, yet shown to the hook.
    b.handle(
        "c",
        PacketV5::Publish(publish("closed/x", QoS::AtMostOnce, None, b"x")),
    );
    // A QoS 2 publish and its retransmission.
    for _ in 0..2 {
        b.handle(
            "c",
            PacketV5::Publish(publish("open/x", QoS::ExactlyOnce, Some(1), b"x")),
        );
    }
    assert_eq!(
        recorder.take(),
        [
            "publish c closed/x AtMostOnce retain=false [120]",
            "publish c open/x ExactlyOnce retain=false [120]",
            "publish c open/x ExactlyOnce retain=false [120]",
        ]
    );
    // Subscriptions the ACL refuses are not reported.
    b.handle("c", subscribe(1, vec![sub("open/#", QoS::AtMostOnce)]));
    assert_eq!(recorder.take(), [""; 0]);
}

// ---- metrics ---------------------------------------------------------------------------

#[test]
fn prometheus_snapshot_adds_the_gauges_the_counters_do_not_track() {
    let mut b = broker();
    b.connect(connect("sub", true, 0));
    b.connect(connect("pub", true, 0));
    b.handle(
        "sub",
        subscribe(
            1,
            vec![sub("home/#", QoS::AtLeastOnce), sub("x/+", QoS::AtMostOnce)],
        ),
    );
    b.handle("pub", retained("home/t", QoS::AtLeastOnce, Some(1), b"abc"));
    b.handle(
        "pub",
        PacketV5::Publish(publish("nobody", QoS::AtMostOnce, None, b"zz")),
    );
    b.network_disconnect("pub");

    // The live counters leave the two state gauges at zero ...
    assert_eq!(
        b.metrics(),
        &BrokerMetrics {
            clients_connected: 1,
            clients_total: 2,
            messages_received: 2,
            messages_sent: 1,
            messages_dropped: 0,
            bytes_received: 5,
            bytes_sent: 3,
            retained_messages: 0,
            subscriptions: 0,
        }
    );
    // ... and the exposition fills them in from the current state.
    let expected = "\
# HELP mqtt_clients_connected Clients currently connected.
# TYPE mqtt_clients_connected gauge
mqtt_clients_connected 1
# HELP mqtt_clients_total Connections accepted over the broker lifetime.
# TYPE mqtt_clients_total counter
mqtt_clients_total 2
# HELP mqtt_messages_received_total PUBLISH packets received from clients.
# TYPE mqtt_messages_received_total counter
mqtt_messages_received_total 2
# HELP mqtt_messages_sent_total PUBLISH packets delivered to subscribers.
# TYPE mqtt_messages_sent_total counter
mqtt_messages_sent_total 1
# HELP mqtt_messages_dropped_total PUBLISH packets dropped.
# TYPE mqtt_messages_dropped_total counter
mqtt_messages_dropped_total 0
# HELP mqtt_bytes_received_total Payload bytes received.
# TYPE mqtt_bytes_received_total counter
mqtt_bytes_received_total 5
# HELP mqtt_bytes_sent_total Payload bytes delivered.
# TYPE mqtt_bytes_sent_total counter
mqtt_bytes_sent_total 3
# HELP mqtt_retained_messages Retained messages stored.
# TYPE mqtt_retained_messages gauge
mqtt_retained_messages 1
# HELP mqtt_subscriptions Active subscriptions.
# TYPE mqtt_subscriptions gauge
mqtt_subscriptions 2
";
    assert_eq!(b.prometheus(), expected);
}
