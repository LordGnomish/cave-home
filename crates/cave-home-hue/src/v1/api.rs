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
        // Update / insert each present id.
        for (id, raw_item) in raw.iter() {
            let Some(obj) = raw_item.as_object() else {
                continue;
            };
            match self.items.get_mut(id) {
                Some(existing) => existing.set_raw(obj.clone()),
                None => {
                    self.items
                        .insert(id.clone(), T::from_raw(id.clone(), obj.clone()));
                }
            }
        }
        // Drop ids that are no longer in the bridge response.
        let dead: Vec<String> = self
            .items
            .keys()
            .filter(|id| !raw.contains_key(id.as_str()))
            .cloned()
            .collect();
        for id in dead {
            self.items.remove(&id);
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
    pub fn get(&self, id: &str) -> Option<&T> {
        self.items.get(id)
    }

    /// Mutable get — used by sub-controllers that mutate raw fields.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut T> {
        self.items.get_mut(id)
    }

    /// Number of items currently held.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if the collection is empty.
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
    fn process_raw_inserts_new_items() {
        let mut items = ApiItems::<DummyItem>::new("lights");
        let raw = json!({
            "1": {"name": "Mutfak"},
            "2": {"name": "Salon"},
        });
        items.process_raw(raw.as_object().unwrap().clone());
        assert_eq!(items.len(), 2);
        assert!(items.get("1").is_some());
        assert!(items.get("2").is_some());
    }

    #[test]
    fn process_raw_removes_disappeared_items() {
        let mut items = ApiItems::<DummyItem>::new("lights");
        items.process_raw(
            json!({"1": {"name": "A"}, "2": {"name": "B"}})
                .as_object()
                .unwrap()
                .clone(),
        );
        // Second fetch drops id "2".
        items.process_raw(json!({"1": {"name": "A"}}).as_object().unwrap().clone());
        assert_eq!(items.len(), 1);
        assert!(items.get("2").is_none());
    }

    #[test]
    fn process_raw_mutates_existing_items_in_place() {
        let mut items = ApiItems::<DummyItem>::new("lights");
        items.process_raw(json!({"1": {"name": "A"}}).as_object().unwrap().clone());
        items.process_raw(json!({"1": {"name": "B"}}).as_object().unwrap().clone());
        let item = items.get("1").unwrap();
        assert_eq!(item.raw.get("name").unwrap(), "B");
    }

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// `value` as a raw bridge map.
    fn raw_map(value: Value) -> Result<RawItem, &'static str> {
        match value {
            Value::Object(map) => Ok(map),
            _ => Err("the fixture is not a JSON object"),
        }
    }

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
