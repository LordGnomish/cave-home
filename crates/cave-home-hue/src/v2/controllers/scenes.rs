// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/controllers/scenes.py
//! v2 scenes controller. Mirrors `aiohue.v2.controllers.scenes`.

use crate::errors::HueResult;
use crate::v2::controllers::base::{ResourcesController, V2Request};
use crate::v2::models::scene::{Scene, ScenePut, SceneRecall, SceneRecallAction};
use serde_json::json;

/// `aiohue.v2.controllers.scenes.ScenesController`.
pub struct ScenesController {
    inner: ResourcesController<Scene>,
}

impl Default for ScenesController {
    fn default() -> Self {
        Self::new()
    }
}

impl ScenesController {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: ResourcesController::new("scene"),
        }
    }

    pub async fn update(&mut self, req: &dyn V2Request) -> HueResult<()> {
        self.inner.update(req).await
    }

    pub fn iter(&self) -> impl Iterator<Item = &Scene> {
        self.inner.iter()
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Scene> {
        self.inner.get(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// `aiohue.v2.controllers.scenes.ScenesController.recall` — convenience
    /// that PUTs `{"recall": {"action": "active"}}` to activate a scene.
    pub async fn recall(
        &self,
        req: &dyn V2Request,
        id: &str,
        action: SceneRecallAction,
        duration_ms: Option<u32>,
    ) -> HueResult<()> {
        let put = ScenePut {
            recall: Some(SceneRecall {
                action: Some(action),
                duration: duration_ms,
                ..Default::default()
            }),
            ..Default::default()
        };
        let body = serde_json::to_value(&put).unwrap_or(json!({}));
        let _ = req.put(&format!("resource/scene/{id}"), body).await?;
        Ok(())
    }

    pub fn apply_event(&mut self, raw: serde_json::Value) -> HueResult<()> {
        self.inner.apply_event(raw)
    }

    pub fn remove(&mut self, id: &str) {
        self.inner.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::controllers::base::V2Envelope;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubReq {
        puts: Mutex<Vec<(String, Value)>>,
    }
    #[async_trait]
    impl V2Request for StubReq {
        async fn get(&self, _: &str) -> HueResult<V2Envelope> {
            Ok(V2Envelope::default())
        }
        async fn put(&self, p: &str, b: Value) -> HueResult<V2Envelope> {
            self.puts.lock().unwrap().push((p.into(), b));
            Ok(V2Envelope::default())
        }
        async fn post(&self, _: &str, _: Value) -> HueResult<V2Envelope> {
            Ok(V2Envelope::default())
        }
        async fn delete(&self, _: &str) -> HueResult<V2Envelope> {
            Ok(V2Envelope::default())
        }
    }

    #[tokio::test]
    async fn recall_puts_to_scene_path_with_recall_block() {
        let req = StubReq::default();
        let ctrl = ScenesController::new();
        ctrl.recall(&req, "scn", SceneRecallAction::Active, Some(800))
            .await
            .unwrap();
        let puts = req.puts.lock().unwrap().clone();
        assert_eq!(puts.len(), 1);
        assert_eq!(puts[0].0, "resource/scene/scn");
        let recall = puts[0].1.get("recall").unwrap();
        assert_eq!(recall.get("action").unwrap(), &Value::from("active"));
        assert_eq!(recall.get("duration").unwrap(), &Value::from(800));
    }
}
