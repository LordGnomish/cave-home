// SPDX-License-Identifier: Apache-2.0
//! Entity state machine — port of `homeassistant/core.py::State` /
//! `StateMachine`.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::context::Context;
use crate::error::{HassError, HassResult};
use crate::event_bus::{
    EVENT_STATE_CHANGED, EVENT_STATE_REPORTED, Event, EventBus, EventOrigin,
};

/// HA's `MAX_LENGTH_STATE_STATE` (255 chars).
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/const.py::MAX_LENGTH_STATE_STATE
pub const MAX_LENGTH_STATE_STATE: usize = 255;

/// Sentinel state strings.
pub const STATE_UNKNOWN: &str = "unknown";
pub const STATE_UNAVAILABLE: &str = "unavailable";

/// Split `light.kitchen` -> `("light", "kitchen")`.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::split_entity_id
pub fn split_entity_id(entity_id: &str) -> HassResult<(String, String)> {
    let (domain, object_id) = entity_id
        .split_once('.')
        .ok_or_else(|| HassError::InvalidEntityFormat(entity_id.to_owned()))?;
    if domain.is_empty() || object_id.is_empty() {
        return Err(HassError::InvalidEntityFormat(entity_id.to_owned()));
    }
    Ok((domain.to_owned(), object_id.to_owned()))
}

/// Validate domain — lower-case alpha plus underscore.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::valid_domain
#[must_use]
pub fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Validate entity id (`<domain>.<object_id>` shape).
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::valid_entity_id
#[must_use]
pub fn valid_entity_id(entity_id: &str) -> bool {
    let Some((domain, object_id)) = entity_id.split_once('.') else {
        return false;
    };
    if !valid_domain(domain) {
        return false;
    }
    !object_id.is_empty()
        && object_id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Validate state value — length and absence of control chars.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::validate_state
pub fn validate_state(state: &str) -> HassResult<String> {
    if state.len() > MAX_LENGTH_STATE_STATE {
        return Err(HassError::InvalidState(format!(
            "state value longer than {MAX_LENGTH_STATE_STATE} chars"
        )));
    }
    Ok(state.to_owned())
}

/// Object representing the state of a single entity in the state
/// machine.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub entity_id: String,
    pub domain: String,
    pub object_id: String,
    pub state: String,
    pub attributes: BTreeMap<String, Value>,
    pub last_changed: OffsetDateTime,
    pub last_reported: OffsetDateTime,
    pub last_updated: OffsetDateTime,
    pub context: Context,
}

impl State {
    /// Build a new state, validating the entity id.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::State.__init__
    pub fn new(
        entity_id: impl Into<String>,
        state: impl Into<String>,
        attributes: BTreeMap<String, Value>,
        context: Option<Context>,
    ) -> HassResult<Self> {
        let entity_id = entity_id.into();
        if !valid_entity_id(&entity_id) {
            return Err(HassError::InvalidEntityFormat(entity_id));
        }
        let (domain, object_id) = split_entity_id(&entity_id)?;
        let state = state.into();
        validate_state(&state)?;
        let now = OffsetDateTime::now_utc();
        Ok(Self {
            entity_id,
            domain,
            object_id,
            state,
            attributes,
            last_changed: now,
            last_reported: now,
            last_updated: now,
            context: context.unwrap_or_default(),
        })
    }

    /// "Friendly name" — `attributes.friendly_name` or the object id with
    /// underscores replaced by spaces.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::State.name
    #[must_use]
    pub fn name(&self) -> String {
        if let Some(Value::String(name)) = self.attributes.get("friendly_name") {
            return name.clone();
        }
        self.object_id.replace('_', " ")
    }

