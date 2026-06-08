//! The [`ConfigEntry`]: one thing a household actually added.
//!
//! Where an [`crate::integration::Integration`] is *a kind of thing*, a
//! [`ConfigEntry`] is *one instance* — this Hue bridge, that camera — with its
//! own configuration, its own lifecycle [`State`], a `unique_id` for dedupe,
//! and a `disabled_by` flag. It owns no behaviour beyond bookkeeping and
//! validated state transitions; the transition rules live in
//! [`crate::lifecycle`].

use crate::lifecycle::{self, State, Transition, TransitionError};

/// Who disabled an entry, if anyone — mirrors HA's `disabled_by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledBy {
    /// The household turned it off.
    User,
    /// The system disabled it (e.g. a failed dependency).
    System,
}

/// One added integration instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    domain: String,
    title: String,
    /// Opaque config key/value pairs (host, port, token-handle…). Stored as
    /// data, never interpreted here.
    data: Vec<(String, String)>,
    state: State,
    unique_id: Option<String>,
    disabled_by: Option<DisabledBy>,
}

impl ConfigEntry {
    /// A fresh, not-yet-loaded entry for an integration domain.
    #[must_use]
    pub fn new(domain: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            title: title.into(),
            data: Vec::new(),
            state: State::NotLoaded,
            unique_id: None,
            disabled_by: None,
        }
    }

    /// Set the dedupe unique-id (typically from a discovery signal).
    #[must_use]
    pub fn with_unique_id(mut self, id: impl Into<String>) -> Self {
        self.unique_id = Some(id.into());
        self
    }

    /// Attach a config value.
    #[must_use]
    pub fn with_data(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.data.push((k.into(), v.into()));
        self
    }

    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    #[must_use]
    pub fn unique_id(&self) -> Option<&str> {
        self.unique_id.as_deref()
    }

    #[must_use]
    pub fn data(&self, key: &str) -> Option<&str> {
        self.data.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[must_use]
    pub const fn disabled_by(&self) -> Option<DisabledBy> {
        self.disabled_by
    }

    /// Whether this entry is disabled (and so should not be set up).
    #[must_use]
    pub const fn is_disabled(&self) -> bool {
        self.disabled_by.is_some()
    }

    /// Disable the entry. It stays known but is not driven through setup.
    pub const fn disable(&mut self, by: DisabledBy) {
        self.disabled_by = Some(by);
    }

    /// Re-enable a disabled entry.
    pub const fn enable(&mut self) {
        self.disabled_by = None;
    }

    /// Drive the entry through one lifecycle [`Transition`], updating its state
    /// in place on success.
    ///
    /// # Errors
    /// Returns [`TransitionError`] if the event is illegal for the current
    /// state (the state is left unchanged).
    pub fn apply(&mut self, event: Transition) -> Result<State, TransitionError> {
        let next = lifecycle::next(self.state, event)?;
        self.state = next;
        Ok(next)
    }
}

/// Whether adding `candidate` would duplicate an entry already in `existing`.
///
/// Two entries collide when they share a domain *and* a unique-id. An entry
/// with no unique-id can never be deduped automatically (HA semantics: such
/// integrations allow multiple instances).
#[must_use]
pub fn is_duplicate(existing: &[ConfigEntry], domain: &str, unique_id: Option<&str>) -> bool {
    let Some(uid) = unique_id else {
        return false;
    };
    existing
        .iter()
        .any(|e| e.domain() == domain && e.unique_id() == Some(uid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Failure;

    #[test]
    fn fresh_entry_is_not_loaded() {
        let e = ConfigEntry::new("hue", "Hue Bridge");
        assert_eq!(e.state(), State::NotLoaded);
        assert_eq!(e.unique_id(), None);
        assert!(!e.is_disabled());
    }

    #[test]
    fn builder_carries_data_and_unique_id() {
        let e = ConfigEntry::new("hue", "Hue Bridge")
            .with_unique_id("hue:AA11")
            .with_data("host", "10.0.0.5");
        assert_eq!(e.unique_id(), Some("hue:AA11"));
        assert_eq!(e.data("host"), Some("10.0.0.5"));
        assert_eq!(e.data("port"), None);
    }

    #[test]
    fn apply_drives_state_and_rejects_illegal() {
        let mut e = ConfigEntry::new("hue", "Hue Bridge");
        e.apply(Transition::BeginSetup).expect("begin");
        assert_eq!(e.state(), State::SettingUp);
        e.apply(Transition::SetupSucceeded).expect("succeed");
        assert_eq!(e.state(), State::Loaded);
        // Illegal event leaves the state untouched.
        let before = e.state();
        assert!(e.apply(Transition::SetupSucceeded).is_err());
        assert_eq!(e.state(), before);
    }

    #[test]
    fn transient_failure_recorded_on_entry() {
        let mut e = ConfigEntry::new("cloud", "Cloud thing");
        e.apply(Transition::BeginSetup).expect("begin");
        e.apply(Transition::SetupFailed(Failure::Unreachable)).expect("fail");
        assert_eq!(e.state(), State::SetupRetry);
        assert!(e.state().wants_retry());
    }

    #[test]
    fn disable_and_enable() {
        let mut e = ConfigEntry::new("hue", "Hue Bridge");
        e.disable(DisabledBy::User);
        assert!(e.is_disabled());
        assert_eq!(e.disabled_by(), Some(DisabledBy::User));
        e.enable();
        assert!(!e.is_disabled());
    }

    #[test]
    fn duplicate_detection_by_domain_and_unique_id() {
        let existing = vec![ConfigEntry::new("hue", "Hue").with_unique_id("hue:AA11")];
        assert!(is_duplicate(&existing, "hue", Some("hue:AA11")));
        // Same id, different domain -> not a duplicate.
        assert!(!is_duplicate(&existing, "esphome", Some("hue:AA11")));
        // Different id -> not a duplicate.
        assert!(!is_duplicate(&existing, "hue", Some("hue:BB22")));
    }

    #[test]
    fn entries_without_unique_id_never_dedupe() {
        let existing = vec![ConfigEntry::new("manual", "Manual A")];
        assert!(!is_duplicate(&existing, "manual", None));
    }
}
