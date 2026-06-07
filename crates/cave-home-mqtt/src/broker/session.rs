//! Per-client MQTT session state (§4.1) — clean-room from spec.
//!
//! A session holds a client's subscriptions, its QoS 1/2 message-flow
//! bookkeeping (§4.3) and, for a persistent session (§3.1.2.11 Session
//! Expiry Interval > 0), the messages queued while it is offline.

use crate::v5::packet::{PublishV5, SubscriptionV5};
use std::collections::{HashMap, HashSet, VecDeque};

/// State retained by the broker for a single client identifier.
#[derive(Debug)]
pub struct Session {
    pub client_id: String,
    pub username: Option<String>,
    /// §3.1.2.11 — seconds the session survives after disconnect.
    pub session_expiry_secs: u32,
    /// Whether a network connection is currently bound to this session.
    pub connected: bool,
    pub subscriptions: Vec<SubscriptionV5>,
    next_packet_id: u16,
    /// QoS 1/2 PUBLISH sent to the client, awaiting PUBACK / PUBREC.
    pub outgoing_unacked: HashMap<u16, PublishV5>,
    /// QoS 2 PUBLISH we sent that reached PUBREC; PUBREL sent, awaiting PUBCOMP.
    pub outgoing_pubrel: HashSet<u16>,
    /// QoS 2 PUBLISH received from the client; PUBREC sent, awaiting PUBREL.
    pub incoming_qos2: HashSet<u16>,
    /// Messages queued while offline (delivered on resume).
    pub queued: VecDeque<PublishV5>,
}

impl Session {
    pub fn new(client_id: String, session_expiry_secs: u32) -> Self {
        Self {
            client_id,
            username: None,
            session_expiry_secs,
            connected: false,
            subscriptions: Vec::new(),
            next_packet_id: 0,
            outgoing_unacked: HashMap::new(),
            outgoing_pubrel: HashSet::new(),
            incoming_qos2: HashSet::new(),
            queued: VecDeque::new(),
        }
    }

    /// §2.2.1 — allocate the next non-zero packet identifier not already
    /// in use by an in-flight outbound message.
    pub fn next_id(&mut self) -> u16 {
        loop {
            self.next_packet_id = self.next_packet_id.wrapping_add(1);
            if self.next_packet_id == 0 {
                self.next_packet_id = 1;
            }
            if !self.outgoing_unacked.contains_key(&self.next_packet_id)
                && !self.outgoing_pubrel.contains(&self.next_packet_id)
            {
                return self.next_packet_id;
            }
        }
    }

    /// Add or replace a subscription by topic filter. Returns `true` if a
    /// subscription for the same filter already existed (§3.8.4).
    pub fn add_subscription(&mut self, sub: SubscriptionV5) -> bool {
        if let Some(existing) =
            self.subscriptions.iter_mut().find(|s| s.topic_filter == sub.topic_filter)
        {
            *existing = sub;
            true
        } else {
            self.subscriptions.push(sub);
            false
        }
    }

    /// Remove a subscription by topic filter. Returns `true` if one was
    /// present (drives the UNSUBACK reason code, §3.11.3).
    pub fn remove_subscription(&mut self, filter: &str) -> bool {
        let before = self.subscriptions.len();
        self.subscriptions.retain(|s| s.topic_filter != filter);
        self.subscriptions.len() != before
    }
}