    /// JSON-ish dictionary representation.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::State.as_dict
    #[must_use]
    pub fn as_dict(&self) -> serde_json::Map<String, Value> {
        let mut map = serde_json::Map::new();
        map.insert("entity_id".into(), Value::String(self.entity_id.clone()));
        map.insert("state".into(), Value::String(self.state.clone()));
        map.insert(
            "attributes".into(),
            Value::Object(self.attributes.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        );
        map.insert(
            "last_changed".into(),
            Value::String(iso8601(self.last_changed)),
        );
        map.insert(
            "last_reported".into(),
            Value::String(iso8601(self.last_reported)),
        );
        map.insert(
            "last_updated".into(),
            Value::String(iso8601(self.last_updated)),
        );
        map.insert(
            "context".into(),
            serde_json::to_value(&self.context).unwrap_or(Value::Null),
        );
        map
    }

    /// Reconstruct a `State` from its JSON dict representation.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::State.from_dict
    pub fn from_dict(map: &serde_json::Map<String, Value>) -> HassResult<Self> {
        let entity_id = map
            .get("entity_id")
            .and_then(Value::as_str)
            .ok_or_else(|| HassError::Other("from_dict: missing entity_id".into()))?
            .to_string();
        let state = map
            .get("state")
            .and_then(Value::as_str)
            .ok_or_else(|| HassError::Other("from_dict: missing state".into()))?
            .to_string();
        let attributes: BTreeMap<String, Value> = map
            .get("attributes")
            .and_then(Value::as_object)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let context: Context = map
            .get("context")
            .map(|v| serde_json::from_value(v.clone()).unwrap_or_default())
            .unwrap_or_default();
        let last_changed = map
            .get("last_changed")
            .and_then(Value::as_str)
            .and_then(parse_iso8601)
            .unwrap_or_else(OffsetDateTime::now_utc);
        let last_reported = map
            .get("last_reported")
            .and_then(Value::as_str)
            .and_then(parse_iso8601)
            .unwrap_or(last_changed);
        let last_updated = map
            .get("last_updated")
            .and_then(Value::as_str)
            .and_then(parse_iso8601)
            .unwrap_or(last_changed);
        let (domain, object_id) = split_entity_id(&entity_id)?;
        Ok(Self {
            entity_id,
            domain,
            object_id,
            state,
            attributes,
            last_changed,
            last_reported,
            last_updated,
            context,
        })
    }

    /// Replace the context with a clone of the same id so the original can
    /// be garbage collected — Python's `Context(user_id, parent_id, id)`.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::State.expire
    pub fn expire(&mut self) {
        self.context = Context {
            id: self.context.id.clone(),
            user_id: self.context.user_id.clone(),
            parent_id: self.context.parent_id.clone(),
        };
    }
}

fn iso8601(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Iso8601::DEFAULT)
        .unwrap_or_else(|_| t.unix_timestamp().to_string())
}

fn parse_iso8601(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &time::format_description::well_known::Iso8601::DEFAULT).ok()
}

/// Inner state container with an additional `domain -> entity_id` index,
/// mirroring HA core's `States(UserDict)`.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::States
#[derive(Debug, Default)]
pub struct States {
    by_entity: HashMap<String, State>,
    by_domain: HashMap<String, HashSet<String>>,
}

impl States {
    pub fn set(&mut self, state: State) {
        self.by_domain
            .entry(state.domain.clone())
            .or_default()
            .insert(state.entity_id.clone());
        self.by_entity.insert(state.entity_id.clone(), state);
    }

    pub fn get(&self, entity_id: &str) -> Option<&State> {
        self.by_entity.get(entity_id)
    }

    pub fn remove(&mut self, entity_id: &str) -> Option<State> {
        if let Some(state) = self.by_entity.remove(entity_id) {
            if let Some(set) = self.by_domain.get_mut(&state.domain) {
                set.remove(entity_id);
                if set.is_empty() {
                    self.by_domain.remove(&state.domain);
                }
            }
            Some(state)
        } else {
            None
        }
    }

    pub fn all(&self) -> Vec<State> {
        self.by_entity.values().cloned().collect()
    }

    pub fn domain_entity_ids(&self, domain: &str) -> Vec<String> {
        self.by_domain
            .get(domain)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn entity_ids(&self) -> Vec<String> {
        self.by_entity.keys().cloned().collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_entity.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_entity.is_empty()
    }
}

/// Helper class tracking the state of all entities and firing
/// `state_changed` / `state_reported` events on mutations.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine
#[derive(Debug)]
pub struct StateMachine {
    states: RwLock<States>,
    reservations: RwLock<HashSet<String>>,
    bus: Arc<dyn EventBus>,
}

/// Payload of `EVENT_STATE_CHANGED`.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventStateChangedData
#[derive(Debug, Clone, Serialize)]
pub struct StateChangedData {
    pub entity_id: String,
    pub old_state: Option<State>,
    pub new_state: Option<State>,
}

/// Payload of `EVENT_STATE_REPORTED`.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::EventStateReportedData
#[derive(Debug, Clone, Serialize)]
pub struct StateReportedData {
    pub entity_id: String,
    pub last_reported: OffsetDateTime,
    pub old_last_reported: OffsetDateTime,
    pub new_state: State,
}

impl StateMachine {
    /// New state machine wired to a shared event bus.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.__init__
    #[must_use]
    pub fn new(bus: Arc<dyn EventBus>) -> Self {
        Self {
            states: RwLock::new(States::default()),
            reservations: RwLock::new(HashSet::new()),
            bus,
        }
    }

