//! Port of `homeassistant.components.person`.
//!
//! A person aggregates one or more `device_tracker` entities into a single
//! presence state. [`Person::resolve_state`] ports the precedence HA uses:
//! `home` wins outright; otherwise the first tracker reporting a named zone is
//! taken; otherwise `not_home`; and with nothing usable, `unknown`.

use crate::entity::{STATE_UNAVAILABLE, STATE_UNKNOWN};
use crate::state::EntityId;
use crate::state_machine::StateMachine;
use crate::util::{ensure_unique_string, slugify};
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

/// `homeassistant.const.STATE_HOME` / `STATE_NOT_HOME`.
pub const STATE_HOME: &str = "home";
pub const STATE_NOT_HOME: &str = "not_home";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PersonError {
    #[error("person name must not be empty")]
    EmptyName,
    #[error("no person with id {0:?}")]
    UnknownId(String),
}

/// Port of a `person` config entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub user_id: Option<String>,
    pub device_trackers: Vec<EntityId>,
}

impl Person {
    /// Resolve this person's presence state from their trackers' current
    /// states in `states`, applying HA's precedence.
    #[must_use]
    pub fn resolve_state(&self, states: &StateMachine) -> String {
        // Collect the current state of every tracker that exists.
        let tracker_states: Vec<String> = self
            .device_trackers
            .iter()
            .filter_map(|t| states.get(t).map(|s| s.state))
            .collect();

        // `home` wins outright.
        if tracker_states.iter().any(|s| s == STATE_HOME) {
            return STATE_HOME.to_owned();
        }
        // Otherwise the first tracker reporting a named zone (anything that is
        // not an away/unknown sentinel) is taken.
        if let Some(zone) = tracker_states.iter().find(|s| {
            !matches!(
                s.as_str(),
                STATE_NOT_HOME | STATE_UNKNOWN | STATE_UNAVAILABLE
            )
        }) {
            return zone.clone();
        }
        // Otherwise, if any tracker is explicitly away, the person is away.
        if tracker_states.iter().any(|s| s == STATE_NOT_HOME) {
            return STATE_NOT_HOME.to_owned();
        }
        // Nothing usable.
        STATE_UNKNOWN.to_owned()
    }
}

#[derive(Default)]
struct PersonInner {
    people: BTreeMap<String, Person>,
}

/// Registry of [`Person`]s.
#[derive(Clone, Default)]
pub struct PersonRegistry {
    inner: Arc<RwLock<PersonInner>>,
}

