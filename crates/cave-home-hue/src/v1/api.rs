// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v1/api.py
//! `APIItems` — the v1 client's map-of-resources base class.
//!
//! Upstream is a tiny dynamic Python class that holds a dict of items keyed
//! by ID, refreshes them from the bridge, and supports `[]` / `iter()` /
//! `values()`. Rust port preserves the same operations and the same diffing
//! behaviour (items missing from a new fetch are removed from the map).

use crate::errors::HueResult;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// HTTP transport used by every v1 sub-controller. One bridge => one client.
///
/// Source: implicit `_request` callable passed into every `APIItems`
/// subclass — `aiohue.v1.api.APIItems.__init__`.
#[async_trait]
pub trait V1Request: Send + Sync {
    /// HTTP GET against a resource path under `/api/<app_key>/<path>`.
    async fn get(&self, path: &str) -> HueResult<Value>;
    /// HTTP PUT (no return-value distinction beyond success / error).
    async fn put(&self, path: &str, body: Value) -> HueResult<Value>;
    /// HTTP POST — used by scene-creation, group-creation.
    async fn post(&self, path: &str, body: Value) -> HueResult<Value>;
    /// HTTP DELETE.
    async fn delete(&self, path: &str) -> HueResult<Value>;
}

/// Convenience newtype for "raw JSON object for a single resource".
pub type RawItem = serde_json::Map<String, Value>;

/// Trait every v1 item type implements (Lights, Groups, Scenes, ...).
/// Source: pattern used across `aiohue.v1.*` — each item type has
/// `ITEM_TYPE` + an `(id, raw, request)` constructor.
pub trait V1Item {
    /// The path segment used in the bridge API (e.g. "lights").
    const ITEM_TYPE: &'static str;
    /// Construct from raw JSON.
    fn from_raw(id: String, raw: RawItem) -> Self;
    /// Mutate raw in-place — preserves the upstream "raw is mutable" idiom.
    fn set_raw(&mut self, raw: RawItem);
}

/// Map of v1 items. Port of `aiohue.v1.api.APIItems`.
pub struct ApiItems<T: V1Item> {
    items: HashMap<String, T>,
    path: String,
}

