//! Bridge to other MQTT brokers (Mosquitto-compatible configuration).
//!
//! A bridge mirrors messages between this broker and a remote one. Each
//! `topic` rule names a topic pattern, a direction, a QoS and optional
//! local/remote prefixes — mirroring Mosquitto's
//! `topic <pattern> [direction [qos [local_prefix remote_prefix]]]`.
//! This module is the I/O-free decision core: it computes which filters
//! to subscribe to on each side and maps a topic across the bridge,
//! applying the prefix rewrite. The socket pump lives in the runtime.

use crate::broker::topic::topic_matches;
use crate::packet::QoS;

/// Direction a bridged topic flows (Mosquitto `out`/`in`/`both`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Local → remote.
    Out,
    /// Remote → local.
    In,
    /// Both directions.
    Both,
}

impl Direction {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "out" => Self::Out,
            "in" => Self::In,
            "both" => Self::Both,
            _ => return None,
        })
    }

    fn flows_out(self) -> bool {
        matches!(self, Self::Out | Self::Both)
    }

    fn flows_in(self) -> bool {
        matches!(self, Self::In | Self::Both)
    }
}

/// One bridged topic rule (a Mosquitto `topic` directive).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopicRule {
    pub pattern: String,
    pub direction: Direction,
    pub qos: QoS,
    pub local_prefix: String,
    pub remote_prefix: String,
}

impl TopicRule {
    /// Parse `topic <pattern> [direction [qos [local_prefix remote_prefix]]]`
    /// (the value part, without the leading `topic` keyword).
    pub fn parse(line: &str) -> Option<Self> {
        let mut it = line.split_whitespace();
        let pattern = it.next()?.to_owned();
        let direction = match it.next() {
            Some(d) => Direction::parse(d)?,
            None => Direction::Both,
        };
        let qos = match it.next() {
            Some(q) => QoS::from_u8(q.parse().ok()?)?,
            None => QoS::AtMostOnce,
        };
        let local_prefix = it.next().unwrap_or("").to_owned();
        let remote_prefix = it.next().unwrap_or("").to_owned();
        Some(Self { pattern, direction, qos, local_prefix, remote_prefix })
    }

    fn local_filter(&self) -> String {
        format!("{}{}", self.local_prefix, self.pattern)
    }

    fn remote_filter(&self) -> String {
        format!("{}{}", self.remote_prefix, self.pattern)
    }
}

/// A configured bridge to a remote broker.
#[derive(Clone, Debug)]
pub struct BridgeConfig {
    pub name: String,
    pub remote_addr: String,
    pub client_id: String,
    pub remote_username: Option<String>,
    pub remote_password: Option<String>,
    pub rules: Vec<TopicRule>,
}

impl BridgeConfig {
    /// Local topic filters the bridge must subscribe to (outbound rules).
    pub fn local_subscriptions(&self) -> Vec<String> {
        self.rules
            .iter()
            .filter(|r| r.direction.flows_out())
            .map(TopicRule::local_filter)
            .collect()
    }

    /// Remote topic filters the bridge must subscribe to (inbound rules).
    pub fn remote_subscriptions(&self) -> Vec<String> {
        self.rules
            .iter()
            .filter(|r| r.direction.flows_in())
            .map(TopicRule::remote_filter)
            .collect()
    }

    /// Map a local topic to its remote name, if an outbound rule matches.
    pub fn map_local_to_remote(&self, local_topic: &str) -> Option<String> {
        for r in &self.rules {
            if !r.direction.flows_out() {
                continue;
            }
            if topic_matches(&r.local_filter(), local_topic) {
                let stem = local_topic.strip_prefix(r.local_prefix.as_str())?;
                return Some(format!("{}{}", r.remote_prefix, stem));
            }
        }
        None
    }

    /// Map a remote topic to its local name, if an inbound rule matches.
    pub fn map_remote_to_local(&self, remote_topic: &str) -> Option<String> {
        for r in &self.rules {
            if !r.direction.flows_in() {
                continue;
            }
            if topic_matches(&r.remote_filter(), remote_topic) {
                let stem = remote_topic.strip_prefix(r.remote_prefix.as_str())?;
                return Some(format!("{}{}", r.local_prefix, stem));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::QoS;

    #[test]
    fn parse_full_mosquitto_topic_line() {
        let r = TopicRule::parse("sensors/# out 0 home/ cloud/").unwrap();
        assert_eq!(r.pattern, "sensors/#");
        assert_eq!(r.direction, Direction::Out);
        assert_eq!(r.qos, QoS::AtMostOnce);
        assert_eq!(r.local_prefix, "home/");
        assert_eq!(r.remote_prefix, "cloud/");
    }

    #[test]
    fn parse_defaults_direction_and_qos() {
        let r = TopicRule::parse("alerts/+").unwrap();
        assert_eq!(r.pattern, "alerts/+");
        assert_eq!(r.direction, Direction::Both);
        assert_eq!(r.qos, QoS::AtMostOnce);
        assert!(r.local_prefix.is_empty());
        assert!(r.remote_prefix.is_empty());
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(TopicRule::parse("").is_none());
        assert!(TopicRule::parse("a sideways 0").is_none()); // bad direction
    }

    fn cfg() -> BridgeConfig {
        BridgeConfig {
            name: "cloud".into(),
            remote_addr: "mqtt.example:8883".into(),
            client_id: "cave-bridge".into(),
            remote_username: None,
            remote_password: None,
            rules: vec![
                TopicRule::parse("sensors/# out 1 home/ cloud/").unwrap(),
                TopicRule::parse("commands/# in 1 home/ cloud/").unwrap(),
                TopicRule::parse("status/+ both 0").unwrap(),
            ],
        }
    }

    #[test]
    fn local_subscriptions_cover_out_and_both_rules() {
        // Outbound rules require a *local* subscription on local_prefix+pattern.
        let mut subs = cfg().local_subscriptions();
        subs.sort();
        assert_eq!(subs, vec!["home/sensors/#".to_string(), "status/+".to_string()]);
    }

    #[test]
    fn remote_subscriptions_cover_in_and_both_rules() {
        let mut subs = cfg().remote_subscriptions();
        subs.sort();
        assert_eq!(subs, vec!["cloud/commands/#".to_string(), "status/+".to_string()]);
    }

    #[test]
    fn maps_local_topic_to_remote_with_prefix_rewrite() {
        let c = cfg();
        // Local home/sensors/loft/temp → remote cloud/sensors/loft/temp.
        assert_eq!(
            c.map_local_to_remote("home/sensors/loft/temp"),
            Some("cloud/sensors/loft/temp".to_string())
        );
        // status/+ (both, no prefixes) maps unchanged.
        assert_eq!(c.map_local_to_remote("status/online"), Some("status/online".to_string()));
        // A purely inbound topic does not go out.
        assert_eq!(c.map_local_to_remote("home/commands/x"), None);
    }

    #[test]
    fn maps_remote_topic_to_local_with_prefix_rewrite() {
        let c = cfg();
        // Remote cloud/commands/reboot → local home/commands/reboot.
        assert_eq!(
            c.map_remote_to_local("cloud/commands/reboot"),
            Some("home/commands/reboot".to_string())
        );
        assert_eq!(c.map_remote_to_local("status/online"), Some("status/online".to_string()));
        // A purely outbound topic does not come in.
        assert_eq!(c.map_remote_to_local("cloud/sensors/x"), None);
    }
}
