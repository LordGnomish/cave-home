//! The notification model — what a household actually gets told.
//!
//! A [`Notification`] is a plain value object: a title, a body, a priority, a
//! topic, optional tags, and optional click-action / attachment metadata. It
//! carries its creation time as an integer tick supplied by the caller — this
//! crate never reads the clock itself (Charter: pure logic, std-only, no time
//! crate). The wire-format / transport details (how it reaches a phone) live in
//! the deferred Phase-1b transports; this is the in-memory shape they carry.

use crate::priority::Priority;
use crate::topic::Topic;

/// A monotonic time tick supplied by the caller (seconds since some epoch).
///
/// The crate treats it as an opaque integer clock for dedup windows and rate
/// limiting; it never interprets it as a wall-clock date.
pub type Tick = u64;

/// Where a notification's tap-action points, as plain metadata.
///
/// The transport decides what to do with it; the model just carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickAction {
    /// Open a place in the cave-home app (e.g. a room or a device tile).
    OpenView(String),
    /// Open an external link.
    OpenLink(String),
}

/// A file attached to a notification, described by metadata only.
///
/// The bytes themselves are out of scope for the pure model (a transport
/// fetches/streams them in Phase 1b); here we carry just enough to render and
/// route a message that has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// A short, human name for the file ("doorbell.jpg").
    pub name: String,
    /// Size in bytes, if the publisher knew it.
    pub size_bytes: Option<u64>,
}

/// A single notification destined for a [`Topic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    topic: Topic,
    title: String,
    body: String,
    priority: Priority,
    tags: Vec<String>,
    click: Option<ClickAction>,
    attachment: Option<Attachment>,
    created_at: Tick,
}

impl Notification {
    /// Build a notification with the required fields.
    ///
    /// Optional fields default to empty / [`Priority::Default`] and are set with
    /// the builder-style `with_*` methods.
    #[must_use]
    pub fn new(
        topic: Topic,
        title: impl Into<String>,
        body: impl Into<String>,
        created_at: Tick,
    ) -> Self {
        Self {
            topic,
            title: title.into(),
            body: body.into(),
            priority: Priority::Default,
            tags: Vec::new(),
            click: None,
            attachment: None,
            created_at,
        }
    }

    /// Set the priority.
    #[must_use]
    pub const fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Replace the tag set.
    #[must_use]
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = String>) -> Self {
        self.tags = tags.into_iter().collect();
        self
    }

    /// Attach a tap-action.
    #[must_use]
    pub fn with_click(mut self, click: ClickAction) -> Self {
        self.click = Some(click);
        self
    }

    /// Attach file metadata.
    #[must_use]
    pub fn with_attachment(mut self, attachment: Attachment) -> Self {
        self.attachment = Some(attachment);
        self
    }

    #[must_use]
    pub fn topic(&self) -> &Topic {
        &self.topic
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    #[must_use]
    pub const fn priority(&self) -> Priority {
        self.priority
    }

    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    #[must_use]
    pub const fn click(&self) -> Option<&ClickAction> {
        self.click.as_ref()
    }

    #[must_use]
    pub const fn attachment(&self) -> Option<&Attachment> {
        self.attachment.as_ref()
    }

    #[must_use]
    pub const fn created_at(&self) -> Tick {
        self.created_at
    }

    /// A content fingerprint used for de-duplication: two notifications with the
    /// same topic, title and body collapse to the same key. Priority, tags and
    /// time are intentionally excluded — a re-send of the same alert is a
    /// duplicate even if its time moved on.
    #[must_use]
    pub fn dedup_key(&self) -> DedupKey {
        DedupKey {
            topic: self.topic.as_str().to_owned(),
            title: self.title.clone(),
            body: self.body.clone(),
        }
    }
}

/// The content fingerprint of a notification (topic + title + body).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupKey {
    topic: String,
    title: String,
    body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topic(name: &str) -> Topic {
        Topic::new(name).expect("valid test topic")
    }

    #[test]
    fn new_defaults_are_sane() {
        let n = Notification::new(topic("leak"), "Water leak", "Under the sink", 100);
        assert_eq!(n.priority(), Priority::Default);
        assert!(n.tags().is_empty());
        assert_eq!(n.click(), None);
        assert_eq!(n.attachment(), None);
        assert_eq!(n.created_at(), 100);
        assert_eq!(n.title(), "Water leak");
        assert_eq!(n.body(), "Under the sink");
        assert_eq!(n.topic().as_str(), "leak");
    }

    #[test]
    fn builders_set_optional_fields() {
        let n = Notification::new(topic("door"), "Front door", "Opened", 5)
            .with_priority(Priority::High)
            .with_tags(["security".to_owned(), "door".to_owned()])
            .with_click(ClickAction::OpenView("front-door".to_owned()))
            .with_attachment(Attachment {
                name: "snap.jpg".to_owned(),
                size_bytes: Some(2048),
            });
        assert_eq!(n.priority(), Priority::High);
        assert_eq!(n.tags(), ["security", "door"]);
        assert_eq!(n.click(), Some(&ClickAction::OpenView("front-door".to_owned())));
        assert_eq!(n.attachment().map(|a| a.name.as_str()), Some("snap.jpg"));
    }

    #[test]
    fn dedup_key_ignores_priority_tags_and_time() {
        let a = Notification::new(topic("leak"), "Leak", "Kitchen", 10)
            .with_priority(Priority::Max)
            .with_tags(["x".to_owned()]);
        let b = Notification::new(topic("leak"), "Leak", "Kitchen", 9999)
            .with_priority(Priority::Min);
        assert_eq!(a.dedup_key(), b.dedup_key());
    }

    #[test]
    fn dedup_key_separates_different_content() {
        let a = Notification::new(topic("leak"), "Leak", "Kitchen", 10);
        let b = Notification::new(topic("leak"), "Leak", "Bathroom", 10);
        let c = Notification::new(topic("door"), "Leak", "Kitchen", 10);
        assert_ne!(a.dedup_key(), b.dedup_key());
        assert_ne!(a.dedup_key(), c.dedup_key());
    }
}
