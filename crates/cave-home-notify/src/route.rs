//! Routing engine — who gets a notification, and over which channel.
//!
//! A [`Subscriber`] registers interest in a [`Topic`] plus a minimum
//! [`Priority`] and the set of delivery [`Channel`]s it can actually receive.
//! [`route`] takes a notification and a slice of subscribers and produces a
//! [`DeliveryPlan`]: one [`Delivery`] per (subscriber, channel) that should
//! fire. A notification below a subscriber's minimum priority is filtered out;
//! a channel the subscriber cannot receive is gated out.
//!
//! This is a pure decision — no transport is touched. The actual sends
//! (self-hosted push server, SMTP, SMS, webhook) are deferred to Phase 1b
//! (ADR-021); they consume the plan this function produces.

use crate::notification::Notification;
use crate::priority::Priority;
use crate::topic::Topic;

/// A way a notification can physically reach someone.
///
/// The transports themselves are Phase-1b (ADR-021); this enum is the routing
/// decision's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// The self-hosted push notification to the cave-home mobile app.
    Push,
    /// An email message.
    Email,
    /// A text message.
    Sms,
    /// A call-out to an external URL.
    Webhook,
}

impl Channel {
    /// All channels — handy for "deliver everywhere this subscriber allows".
    pub const ALL: [Self; 4] = [Self::Push, Self::Email, Self::Sms, Self::Webhook];
}

/// Someone (or something) that wants to be told about a topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscriber {
    name: String,
    topic: Topic,
    minimum: Priority,
    channels: Vec<Channel>,
}

impl Subscriber {
    /// Register a subscriber for one topic, with a minimum priority and the
    /// channels it is able to receive.
    ///
    /// Duplicate channels are de-duplicated while preserving first-seen order.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        topic: Topic,
        minimum: Priority,
        channels: impl IntoIterator<Item = Channel>,
    ) -> Self {
        let mut deduped: Vec<Channel> = Vec::new();
        for c in channels {
            if !deduped.contains(&c) {
                deduped.push(c);
            }
        }
        Self {
            name: name.into(),
            topic,
            minimum,
            channels: deduped,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn topic(&self) -> &Topic {
        &self.topic
    }

    #[must_use]
    pub const fn minimum(&self) -> Priority {
        self.minimum
    }

    #[must_use]
    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    /// Whether this subscriber accepts this notification at all — same topic
    /// and the message clears its minimum priority.
    #[must_use]
    pub fn accepts(&self, n: &Notification) -> bool {
        n.topic() == &self.topic && n.priority().meets(self.minimum)
    }
}

/// One concrete decision: deliver to `subscriber` over `channel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// The subscriber's name (the plan is consumed after routing, so we copy
    /// the identifying handle rather than borrow the subscriber).
    pub subscriber: String,
    /// Which channel to use.
    pub channel: Channel,
}

/// The full set of deliveries a notification produces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeliveryPlan {
    /// Every (subscriber, channel) decision, in subscriber-then-channel order.
    pub deliveries: Vec<Delivery>,
}

impl DeliveryPlan {
    /// How many physical sends this plan represents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.deliveries.len()
    }

    /// Whether the plan would send nothing (everyone filtered or gated out).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.deliveries.is_empty()
    }

    /// The distinct subscribers this plan reaches, first-seen order preserved.
    #[must_use]
    pub fn recipients(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for d in &self.deliveries {
            if !seen.contains(&d.subscriber.as_str()) {
                seen.push(&d.subscriber);
            }
        }
        seen
    }
}

