//! Retained-message store (§3.3.1.3) — clean-room from spec.

use crate::broker::topic::topic_matches;
use crate::packet::QoS;
use crate::v5::property::Property;
use bytes::Bytes;
use std::collections::HashMap;

/// A message retained against a topic (§3.3.1.3): the last PUBLISH on a
/// topic that had RETAIN set is kept and delivered to new subscribers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedMessage {
    pub payload: Bytes,
    pub qos: QoS,
    pub properties: Vec<Property>,
}

/// Topic → retained message. Keys are exact Topic Names.
#[derive(Debug, Default)]
pub struct RetainedStore {
    map: HashMap<String, RetainedMessage>,
}

impl RetainedStore {
    /// Apply a retained PUBLISH. Per §3.3.1.3 a zero-length payload
    /// removes any retained message for the topic and stores nothing.
    pub fn apply(&mut self, topic: &str, msg: RetainedMessage) {
        if msg.payload.is_empty() {
            self.map.remove(topic);
        } else {
            self.map.insert(topic.to_owned(), msg);
        }
    }

    /// Explicitly clear a topic's retained message.
    pub fn clear(&mut self, topic: &str) {
        self.map.remove(topic);
    }

    /// All retained messages whose topic matches `filter` (§3.3.1.3:
    /// delivered when a matching subscription is created).
    pub fn matching(&self, filter: &str) -> Vec<(&str, &RetainedMessage)> {
        self.map
            .iter()
            .filter(|(topic, _)| topic_matches(filter, topic))
            .map(|(topic, msg)| (topic.as_str(), msg))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::QoS;
    use bytes::Bytes;

    fn msg(payload: &'static [u8]) -> RetainedMessage {
        RetainedMessage {
            payload: Bytes::from_static(payload),
            qos: QoS::AtLeastOnce,
            properties: vec![],
        }
    }

    #[test]
    fn stored_message_is_returned_for_matching_filter() {
        let mut store = RetainedStore::default();
        store.apply("home/loft/temp", msg(b"21.0"));
        store.apply("home/cellar/temp", msg(b"12.0"));
        let mut hits: Vec<_> =
            store.matching("home/+/temp").into_iter().map(|(t, _)| t).collect();
        hits.sort();
        assert_eq!(hits, vec!["home/cellar/temp", "home/loft/temp"]);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn empty_payload_with_retain_clears_the_topic() {
        // §3.3.1.3: a retained PUBLISH with a zero-length payload removes
        // the retained message and is not itself stored.
        let mut store = RetainedStore::default();
        store.apply("home/loft/temp", msg(b"21.0"));
        assert_eq!(store.len(), 1);
        store.apply("home/loft/temp", msg(b""));
        assert_eq!(store.len(), 0);
        assert!(store.matching("home/loft/temp").is_empty());
    }

    #[test]
    fn last_retained_message_wins() {
        let mut store = RetainedStore::default();
        store.apply("a/b", msg(b"first"));
        store.apply("a/b", msg(b"second"));
        let hits = store.matching("a/b");
        assert_eq!(hits.len(), 1);
        assert_eq!(&hits[0].1.payload[..], b"second");
    }
}