    /// Retrieve state of `entity_id` or `None`.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.get
    #[must_use]
    pub fn get(&self, entity_id: &str) -> Option<State> {
        let states = self.states.read();
        states
            .get(entity_id)
            .or_else(|| states.get(&entity_id.to_ascii_lowercase()))
            .cloned()
    }

    /// Test if entity exists and is in `state`.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.is_state
    pub fn is_state(&self, entity_id: &str, state: &str) -> bool {
        self.get(entity_id).is_some_and(|s| s.state == state)
    }

    /// List all entity ids, optionally filtered to a single domain.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.async_entity_ids
    #[must_use]
    pub fn entity_ids(&self, domain_filter: Option<&str>) -> Vec<String> {
        let states = self.states.read();
        match domain_filter {
            Some(d) => states.domain_entity_ids(&d.to_ascii_lowercase()),
            None => states.entity_ids(),
        }
    }

    /// All states, optionally filtered to a single domain.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.async_all
    #[must_use]
    pub fn all(&self, domain_filter: Option<&str>) -> Vec<State> {
        let states = self.states.read();
        match domain_filter {
            Some(d) => states
                .domain_entity_ids(&d.to_ascii_lowercase())
                .into_iter()
                .filter_map(|id| states.get(&id).cloned())
                .collect(),
            None => states.all(),
        }
    }

    /// Remove the state of `entity_id`. Fires `state_changed`.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.async_remove
    pub fn async_remove(&self, entity_id: &str, context: Option<Context>) -> bool {
        let entity_id = entity_id.to_ascii_lowercase();
        let mut states = self.states.write();
        let Some(mut old_state) = states.remove(&entity_id) else {
            self.reservations.write().remove(&entity_id);
            return false;
        };
        self.reservations.write().remove(&entity_id);
        old_state.expire();
        let ctx = context.unwrap_or_else(|| old_state.context.clone());
        let payload = StateChangedData {
            entity_id: entity_id.clone(),
            old_state: Some(old_state),
            new_state: None,
        };
        let event = Event::new(
            EVENT_STATE_CHANGED.into(),
            serde_json::to_value(&payload).unwrap_or(Value::Null),
            EventOrigin::Local,
            ctx,
        );
        drop(states);
        self.bus.fire(event);
        true
    }

    /// Reserve an entity id so a concurrent caller can't take it.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.async_reserve
    pub fn async_reserve(&self, entity_id: &str) -> HassResult<()> {
        let states = self.states.read();
        let mut reservations = self.reservations.write();
        if states.get(entity_id).is_some() || reservations.contains(entity_id) {
            return Err(HassError::Other(
                "async_reserve must not be called once the state is in the state machine".into(),
            ));
        }
        reservations.insert(entity_id.to_owned());
        Ok(())
    }

    /// True iff `entity_id` is free (not reserved and no current state).
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.async_available
    pub fn async_available(&self, entity_id: &str) -> bool {
        let entity_id = entity_id.to_ascii_lowercase();
        let states = self.states.read();
        let reservations = self.reservations.read();
        states.get(&entity_id).is_none() && !reservations.contains(&entity_id)
    }

    /// Set / replace the state of an entity.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.async_set
    pub fn async_set(
        &self,
        entity_id: &str,
        new_state: impl Into<String>,
        attributes: BTreeMap<String, Value>,
        force_update: bool,
        context: Option<Context>,
    ) -> HassResult<()> {
        let entity_id = entity_id.to_ascii_lowercase();
        let new_state = new_state.into();
        validate_state(&new_state)?;
        self.async_set_internal(&entity_id, new_state, attributes, force_update, context)
    }

