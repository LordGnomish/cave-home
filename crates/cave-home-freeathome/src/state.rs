// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Local datapoint state cache, fed by REST polls and WebSocket pushes.
//!
//! The cache is the single source of truth the bridges read from: a REST poll
//! seeds it, then WebSocket pushes keep it live. It is keyed by the canonical
//! `serial/channel/datapoint` address so REST and WS agree on identity.

use std::collections::BTreeMap;

use crate::event::{DatapointUpdate, FreeAtHomeEvent};

/// The last-known wire value of every datapoint we've observed.
#[derive(Debug, Clone, Default)]
pub struct StateCache {
    values: BTreeMap<String, String>,
}

impl StateCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one datapoint update, returning the previous value if any.
    pub fn apply(&mut self, update: &DatapointUpdate) -> Option<String> {
        self.values
            .insert(update.address(), update.value().to_string())
    }

    /// Apply any event; only datapoint updates change cached values.
    pub fn apply_event(&mut self, event: &FreeAtHomeEvent) {
        if let Some(update) = event.as_datapoint_update() {
            self.apply(update);
        }
    }

    /// The current value at a canonical `serial/channel/datapoint` address.
    pub fn get_by_address(&self, address: &str) -> Option<&str> {
        self.values.get(address).map(String::as_str)
    }

    /// Number of distinct datapoints tracked.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{DatapointUpdate, FreeAtHomeEvent};
    use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial, Direction};

    fn upd(dp_index: u16, value: &str) -> DatapointUpdate {
        DatapointUpdate::new(
            DeviceSerial::parse("ABB700C12345").expect("serial"),
            ChannelId::new(0),
            DatapointId::new(Direction::Output, dp_index),
            value.to_string(),
        )
    }

    #[test]
    fn apply_then_get() {
        let mut c = StateCache::new();
        c.apply(&upd(0, "1"));
        assert_eq!(c.get_by_address("ABB700C12345/ch0000/odp0000"), Some("1"));
    }

    #[test]
    fn overwrite_returns_previous() {
        let mut c = StateCache::new();
        assert_eq!(c.apply(&upd(0, "1")), None);
        assert_eq!(c.apply(&upd(0, "0")), Some("1".to_string()));
    }

    #[test]
    fn unknown_key_is_none() {
        let c = StateCache::new();
        assert_eq!(c.get_by_address("nope"), None);
    }

    #[test]
    fn len_tracks_distinct_datapoints() {
        let mut c = StateCache::new();
        c.apply(&upd(0, "1"));
        c.apply(&upd(1, "50"));
        c.apply(&upd(0, "0"));
        assert_eq!(c.len(), 2);
        assert!(!c.is_empty());
    }

    #[test]
    fn apply_event_updates_only_on_datapoint() {
        let mut c = StateCache::new();
        c.apply_event(&FreeAtHomeEvent::DatapointUpdate(upd(0, "1")));
        c.apply_event(&FreeAtHomeEvent::DeviceAdded(
            DeviceSerial::parse("ABB700C99999").expect("serial"),
        ));
        assert_eq!(c.len(), 1);
    }
}