/// Decide who receives `notification` and over which channels.
///
/// A subscriber contributes one [`Delivery`] per channel it can receive, but
/// only if it subscribes to the notification's topic and the notification's
/// priority clears the subscriber's minimum. Subscribers are considered in the
/// order given; each subscriber's channels are emitted in its own order.
#[must_use]
pub fn route(notification: &Notification, subscribers: &[Subscriber]) -> DeliveryPlan {
    let mut deliveries = Vec::new();
    for sub in subscribers {
        if !sub.accepts(notification) {
            continue;
        }
        for &channel in sub.channels() {
            deliveries.push(Delivery {
                subscriber: sub.name.clone(),
                channel,
            });
        }
    }
    DeliveryPlan { deliveries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::Notification;

    fn topic(name: &str) -> Topic {
        Topic::new(name).expect("valid test topic")
    }

    fn note(t: &str, p: Priority) -> Notification {
        Notification::new(topic(t), "Title", "Body", 0).with_priority(p)
    }

    #[test]
    fn routes_to_matching_topic_subscriber() {
        let subs = [Subscriber::new(
            "mum",
            topic("leak"),
            Priority::Low,
            [Channel::Push],
        )];
        let plan = route(&note("leak", Priority::Default), &subs);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.deliveries[0].subscriber, "mum");
        assert_eq!(plan.deliveries[0].channel, Channel::Push);
    }

    #[test]
    fn filters_other_topics() {
        let subs = [Subscriber::new(
            "mum",
            topic("door"),
            Priority::Min,
            [Channel::Push],
        )];
        let plan = route(&note("leak", Priority::Max), &subs);
        assert!(plan.is_empty());
    }

    #[test]
    fn filters_below_minimum_priority() {
        let subs = [Subscriber::new(
            "mum",
            topic("leak"),
            Priority::High,
            [Channel::Push],
        )];
        // Default < High -> filtered.
        assert!(route(&note("leak", Priority::Default), &subs).is_empty());
        // High == High -> delivered.
        assert_eq!(route(&note("leak", Priority::High), &subs).len(), 1);
        // Max > High -> delivered.
        assert_eq!(route(&note("leak", Priority::Max), &subs).len(), 1);
    }

    #[test]
    fn fans_out_to_all_a_subscribers_channels() {
        let subs = [Subscriber::new(
            "dad",
            topic("alarm"),
            Priority::Default,
            [Channel::Push, Channel::Email, Channel::Sms],
        )];
        let plan = route(&note("alarm", Priority::High), &subs);
        assert_eq!(plan.len(), 3);
        assert_eq!(
            plan.deliveries.iter().map(|d| d.channel).collect::<Vec<_>>(),
            vec![Channel::Push, Channel::Email, Channel::Sms]
        );
    }

    #[test]
    fn multi_subscriber_routing_preserves_order() {
        let subs = [
            Subscriber::new("mum", topic("leak"), Priority::Min, [Channel::Push]),
            Subscriber::new("dad", topic("leak"), Priority::High, [Channel::Sms]),
            Subscriber::new("sitter", topic("door"), Priority::Min, [Channel::Push]),
        ];
        // Default priority: mum (min Min) gets it, dad (min High) does not,
        // sitter is on a different topic.
        let plan = route(&note("leak", Priority::Default), &subs);
        assert_eq!(plan.recipients(), vec!["mum"]);
        // Max priority: mum and dad both clear; sitter still wrong topic.
        let plan = route(&note("leak", Priority::Max), &subs);
        assert_eq!(plan.recipients(), vec!["mum", "dad"]);
    }

    #[test]
    fn capability_gating_only_uses_declared_channels() {
        // A webhook-only integration never receives a Push or Sms decision.
        let subs = [Subscriber::new(
            "logger",
            topic("leak"),
            Priority::Min,
            [Channel::Webhook],
        )];
        let plan = route(&note("leak", Priority::Min), &subs);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.deliveries[0].channel, Channel::Webhook);
        assert!(!plan
            .deliveries
            .iter()
            .any(|d| d.channel == Channel::Push || d.channel == Channel::Sms));
    }

    #[test]
    fn subscriber_dedups_repeated_channels() {
        let sub = Subscriber::new(
            "mum",
            topic("leak"),
            Priority::Min,
            [Channel::Push, Channel::Push, Channel::Email],
        );
        assert_eq!(sub.channels(), [Channel::Push, Channel::Email]);
    }

    #[test]
    fn empty_subscriber_list_routes_nothing() {
        assert!(route(&note("leak", Priority::Max), &[]).is_empty());
    }
}
