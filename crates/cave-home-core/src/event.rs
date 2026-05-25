//! Port of `homeassistant.core.Event`.
//!
//! Events carry a string `event_type` (e.g. `state_changed`), a JSON
//! `data` payload, an `origin` (Local vs Remote — Remote is reserved
//! for events arriving over the websocket API from a paired instance),
//! the wall-clock `time_fired`, and a `Context` for tracing.

use crate::context::Context;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventOrigin {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "remote")]
    Remote,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub event_type: String,
    pub data: serde_json::Value,
    pub origin: EventOrigin,
    #[serde(with = "time::serde::rfc3339")]
    pub time_fired: OffsetDateTime,
    pub context: Context,
}

impl Event {
    pub fn new(
        event_type: impl Into<String>,
        data: serde_json::Value,
        origin: EventOrigin,
        context: Context,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            data,
            origin,
            time_fired: OffsetDateTime::now_utc(),
            context,
        }
    }

    pub fn local(event_type: impl Into<String>, data: serde_json::Value) -> Self {
        Self::new(event_type, data, EventOrigin::Local, Context::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn local_event_origin_and_round_trip() {
        let evt = Event::local("state_changed", json!({"entity_id": "light.kitchen"}));
        assert_eq!(evt.origin, EventOrigin::Local);

        let s = serde_json::to_string(&evt).expect("serialise");
        let back: Event = serde_json::from_str(&s).expect("deserialise");
        assert_eq!(back.event_type, "state_changed");
        assert_eq!(back.origin, EventOrigin::Local);
    }
}
