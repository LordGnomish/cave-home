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

use crate::broker::auth::{AclAction, Authenticator};
use crate::broker::retain::{RetainedMessage, RetainedStore};
use crate::broker::session::Session;
use crate::broker::topic::{topic_matches, valid_topic_filter, valid_topic_name};
use crate::packet::QoS;
use crate::v5::packet::{
    ConnAckV5, ConnectV5, DisconnectV5, PacketV5, PubAckV5, PubCompV5, PubRecV5,
    PubRelV5, PublishV5, RetainHandling, SubAckV5, SubscribeV5, SubscriptionV5,
    UnsubAckV5, UnsubscribeV5, Will,
};
use crate::v5::property::Property;
use crate::v5::reason::ReasonCode;
use bytes::Bytes;
use std::collections::HashMap;

/// Broker-wide capabilities advertised in CONNACK and applied to QoS /
/// retain handling.
#[derive(Clone, Debug)]
pub struct BrokerConfig {
    /// §3.2.2.3.4 — the maximum QoS the server accepts/grants.
    pub max_qos: QoS,
    /// §3.2.2.3.5 — whether RETAIN is supported.
    pub retain_available: bool,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self { max_qos: QoS::ExactlyOnce, retain_available: true }
    }
}

/// A side effect the runtime must perform. The decision core never does
/// I/O itself; it returns these for the transport layer to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Write `packet` to the connection bound to `client_id`.
    Send { client_id: String, packet: PacketV5 },
    /// Close the connection bound to `client_id` (e.g. session takeover).
    Drop { client_id: String, reason: ReasonCode },
}

/// The clean-room MQTT broker decision core.
pub struct Broker {
    config: BrokerConfig,
    auth: Authenticator,
    retained: RetainedStore,
    sessions: HashMap<String, Session>,
    wills: HashMap<String, Will>,
    next_auto: u64,
}