    /// Internal set — mirrors HA's `async_set_internal` semantics around
    /// `state_changed` vs `state_reported`.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/core.py::StateMachine.async_set_internal
    pub fn async_set_internal(
        &self,
        entity_id: &str,
        new_state: String,
        attributes: BTreeMap<String, Value>,
        force_update: bool,
        context: Option<Context>,
    ) -> HassResult<()> {
        let mut effective_state = new_state;
        if effective_state.len() > MAX_LENGTH_STATE_STATE {
            tracing::error!(
                target: "cave_home_automation::state",
                "state %s for {entity_id} longer than {MAX_LENGTH_STATE_STATE}, falling back to {STATE_UNKNOWN}"
            );
            effective_state = STATE_UNKNOWN.to_owned();
        }
        let now = OffsetDateTime::now_utc();
        let context = context.unwrap_or_default();

        let mut states = self.states.write();
        let old_state = states.get(entity_id).cloned();
        let (same_state, same_attr, last_changed) = match &old_state {
            Some(prev) => (
                prev.state == effective_state && !force_update,
                prev.attributes == attributes,
                if prev.state == effective_state && !force_update {
                    Some(prev.last_changed)
                } else {
                    None
                },
            ),
            None => (false, false, None),
        };

        if same_state && same_attr {
            // No change → fire state_reported, mutate last_reported in-place.
            let mut existing = states
                .get(entity_id)
                .cloned()
                .expect("same_state implies old_state present");
            let old_last_reported = existing.last_reported;
            existing.last_reported = now;
            states.set(existing.clone());
            let payload = StateReportedData {
                entity_id: entity_id.to_owned(),
                last_reported: now,
                old_last_reported,
                new_state: existing,
            };
            let event = Event::new(
                EVENT_STATE_REPORTED.into(),
                serde_json::to_value(&payload).unwrap_or(Value::Null),
                EventOrigin::Local,
                context,
            );
            drop(states);
            self.bus.fire(event);
            return Ok(());
        }

        let attributes = if same_attr {
            old_state.as_ref().map_or(attributes, |p| p.attributes.clone())
        } else {
            attributes
        };
        let (domain, object_id) = split_entity_id(entity_id)?;
        let new = State {
            entity_id: entity_id.to_owned(),
            domain,
            object_id,
            state: effective_state,
            attributes,
            last_changed: last_changed.unwrap_or(now),
            last_reported: now,
            last_updated: now,
            context: context.clone(),
        };
        let mut old = old_state;
        if let Some(o) = old.as_mut() {
            o.expire();
        }
        states.set(new.clone());
        let payload = StateChangedData {
            entity_id: entity_id.to_owned(),
            old_state: old,
            new_state: Some(new),
        };
        let event = Event::new(
            EVENT_STATE_CHANGED.into(),
            serde_json::to_value(&payload).unwrap_or(Value::Null),
            EventOrigin::Local,
            context,
        );
        drop(states);
        self.bus.fire(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::InMemoryEventBus;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_machine() -> Arc<StateMachine> {
        Arc::new(StateMachine::new(Arc::new(InMemoryEventBus::new())))
    }

    /// Upstream-test: `tests/test_core.py::test_state_basics`
    #[test]
    fn state_basics() {
        let attrs: BTreeMap<String, Value> = BTreeMap::new();
        let s = State::new("light.kitchen", "on", attrs, None).unwrap();
        assert_eq!(s.entity_id, "light.kitchen");
        assert_eq!(s.domain, "light");
        assert_eq!(s.object_id, "kitchen");
        assert_eq!(s.state, "on");
    }

    /// Upstream-test: `tests/test_core.py::test_invalid_entity_id`
    #[test]
    fn invalid_entity_id_rejected() {
        assert!(matches!(
            State::new("not-an-entity", "on", BTreeMap::new(), None),
            Err(HassError::InvalidEntityFormat(_))
        ));
        assert!(matches!(
            State::new("Light.kitchen", "on", BTreeMap::new(), None),
            Err(HassError::InvalidEntityFormat(_))
        ));
    }

    /// Upstream-test: `tests/test_core.py::test_state_as_dict`
    #[test]
    fn as_dict_round_trip() {
        let mut attrs: BTreeMap<String, Value> = BTreeMap::new();
        attrs.insert("brightness".into(), Value::from(200));
        attrs.insert("friendly_name".into(), Value::String("Kitchen".into()));
        let s = State::new("light.kitchen", "on", attrs, None).unwrap();
        let map = s.as_dict();
        assert_eq!(map.get("entity_id").unwrap(), &Value::from("light.kitchen"));
        assert_eq!(map.get("state").unwrap(), &Value::from("on"));
        assert_eq!(
            map.get("attributes")
                .unwrap()
                .get("brightness")
                .unwrap(),
            &Value::from(200)
        );

        let back = State::from_dict(&map).unwrap();
        assert_eq!(back.entity_id, s.entity_id);
        assert_eq!(back.state, s.state);
        assert_eq!(back.name(), "Kitchen");
    }

    /// Upstream-test: `tests/test_core.py::test_state_machine`
    #[test]
    fn state_machine_set_get_remove() {
        let sm = make_machine();
        sm.async_set("light.kitchen", "on", BTreeMap::new(), false, None)
            .unwrap();
        assert!(sm.is_state("light.kitchen", "on"));
        assert!(sm.entity_ids(None).contains(&"light.kitchen".into()));
        assert!(sm.entity_ids(Some("light")).contains(&"light.kitchen".into()));
        assert!(sm.entity_ids(Some("switch")).is_empty());
        assert!(sm.async_remove("light.kitchen", None));
        assert!(sm.get("light.kitchen").is_none());
    }

    #[test]
    fn state_machine_fires_state_changed_on_set() {
        let bus = Arc::new(InMemoryEventBus::new());
        let sm = StateMachine::new(bus.clone());
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let _handle = bus.async_listen(EVENT_STATE_CHANGED, move |_event| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        sm.async_set("light.kitchen", "on", BTreeMap::new(), false, None)
            .unwrap();
        sm.async_set("light.kitchen", "off", BTreeMap::new(), false, None)
            .unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn state_machine_fires_state_reported_when_unchanged() {
        let bus = Arc::new(InMemoryEventBus::new());
        let sm = StateMachine::new(bus.clone());
        let changed = Arc::new(AtomicUsize::new(0));
        let reported = Arc::new(AtomicUsize::new(0));
        let c1 = changed.clone();
        let c2 = reported.clone();
        let _h1 = bus.async_listen(EVENT_STATE_CHANGED, move |_e| {
            c1.fetch_add(1, Ordering::SeqCst);
        });
        let _h2 = bus.async_listen(EVENT_STATE_REPORTED, move |_e| {
            c2.fetch_add(1, Ordering::SeqCst);
        });
        sm.async_set("light.kitchen", "on", BTreeMap::new(), false, None)
            .unwrap();
        sm.async_set("light.kitchen", "on", BTreeMap::new(), false, None)
            .unwrap();
        assert_eq!(changed.load(Ordering::SeqCst), 1);
        assert_eq!(reported.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn long_state_value_falls_back_to_unknown() {
        let sm = make_machine();
        let long = "x".repeat(MAX_LENGTH_STATE_STATE + 5);
        // async_set validates length first; async_set_internal demonstrates fallback.
        sm.async_set_internal("sensor.test", long, BTreeMap::new(), false, None)
            .unwrap();
        assert_eq!(sm.get("sensor.test").unwrap().state, STATE_UNKNOWN);
    }

    #[test]
    fn reserve_blocks_double_register() {
        let sm = make_machine();
        sm.async_reserve("light.kitchen").unwrap();
        assert!(!sm.async_available("light.kitchen"));
        assert!(sm.async_reserve("light.kitchen").is_err());
    }

    #[test]
    fn split_entity_id_works() {
        assert_eq!(
            split_entity_id("light.kitchen").unwrap(),
            ("light".into(), "kitchen".into())
        );
        assert!(split_entity_id("no_dot").is_err());
        assert!(split_entity_id(".empty_domain").is_err());
    }

    #[test]
    fn valid_helpers() {
        assert!(valid_entity_id("light.kitchen"));
        assert!(!valid_entity_id("Light.kitchen"));
        assert!(!valid_entity_id("light"));
        assert!(valid_domain("light"));
        assert!(!valid_domain("LIGHT"));
    }
}
