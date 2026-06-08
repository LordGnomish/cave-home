//! Port of `homeassistant.core.State` + `EntityId`.
//!
//! HA models every device as an entity in the form `domain.object_id`
//! (`light.kitchen`, `binary_sensor.front_door`). A `State` is a
//! snapshot: id, textual state value, attribute bag, and two
//! timestamps — `last_changed` (updated only when the state value
//! actually changes) and `last_updated` (updated on every report,
//! including attribute-only ones).

use crate::context::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId {
    pub domain: String,
    pub object_id: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EntityIdError {
    #[error("entity id must be domain.object_id: {0:?}")]
    Invalid(String),
}

impl EntityId {
    pub fn new(domain: impl Into<String>, object_id: impl Into<String>) -> Result<Self, EntityIdError> {
        let d: String = domain.into();
        let o: String = object_id.into();
        let ok = |s: &str| {
            !s.is_empty()
                && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        if !ok(&d) || !ok(&o) {
            return Err(EntityIdError::Invalid(format!("{d}.{o}")));
        }
        Ok(Self { domain: d, object_id: o })
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.domain, self.object_id)
    }
}

impl FromStr for EntityId {
    type Err = EntityIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '.');
        let d = parts.next().ok_or_else(|| EntityIdError::Invalid(s.to_owned()))?;
        let o = parts.next().ok_or_else(|| EntityIdError::Invalid(s.to_owned()))?;
        if o.contains('.') {
            return Err(EntityIdError::Invalid(s.to_owned()));
        }
        Self::new(d, o)
    }
}

pub type StateAttributes = BTreeMap<String, serde_json::Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub entity_id: EntityId,
    pub state: String,
    pub attributes: StateAttributes,
    #[serde(with = "time::serde::rfc3339")]
    pub last_changed: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_updated: OffsetDateTime,
    pub context: Context,
}

impl State {
    pub fn new(
        entity_id: EntityId,
        state: impl Into<String>,
        attributes: StateAttributes,
        context: Context,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            entity_id,
            state: state.into(),
            attributes,
            last_changed: now,
            last_updated: now,
            context,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_round_trip_and_validation() {
        let id: EntityId = "light.kitchen".parse().expect("parse");
        assert_eq!(id.to_string(), "light.kitchen");
        assert!("LIGHT.kitchen".parse::<EntityId>().is_err());
        assert!("light.kitchen.x".parse::<EntityId>().is_err());
        assert!("light.".parse::<EntityId>().is_err());
    }

    #[test]
    fn entity_id_new_validates_both_halves() {
        assert!(EntityId::new("binary_sensor", "front_door").is_ok());
        assert!(EntityId::new("sensor", "temp_2").is_ok());
        // empty halves rejected
        assert!(EntityId::new("", "kitchen").is_err());
        assert!(EntityId::new("light", "").is_err());
        // uppercase / hyphen / space rejected (HA slug grammar)
        assert!(EntityId::new("Light", "kitchen").is_err());
        assert!(EntityId::new("light", "Kitchen").is_err());
        assert!(EntityId::new("light", "front-door").is_err());
        assert!(EntityId::new("light", "front door").is_err());
    }

    #[test]
    fn entity_id_from_str_no_dot_is_error() {
        assert!("lightkitchen".parse::<EntityId>().is_err());
        // leading/trailing dot variants
        assert!(".kitchen".parse::<EntityId>().is_err());
        assert!("light.kitchen.".parse::<EntityId>().is_err());
    }

    #[test]
    fn state_serde_round_trip() {
        let id: EntityId = "light.kitchen".parse().expect("parse");
        let mut attrs = StateAttributes::new();
        attrs.insert("brightness".into(), serde_json::json!(128));
        let st = State::new(id, "on", attrs, Context::new());
        let s = serde_json::to_string(&st).expect("serialise");
        let back: State = serde_json::from_str(&s).expect("deserialise");
        assert_eq!(back.state, "on");
        assert_eq!(back.attributes["brightness"], 128);
        assert_eq!(back.entity_id.to_string(), "light.kitchen");
        // fresh State has last_changed == last_updated
        assert_eq!(back.last_changed, back.last_updated);
    }
}