impl Broker {
    pub fn new(config: BrokerConfig, auth: Authenticator) -> Self {
        Self {
            config,
            auth,
            retained: RetainedStore::default(),
            sessions: HashMap::new(),
            wills: HashMap::new(),
            next_auto: 0,
        }
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn retained_count(&self) -> usize {
        self.retained.len()
    }

    // ---- CONNECT ---------------------------------------------------------

    /// §3.1 — process a CONNECT, returning the CONNACK plus any takeover
    /// drop and queued-message deliveries.
    pub fn connect(&mut self, mut c: ConnectV5) -> Vec<Action> {
        let mut actions = Vec::new();
        let username = c.username.clone();
        let password = c.password.clone();
        if !self.auth.authenticate(username.as_deref(), password.as_deref()) {
            let cid = if c.client_id.is_empty() { "<unauthenticated>".to_owned() } else { c.client_id.clone() };
            actions.push(Action::Send {
                client_id: cid.clone(),
                packet: PacketV5::ConnAck(ConnAckV5 {
                    session_present: false,
                    reason_code: ReasonCode::BadUserNameOrPassword,
                    properties: vec![],
                }),
            });
            actions.push(Action::Drop { client_id: cid, reason: ReasonCode::BadUserNameOrPassword });
            return actions;
        }

        // §3.1.3.1 — assign a client id when the payload one is empty.
        let (client_id, assigned) = if c.client_id.is_empty() {
            self.next_auto += 1;
            (format!("auto-{}", self.next_auto), true)
        } else {
            (c.client_id.clone(), false)
        };

        // §3.1.4.3 — a second connection with the same id takes over.
        if self.sessions.get(&client_id).is_some_and(|s| s.connected) {
            actions.push(Action::Drop {
                client_id: client_id.clone(),
                reason: ReasonCode::SessionTakenOver,
            });
        }

        let session_expiry = session_expiry_from(&c.properties);
        let session_present = if c.clean_start {
            self.sessions.remove(&client_id);
            false
        } else {
            self.sessions.contains_key(&client_id)
        };

        let will = c.will.take();
        {
            let sess = self
                .sessions
                .entry(client_id.clone())
                .or_insert_with(|| Session::new(client_id.clone(), session_expiry));
            sess.connected = true;
            sess.session_expiry_secs = session_expiry;
            sess.username = username;
        }
        if let Some(w) = will {
            self.wills.insert(client_id.clone(), w);
        } else {
            self.wills.remove(&client_id);
        }

        let mut props = Vec::new();
        if assigned {
            props.push(Property::AssignedClientIdentifier(client_id.clone()));
        }
        actions.push(Action::Send {
            client_id: client_id.clone(),
            packet: PacketV5::ConnAck(ConnAckV5 {
                session_present,
                reason_code: ReasonCode::Success,
                properties: props,
            }),
        });

        // Deliver anything queued while the (resumed) session was offline.
        if session_present {
            if let Some(sess) = self.sessions.get_mut(&client_id) {
                let queued: Vec<PublishV5> = sess.queued.drain(..).collect();
                for p in queued {
                    if let Some(pid) = p.packet_id {
                        sess.outgoing_unacked.insert(pid, p.clone());
                    }
                    actions.push(Action::Send {
                        client_id: client_id.clone(),
                        packet: PacketV5::Publish(p),
                    });
                }
            }
        }
        actions
    }

    // ---- Post-CONNECT packet dispatch ------------------------------------

    /// Dispatch a packet from an already-connected `client_id` (§3.x).
    pub fn handle(&mut self, client_id: &str, packet: PacketV5) -> Vec<Action> {
        match packet {
            PacketV5::Subscribe(s) => self.on_subscribe(client_id, &s),
            PacketV5::Unsubscribe(u) => self.on_unsubscribe(client_id, &u),
            PacketV5::Publish(p) => self.on_publish(client_id, &p),
            PacketV5::PubAck(a) => {
                if let Some(s) = self.sessions.get_mut(client_id) {
                    s.outgoing_unacked.remove(&a.packet_id);
                }
                vec![]
            }
            PacketV5::PubRec(r) => {
                // Subscriber acknowledged our QoS 2 PUBLISH: release it.
                if let Some(s) = self.sessions.get_mut(client_id) {
                    s.outgoing_unacked.remove(&r.packet_id);
                    s.outgoing_pubrel.insert(r.packet_id);
                }
                vec![Action::Send {
                    client_id: client_id.to_owned(),
                    packet: PacketV5::PubRel(PubRelV5 {
                        packet_id: r.packet_id,
                        reason_code: ReasonCode::Success,
                        properties: vec![],
                    }),
                }]
            }
            PacketV5::PubRel(r) => {
                // Publisher released a QoS 2 message: complete the handshake.
                if let Some(s) = self.sessions.get_mut(client_id) {
                    s.incoming_qos2.remove(&r.packet_id);
                }
                vec![Action::Send {
                    client_id: client_id.to_owned(),
                    packet: PacketV5::PubComp(PubCompV5 {
                        packet_id: r.packet_id,
                        reason_code: ReasonCode::Success,
                        properties: vec![],
                    }),
                }]
            }
            PacketV5::PubComp(c) => {
                if let Some(s) = self.sessions.get_mut(client_id) {
                    s.outgoing_pubrel.remove(&c.packet_id);
                }
                vec![]
            }
            PacketV5::PingReq => vec![Action::Send {
                client_id: client_id.to_owned(),
                packet: PacketV5::PingResp,
            }],
            PacketV5::Disconnect(d) => self.on_disconnect(client_id, &d),
            // The server is not expected to receive these; ignore safely.
            PacketV5::Connect(_)
            | PacketV5::ConnAck(_)
            | PacketV5::SubAck(_)
            | PacketV5::UnsubAck(_)
            | PacketV5::PingResp
            | PacketV5::Auth(_) => vec![],
        }
    }

    // ---- SUBSCRIBE / UNSUBSCRIBE -----------------------------------------

    fn on_subscribe(&mut self, client_id: &str, s: &SubscribeV5) -> Vec<Action> {
        let username = self.sessions.get(client_id).and_then(|s| s.username.clone());
        let mut reason_codes = Vec::with_capacity(s.subscriptions.len());
        // (filter, granted-qos) pairs whose retained messages to deliver.
        let mut deliver_retained: Vec<(String, QoS)> = Vec::new();

        for sub in &s.subscriptions {
            if !valid_topic_filter(&sub.topic_filter) {
                reason_codes.push(ReasonCode::TopicFilterInvalid);
                continue;
            }
            if !self.auth.authorize(username.as_deref(), AclAction::Subscribe, &sub.topic_filter) {
                reason_codes.push(ReasonCode::NotAuthorized);
                continue;
            }
            let granted = qos_min(sub.qos, self.config.max_qos);
            let stored = SubscriptionV5 { qos: granted, ..sub.clone() };
            let existed = self
                .sessions
                .get_mut(client_id)
                .is_some_and(|sess| sess.add_subscription(stored));
            reason_codes.push(granted_code(granted));

            let send_retained = match sub.retain_handling {
                RetainHandling::SendOnSubscribe => true,
                RetainHandling::SendIfNew => !existed,
                RetainHandling::DoNotSend => false,
            };
            if send_retained {
                deliver_retained.push((sub.topic_filter.clone(), granted));
            }
        }

        let mut actions = vec![Action::Send {
            client_id: client_id.to_owned(),
            packet: PacketV5::SubAck(SubAckV5 {
                packet_id: s.packet_id,
                properties: vec![],
                reason_codes,
            }),
        }];

        for (filter, granted) in deliver_retained {
            let hits: Vec<(String, RetainedMessage)> = self
                .retained
                .matching(&filter)
                .into_iter()
                .map(|(t, m)| (t.to_owned(), m.clone()))
                .collect();
            for (topic, msg) in hits {
                let qos = qos_min(msg.qos, granted);
                if let Some(sess) = self.sessions.get_mut(client_id) {
                    let packet_id = if qos == QoS::AtMostOnce { None } else { Some(sess.next_id()) };
                    let p = PublishV5 {
                        topic,
                        qos,
                        retain: true, // §3.3.1.3 retained delivery has RETAIN=1
                        dup: false,
                        packet_id,
                        properties: msg.properties.clone(),
                        payload: msg.payload.clone(),
                    };
                    if let Some(pid) = packet_id {
                        sess.outgoing_unacked.insert(pid, p.clone());
                    }
                    actions.push(Action::Send {
                        client_id: client_id.to_owned(),
                        packet: PacketV5::Publish(p),
                    });
                }
            }
        }
        actions
    }

    fn on_unsubscribe(&mut self, client_id: &str, u: &UnsubscribeV5) -> Vec<Action> {
        let mut reason_codes = Vec::with_capacity(u.topic_filters.len());
        for f in &u.topic_filters {
            let removed = self
                .sessions
                .get_mut(client_id)
                .is_some_and(|s| s.remove_subscription(f));
            reason_codes.push(if removed {
                ReasonCode::Success
            } else {
                ReasonCode::NoSubscriptionExisted
            });
        }
        vec![Action::Send {
            client_id: client_id.to_owned(),
            packet: PacketV5::UnsubAck(UnsubAckV5 {
                packet_id: u.packet_id,
                properties: vec![],
                reason_codes,
            }),
        }]
    }

    // ---- PUBLISH ---------------------------------------------------------

    fn on_publish(&mut self, client_id: &str, p: &PublishV5) -> Vec<Action> {
        if !valid_topic_name(&p.topic) {
            return ack_publish_error(client_id, p, ReasonCode::TopicNameInvalid);
        }
        let username = self.sessions.get(client_id).and_then(|s| s.username.clone());
        if !self.auth.authorize(username.as_deref(), AclAction::Publish, &p.topic) {
            return ack_publish_error(client_id, p, ReasonCode::NotAuthorized);
        }

        // §3.3.1.2 — QoS 2 duplicate: re-acknowledge without re-routing.
        if p.qos == QoS::ExactlyOnce {
            if let Some(pid) = p.packet_id {
                if self.sessions.get(client_id).is_some_and(|s| s.incoming_qos2.contains(&pid)) {
                    return vec![pubrec(client_id, pid, ReasonCode::Success)];
                }
            }
        }

        if p.retain && self.config.retain_available {
            self.retained.apply(
                &p.topic,
                RetainedMessage {
                    payload: p.payload.clone(),
                    qos: p.qos,
                    properties: p.properties.clone(),
                },
            );
        }

        let (delivered, mut actions) = self.route_publish(
            Some(client_id),
            &p.topic,
            p.qos,
            &p.payload,
            &p.properties,
            p.retain,
        );

        let reason = if delivered > 0 {
            ReasonCode::Success
        } else {
            ReasonCode::NoMatchingSubscribers
        };
        match p.qos {
            QoS::AtMostOnce => {}
            QoS::AtLeastOnce => {
                if let Some(pid) = p.packet_id {
                    actions.push(Action::Send {
                        client_id: client_id.to_owned(),
                        packet: PacketV5::PubAck(PubAckV5 {
                            packet_id: pid,
                            reason_code: reason,
                            properties: vec![],
                        }),
                    });
                }
            }
            QoS::ExactlyOnce => {
                if let Some(pid) = p.packet_id {
                    if let Some(s) = self.sessions.get_mut(client_id) {
                        s.incoming_qos2.insert(pid);
                    }
                    actions.push(pubrec(client_id, pid, reason));
                }
            }
        }
        actions
    }

    /// Fan a message out to every session with a matching subscription.
    /// Returns the delivery count (sent or queued) and the Send actions.
    fn route_publish(
        &mut self,
        publisher: Option<&str>,
        topic: &str,
        src_qos: QoS,
        payload: &Bytes,
        properties: &[Property],
        retain_src: bool,
    ) -> (usize, Vec<Action>) {
        // Resolve targets first to avoid aliasing the session map.
        let targets: Vec<(String, QoS, bool)> = self
            .sessions
            .iter()
            .filter_map(|(id, sess)| {
                let mut best: Option<QoS> = None;
                let mut rap = false;
                for sub in &sess.subscriptions {
                    if !topic_matches(&sub.topic_filter, topic) {
                        continue;
                    }
                    if sub.no_local && publisher == Some(id.as_str()) {
                        continue; // §3.8.3.1 No Local
                    }
                    best = Some(match best {
                        Some(q) => qos_max(q, sub.qos),
                        None => sub.qos,
                    });
                    rap |= sub.retain_as_published;
                }
                best.map(|q| (id.clone(), qos_min(src_qos, q), rap))
            })
            .collect();

        let mut actions = Vec::new();
        let mut delivered = 0;
        for (id, qos, rap) in targets {
            delivered += 1;
            let forward_retain = retain_src && rap;
            let Some(sess) = self.sessions.get_mut(&id) else { continue };
            let packet_id = if qos == QoS::AtMostOnce { None } else { Some(sess.next_id()) };
            let pkt = PublishV5 {
                topic: topic.to_owned(),
                qos,
                retain: forward_retain,
                dup: false,
                packet_id,
                properties: properties.to_vec(),
                payload: payload.clone(),
            };
            if sess.connected {
                if let Some(pid) = packet_id {
                    sess.outgoing_unacked.insert(pid, pkt.clone());
                }
                actions.push(Action::Send { client_id: id, packet: PacketV5::Publish(pkt) });
            } else if qos != QoS::AtMostOnce {
                sess.queued.push_back(pkt); // persistent session: queue QoS 1/2
            }
            // Offline QoS 0 is dropped (§4.1 / non-persistent fan-out).
        }
        (delivered, actions)
    }

    // ---- DISCONNECT / network teardown -----------------------------------

    fn on_disconnect(&mut self, client_id: &str, d: &DisconnectV5) -> Vec<Action> {
        // §3.14.4 — a normal DISCONNECT discards the Will unless the client
        // explicitly asks to publish it (reason 0x04).
        if d.reason_code != ReasonCode::DisconnectWithWill {
            self.wills.remove(client_id);
        }
        self.detach(client_id);
        vec![]
    }

    /// Ungraceful network loss (§3.1.2.5): publish the Will, then detach.
    pub fn network_disconnect(&mut self, client_id: &str) -> Vec<Action> {
        let mut actions = Vec::new();
        if let Some(will) = self.wills.remove(client_id) {
            if will.retain && self.config.retain_available {
                self.retained.apply(
                    &will.topic,
                    RetainedMessage {
                        payload: will.payload.clone(),
                        qos: will.qos,
                        properties: will.properties.clone(),
                    },
                );
            }
            let (_n, will_actions) = self.route_publish(
                Some(client_id),
                &will.topic,
                will.qos,
                &will.payload,
                &will.properties,
                will.retain,
            );
            actions.extend(will_actions);
        }
        self.detach(client_id);
        actions
    }

    /// Mark the session offline; drop it entirely if it must not persist.
    fn detach(&mut self, client_id: &str) {
        let expire = match self.sessions.get_mut(client_id) {
            Some(sess) => {
                sess.connected = false;
                sess.session_expiry_secs == 0
            }
            None => return,
        };
        if expire {
            self.sessions.remove(client_id);
        }
    }
}

/// §3.1.2.11 — read the Session Expiry Interval property (default 0).
fn session_expiry_from(props: &[Property]) -> u32 {
    props
        .iter()
        .find_map(|p| match p {
            Property::SessionExpiryInterval(v) => Some(*v),
            _ => None,
        })
        .unwrap_or(0)
}

fn qos_min(a: QoS, b: QoS) -> QoS {
    if (a as u8) <= (b as u8) { a } else { b }
}

fn qos_max(a: QoS, b: QoS) -> QoS {
    if (a as u8) >= (b as u8) { a } else { b }
}

/// §3.9.3 — map a granted QoS to its SUBACK reason code.
fn granted_code(qos: QoS) -> ReasonCode {
    match qos {
        QoS::AtMostOnce => ReasonCode::Success, // 0x00 == Granted QoS 0
        QoS::AtLeastOnce => ReasonCode::GrantedQoS1,
        QoS::ExactlyOnce => ReasonCode::GrantedQoS2,
    }
}

fn pubrec(client_id: &str, packet_id: u16, reason: ReasonCode) -> Action {
    Action::Send {
        client_id: client_id.to_owned(),
        packet: PacketV5::PubRec(PubRecV5 { packet_id, reason_code: reason, properties: vec![] }),
    }
}

/// QoS-appropriate negative acknowledgement for a rejected PUBLISH.
fn ack_publish_error(client_id: &str, p: &PublishV5, reason: ReasonCode) -> Vec<Action> {
    match (p.qos, p.packet_id) {
        (QoS::AtLeastOnce, Some(pid)) => vec![Action::Send {
            client_id: client_id.to_owned(),
            packet: PacketV5::PubAck(PubAckV5 { packet_id: pid, reason_code: reason, properties: vec![] }),
        }],
        (QoS::ExactlyOnce, Some(pid)) => vec![pubrec(client_id, pid, reason)],
        _ => vec![],
    }
}

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