impl PersonRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a person from a name (slug id) and optional linked user.
    ///
    /// # Errors
    /// [`PersonError::EmptyName`] if `name` slugs to nothing.
    pub fn create(
        &self,
        name: impl Into<String>,
        user_id: Option<String>,
    ) -> Result<Person, PersonError> {
        let name = name.into();
        let slug = slugify(&name);
        if slug.is_empty() {
            return Err(PersonError::EmptyName);
        }
        let mut guard = self.inner.write();
        let existing: HashSet<String> = guard.people.keys().cloned().collect();
        let id = ensure_unique_string(&slug, &existing);
        let person = Person {
            id: id.clone(),
            name,
            user_id,
            device_trackers: Vec::new(),
        };
        guard.people.insert(id, person.clone());
        drop(guard);
        Ok(person)
    }

    /// Attach a `device_tracker` to a person.
    ///
    /// # Errors
    /// [`PersonError::UnknownId`] if `id` is not registered.
    pub fn add_tracker(&self, id: &str, tracker: EntityId) -> Result<Person, PersonError> {
        let mut guard = self.inner.write();
        let Some(person) = guard.people.get_mut(id) else {
            return Err(PersonError::UnknownId(id.to_owned()));
        };
        if !person.device_trackers.contains(&tracker) {
            person.device_trackers.push(tracker);
        }
        let updated = person.clone();
        drop(guard);
        Ok(updated)
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Person> {
        self.inner.read().people.get(id).cloned()
    }

    #[must_use]
    pub fn list(&self) -> Vec<Person> {
        self.inner.read().people.values().cloned().collect()
    }

    /// Resolve a person's presence state, or `None` if `id` is unknown.
    #[must_use]
    pub fn state_of(&self, id: &str, states: &StateMachine) -> Option<String> {
        self.get(id).map(|p| p.resolve_state(states))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;
    use crate::state::StateAttributes;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn tracker(object: &str) -> Result<EntityId, crate::state::EntityIdError> {
        EntityId::new("device_tracker", object)
    }

    fn states_with(pairs: &[(&str, &str)]) -> Result<StateMachine, crate::state::EntityIdError> {
        let sm = StateMachine::new(crate::event_bus::EventBus::new());
        for (obj, st) in pairs {
            sm.set(tracker(obj)?, *st, StateAttributes::new(), Context::new());
        }
        Ok(sm)
    }

    #[test]
    fn home_wins_over_everything() -> TestResult {
        let p = Person {
            id: "alice".into(),
            name: "Alice".into(),
            user_id: None,
            device_trackers: vec![tracker("phone")?, tracker("watch")?],
        };
        let states = states_with(&[("phone", "not_home"), ("watch", STATE_HOME)])?;
        assert_eq!(p.resolve_state(&states), STATE_HOME);
        Ok(())
    }

    #[test]
    fn named_zone_beats_not_home() -> TestResult {
        let p = Person {
            id: "bob".into(),
            name: "Bob".into(),
            user_id: None,
            device_trackers: vec![tracker("phone")?, tracker("car")?],
        };
        let states = states_with(&[("phone", "not_home"), ("car", "Work")])?;
        assert_eq!(p.resolve_state(&states), "Work");
        Ok(())
    }

    #[test]
    fn all_away_is_not_home() -> TestResult {
        let p = Person {
            id: "carol".into(),
            name: "Carol".into(),
            user_id: None,
            device_trackers: vec![tracker("phone")?],
        };
        let states = states_with(&[("phone", "not_home")])?;
        assert_eq!(p.resolve_state(&states), STATE_NOT_HOME);
        Ok(())
    }

    #[test]
    fn no_usable_tracker_is_unknown() -> TestResult {
        // unknown/unavailable trackers and missing trackers → unknown
        let p = Person {
            id: "dave".into(),
            name: "Dave".into(),
            user_id: None,
            device_trackers: vec![tracker("phone")?, tracker("ghost")?],
        };
        let states = states_with(&[("phone", STATE_UNAVAILABLE)])?;
        assert_eq!(p.resolve_state(&states), STATE_UNKNOWN);

        // a person with no trackers at all is unknown
        let empty = Person {
            id: "eve".into(),
            name: "Eve".into(),
            user_id: None,
            device_trackers: vec![],
        };
        assert_eq!(empty.resolve_state(&states), STATE_UNKNOWN);
        Ok(())
    }

    #[test]
    fn registry_create_track_and_state_of() -> TestResult {
        let reg = PersonRegistry::new();
        let alice = reg.create("Alice Smith", Some("user-1".into()))?;
        assert_eq!(alice.id, "alice_smith");
        assert_eq!(alice.user_id.as_deref(), Some("user-1"));

        reg.add_tracker(&alice.id, tracker("phone")?)?;
        // adding the same tracker twice is idempotent
        let p = reg.add_tracker(&alice.id, tracker("phone")?)?;
        assert_eq!(p.device_trackers.len(), 1);

        let states = states_with(&[("phone", STATE_HOME)])?;
        assert_eq!(
            reg.state_of(&alice.id, &states).as_deref(),
            Some(STATE_HOME)
        );
        assert!(reg.state_of("nobody", &states).is_none());

        assert_eq!(
            reg.add_tracker("nobody", tracker("x")?)
                .err()
                .ok_or("expected an error")?,
            PersonError::UnknownId("nobody".into())
        );
        Ok(())
    }
}
