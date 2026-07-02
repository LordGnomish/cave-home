// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/base.py
//! v2 controller base class — `BaseResourcesController`. Owns a typed map
//! of resources keyed by UUID and exposes `iter` / `get` / `update`.

use crate::errors::{HueError, HueResult};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;

/// HTTP transport for the v2 CLIP API. Implementations bridge to the
/// project's chosen HTTP client (the binary will wire reqwest/hyper).
#[async_trait]
pub trait V2Request: Send + Sync {
    /// GET against `/clip/v2/<path>`. The bridge always returns the
    /// envelope shape `{"errors": [...], "data": [...]}`.
    async fn get(&self, path: &str) -> HueResult<V2Envelope>;
    /// PUT a serialised body to `/clip/v2/<path>`.
    async fn put(&self, path: &str, body: Value) -> HueResult<V2Envelope>;
    /// POST — for resource creation (scenes / smart scenes).
    async fn post(&self, path: &str, body: Value) -> HueResult<V2Envelope>;
    /// DELETE.
    async fn delete(&self, path: &str) -> HueResult<V2Envelope>;
}

/// The wrapping envelope every CLIP endpoint returns.
/// Source: developer-portal `Response object` section.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct V2Envelope {
    #[serde(default)]
    pub errors: Vec<V2Error>,
    #[serde(default)]
    pub data: Vec<Value>,
}

/// One error inside the envelope. Source: `aiohue.v2.controllers.base`
/// indirectly + developer-portal docs.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct V2Error {
    pub description: String,
}

impl V2Envelope {
    /// Raise the first error as a typed [`HueError`], or pass through `data`.
    pub fn into_data(self) -> HueResult<Vec<Value>> {
        if let Some(first) = self.errors.into_iter().next() {
            return Err(HueError::Generic(first.description));
        }
        Ok(self.data)
    }
}

/// Typed map of resources of one kind. Source: `BaseResourcesController.items`.
pub struct ResourcesController<T> {
    items: HashMap<String, T>,
    path: String,
}

impl<T: DeserializeOwned + ResourceIdAccess + Clone> ResourcesController<T> {
    /// Wire up a controller against a `/resource/<segment>` endpoint.
    pub fn new(segment: &str) -> Self {
        Self {
            items: HashMap::new(),
            path: format!("resource/{segment}"),
        }
    }

    /// Pull the list from the bridge and replace the map.
    /// Source: `BaseResourcesController.update`.
    pub async fn update(&mut self, req: &dyn V2Request) -> HueResult<()> {
        let env = req.get(&self.path).await?;
        let data = env.into_data()?;
        let mut new_items = HashMap::with_capacity(data.len());
        for raw in data {
            let item: T = serde_json::from_value(raw)
                .map_err(|err| HueError::Generic(format!("v2 decode: {err}")))?;
            new_items.insert(item.resource_id().to_string(), item);
        }
        self.items = new_items;
        Ok(())
    }

    /// Apply one resource event payload directly (used by the event router).
    pub fn apply_event(&mut self, raw: Value) -> HueResult<()> {
        let item: T = serde_json::from_value(raw)
            .map_err(|err| HueError::Generic(format!("v2 event decode: {err}")))?;
        self.items.insert(item.resource_id().to_string(), item);
        Ok(())
    }

    /// Drop an item, e.g. on a `delete` event.
    pub fn remove(&mut self, id: &str) {
        self.items.remove(id);
    }

    /// Iter over items.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.values()
    }

    /// Get by id.
    pub fn get(&self, id: &str) -> Option<&T> {
        self.items.get(id)
    }

    /// Number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Underlying path (e.g. `resource/light`). Used by sub-controllers.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Helper trait so [`ResourcesController`] can extract a resource ID without
/// hand-writing a key extractor per type.
pub trait ResourceIdAccess {
    fn resource_id(&self) -> &str;
}

macro_rules! impl_resource_id_access {
    ($($t:ty),* $(,)?) => {
        $(
            impl ResourceIdAccess for $t {
                fn resource_id(&self) -> &str { &self.id }
            }
        )*
    };
}

use crate::v2::models::{
    button::Button, device::Device, grouped_light::GroupedLight, light::Light, motion::Motion,
    room::Room, scene::Scene, zone::Zone,
};
impl_resource_id_access!(Light, Scene, Motion, Button, Device, Room, Zone, GroupedLight);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::models::light::Light;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    struct StubReq {
        responses: Mutex<Vec<V2Envelope>>,
    }
    #[async_trait]
    impl V2Request for StubReq {
        async fn get(&self, _path: &str) -> HueResult<V2Envelope> {
            self.responses
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| HueError::Transport("no response".into()))
        }
        async fn put(&self, _p: &str, _b: Value) -> HueResult<V2Envelope> {
            Ok(V2Envelope::default())
        }
        async fn post(&self, _p: &str, _b: Value) -> HueResult<V2Envelope> {
            Ok(V2Envelope::default())
        }
        async fn delete(&self, _p: &str) -> HueResult<V2Envelope> {
            Ok(V2Envelope::default())
        }
    }

    #[tokio::test]
    async fn envelope_errors_become_hue_errors() {
        let env = V2Envelope {
            errors: vec![V2Error {
                description: "bad".into(),
            }],
            data: vec![],
        };
        let result = env.into_data();
        assert!(matches!(result, Err(HueError::Generic(_))));
    }

    #[tokio::test]
    async fn resources_controller_loads_typed_items() {
        let env = V2Envelope {
            errors: vec![],
            data: vec![json!({
                "id": "11111111-1111-1111-1111-111111111111",
                "owner": {"rid": "dev", "rtype": "device"},
                "on": {"on": true},
                "mode": "normal",
                "type": "light"
            })],
        };
        let req = StubReq {
            responses: Mutex::new(vec![env]),
        };
        let mut ctrl = ResourcesController::<Light>::new("light");
        ctrl.update(&req).await.unwrap();
        assert_eq!(ctrl.len(), 1);
        let light = ctrl
            .get("11111111-1111-1111-1111-111111111111")
            .unwrap();
        assert!(light.is_on());
    }

    #[tokio::test]
    async fn remove_drops_entry() {
        let mut ctrl = ResourcesController::<Light>::new("light");
        let raw = json!({
            "id": "abc",
            "owner": {"rid": "dev", "rtype": "device"},
            "on": {"on": true},
            "mode": "normal",
            "type": "light"
        });
        ctrl.apply_event(raw).unwrap();
        assert_eq!(ctrl.len(), 1);
        ctrl.remove("abc");
        assert_eq!(ctrl.len(), 0);
    }
}