impl<T: V1Item> ApiItems<T> {
    /// Build an empty collection wired to the given path.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            items: HashMap::new(),
            path: path.into(),
        }
    }

    /// Replace the collection's contents from raw bridge JSON.
    /// Source: `APIItems._process_raw`.
    pub fn process_raw(&mut self, raw: serde_json::Map<String, Value>) {
        // Drop ids that are no longer in the bridge response. An id whose
        // new value is not an object is still "in the response": it is kept
        // here and left untouched below.
        self.items.retain(|id, _| raw.contains_key(id.as_str()));
        // Update / insert each present id, moving its object out of `raw`.
        for (id, raw_item) in raw {
            let Value::Object(obj) = raw_item else {
                continue;
            };
            if let Some(existing) = self.items.get_mut(&id) {
                existing.set_raw(obj);
            } else {
                self.items.insert(id.clone(), T::from_raw(id, obj));
            }
        }
    }

    /// Refresh from the bridge.
    pub async fn update(&mut self, req: &dyn V1Request) -> HueResult<()> {
        let value = req.get(&self.path).await?;
        if let Value::Object(map) = value {
            self.process_raw(map);
        }
        Ok(())
    }

    /// Iterator over `(id, &item)` pairs in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        self.items.iter()
    }

    /// Get by id — `aiohue.v1.api.APIItems.__getitem__`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&T> {
        self.items.get(id)
    }

    /// Mutable get — used by sub-controllers that mutate raw fields.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut T> {
        self.items.get_mut(id)
    }

    /// Number of items currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if the collection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Test double shared by the v1 controller tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::V1Request;
    use crate::errors::HueResult;
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use serde_json::Value;

    /// `value` as a raw bridge map; an error if it is not a JSON object.
    pub fn raw_map(value: Value) -> Result<super::RawItem, &'static str> {
        match value {
            Value::Object(map) => Ok(map),
            _ => Err("the fixture is not a JSON object"),
        }
    }

    /// Records every PUT so a test can compare path and body exactly.
    #[derive(Default)]
    pub struct RecordingReq {
        pub puts: Mutex<Vec<(String, Value)>>,
    }

    #[async_trait]
    impl V1Request for RecordingReq {
        async fn get(&self, _path: &str) -> HueResult<Value> {
            Ok(Value::Null)
        }
        async fn put(&self, path: &str, body: Value) -> HueResult<Value> {
            self.puts.lock().push((path.into(), body));
            Ok(Value::Null)
        }
        async fn post(&self, _path: &str, _body: Value) -> HueResult<Value> {
            Ok(Value::Null)
        }
        async fn delete(&self, _path: &str) -> HueResult<Value> {
            Ok(Value::Null)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::raw_map;
    use super::*;
    use serde_json::json;

    #[derive(Debug, PartialEq)]
    struct DummyItem {
        id: String,
        raw: RawItem,
    }
    impl V1Item for DummyItem {
        const ITEM_TYPE: &'static str = "lights";
        fn from_raw(id: String, raw: RawItem) -> Self {
            Self { id, raw }
        }
        fn set_raw(&mut self, raw: RawItem) {
            self.raw = raw;
        }
    }

    #[test]
    fn process_raw_inserts_new_items() -> TestResult {
        let mut items = ApiItems::<DummyItem>::new("lights");
        let raw = json!({
            "1": {"name": "Mutfak"},
            "2": {"name": "Salon"},
        });
        items.process_raw(raw_map(raw)?);
        assert_eq!(items.len(), 2);
        assert!(items.get("1").is_some());
        assert!(items.get("2").is_some());
        Ok(())
    }

    #[test]
    fn process_raw_removes_disappeared_items() -> TestResult {
        let mut items = ApiItems::<DummyItem>::new("lights");
        items.process_raw(raw_map(json!({"1": {"name": "A"}, "2": {"name": "B"}}))?);
        // Second fetch drops id "2".
        items.process_raw(raw_map(json!({"1": {"name": "A"}}))?);
        assert_eq!(items.len(), 1);
        assert!(items.get("2").is_none());
        Ok(())
    }

    #[test]
    fn process_raw_mutates_existing_items_in_place() -> TestResult {
        let mut items = ApiItems::<DummyItem>::new("lights");
        items.process_raw(raw_map(json!({"1": {"name": "A"}}))?);
        items.process_raw(raw_map(json!({"1": {"name": "B"}}))?);
        let item = items
            .get("1")
            .ok_or("item 1 is missing after the refresh")?;
        assert_eq!(item.raw.get("name").ok_or("item 1 has no `name`")?, "B");
        Ok(())
    }

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn process_raw_keeps_an_item_whose_new_value_is_not_an_object() -> TestResult {
        let mut items = ApiItems::<DummyItem>::new("lights");
        items.process_raw(raw_map(json!({"1": {"name": "A"}, "2": {"name": "B"}}))?);
        // "1" is still listed but carries no object: it stays as it was.
        // "2" is gone. "3" and "4" are new but only "4" is an object.
        items.process_raw(raw_map(
            json!({"1": 5, "3": "not an object", "4": {"name": "D"}}),
        )?);
        assert_eq!(items.len(), 2);
        let kept = items.get("1").ok_or("item 1 was dropped")?;
        assert_eq!(kept.raw, raw_map(json!({"name": "A"}))?);
        assert!(items.get("2").is_none());
        assert!(items.get("3").is_none());
        let added = items.get("4").ok_or("item 4 was not inserted")?;
        assert_eq!(added.id, "4");
        assert_eq!(added.raw, raw_map(json!({"name": "D"}))?);
        Ok(())
    }

    #[test]
    fn process_raw_with_an_empty_map_clears_the_collection() -> TestResult {
        let mut items = ApiItems::<DummyItem>::new("lights");
        assert!(items.is_empty());
        items.process_raw(raw_map(json!({"1": {"name": "A"}}))?);
        assert!(!items.is_empty());
        items.process_raw(serde_json::Map::new());
        assert!(items.is_empty());
        assert_eq!(items.iter().count(), 0);
        Ok(())
    }
}
