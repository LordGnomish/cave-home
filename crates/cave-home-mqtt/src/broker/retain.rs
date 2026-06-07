//! Retained-message store (§3.3.1.3) — clean-room from spec.

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
