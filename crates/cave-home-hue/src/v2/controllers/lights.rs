// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/lights.py
//! v2 lights controller. Mirrors `aiohue.v2.controllers.lights`.

use crate::errors::HueResult;
use crate::v2::controllers::base::{ResourcesController, V2Request};
use crate::v2::models::light::{Light, LightPut};
use serde_json::json;

/// `aiohue.v2.controllers.lights.LightsController`.
pub struct LightsController {
    inner: ResourcesController<Light>,
}

impl Default for LightsController {
    fn default() -> Self {
        Self::new()
    }
}

impl LightsController {
    /// Wire up against `/clip/v2/resource/light`. Source:
    /// `LightsController.__init__` (segment = "light").
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: ResourcesController::new("light"),
        }
    }

    /// `LightsController.update`.
    pub async fn update(&mut self, req: &dyn V2Request) -> HueResult<()> {
        self.inner.update(req).await
    }

    /// Iterate lights.
    pub fn iter(&self) -> impl Iterator<Item = &Light> {
        self.inner.iter()
    }

    /// Lookup by UUID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Light> {
        self.inner.get(id)
    }

    /// Number of lights tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// `LightsController.set_state` — port. The upstream method takes
    /// keyword args; we accept a pre-built [`LightPut`] for type safety.
    pub async fn set_state(
        &self,
        req: &dyn V2Request,
        id: &str,
        put: &LightPut,
    ) -> HueResult<()> {
        let body = serde_json::to_value(put).unwrap_or(json!({}));
        let _ = req.put(&format!("resource/light/{id}"), body).await?;
        Ok(())
    }

    /// Apply one event payload (called by event router).
    pub fn apply_event(&mut self, raw: serde_json::Value) -> HueResult<()> {
        self.inner.apply_event(raw)
    }

    /// Forget an id (for `delete` events).
    pub fn remove(&mut self, id: &str) {
        self.inner.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::controllers::base::{V2Envelope, V2Error};
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubReq {
        gets: Mutex<Vec<V2Envelope>>,
        puts: Mutex<Vec<(String, Value)>>,
    }
    #[async_trait]
    impl V2Request for StubReq {
        async fn get(&self, _path: &str) -> HueResult<V2Envelope> {
            Ok(self.gets.lock().unwrap().pop().unwrap_or_default())
        }
        async fn put(&self, p: &str, b: Value) -> HueResult<V2Envelope> {
            self.puts.lock().unwrap().push((p.into(), b));
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
    async fn set_state_puts_to_correct_path_with_body() {
        let req = StubReq::default();
        let ctrl = LightsController::new();
        let put = LightPut {
            on: Some(crate::v2::models::feature::OnFeature { on: true }),
            ..Default::default()
        };
        ctrl.set_state(&req, "abc", &put).await.unwrap();
        let puts = req.puts.lock().unwrap().clone();
        assert_eq!(puts.len(), 1);
        assert_eq!(puts[0].0, "resource/light/abc");
        let body_obj = puts[0].1.as_object().unwrap();
        assert!(body_obj.get("on").is_some());
        assert!(body_obj.get("dimming").is_none());
    }

    #[tokio::test]
    async fn update_loads_lights_from_envelope() {
        let env = V2Envelope {
            errors: vec![],
            data: vec![json!({
                "id": "light-1",
                "owner": {"rid": "dev", "rtype": "device"},
                "on": {"on": false},
                "mode": "normal",
                "type": "light"
            })],
        };
        let req = StubReq {
            gets: Mutex::new(vec![env]),
            puts: Mutex::new(Vec::new()),
        };
        let mut ctrl = LightsController::new();
        ctrl.update(&req).await.unwrap();
        assert_eq!(ctrl.len(), 1);
        assert!(!ctrl.get("light-1").unwrap().is_on());
    }

    #[tokio::test]
    async fn update_surfaces_envelope_errors() {
        let env = V2Envelope {
            errors: vec![V2Error {
                description: "boom".into(),
            }],
            data: vec![],
        };
        let req = StubReq {
            gets: Mutex::new(vec![env]),
            puts: Mutex::new(Vec::new()),
        };
        let mut ctrl = LightsController::new();
        assert!(ctrl.update(&req).await.is_err());
    }
}
