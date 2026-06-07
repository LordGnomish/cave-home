// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/__init__.py
//! High-level v2 bridge wrapper. Mirrors `aiohue.v2.HueBridgeV2`.

use crate::errors::HueResult;
use crate::v2::controllers::base::V2Request;
use crate::v2::controllers::lights::LightsController;
use crate::v2::controllers::scenes::ScenesController;
use std::sync::Arc;

/// `aiohue.v2.HueBridgeV2`. Holds the bridge identity + per-resource controllers.
pub struct HueBridgeV2 {
    pub host: String,
    pub app_key: String,
    pub request: Arc<dyn V2Request>,
    pub lights: LightsController,
    pub scenes: ScenesController,
}

impl HueBridgeV2 {
    /// Build a v2 bridge wrapper without doing IO.
    #[must_use]
    pub fn new(host: String, app_key: String, request: Arc<dyn V2Request>) -> Self {
        Self {
            host,
            app_key,
            request,
            lights: LightsController::new(),
            scenes: ScenesController::new(),
        }
    }

    /// Fetch the initial snapshot. Source: `aiohue.v2.HueBridgeV2.initialize`.
    pub async fn initialize(&mut self) -> HueResult<()> {
        self.lights.update(self.request.as_ref()).await?;
        self.scenes.update(self.request.as_ref()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::controllers::base::V2Envelope;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    struct StubReq {
        gets: Mutex<Vec<V2Envelope>>,
    }
    #[async_trait]
    impl V2Request for StubReq {
        async fn get(&self, _: &str) -> HueResult<V2Envelope> {
            Ok(self.gets.lock().unwrap().pop().unwrap_or_default())
        }
        async fn put(&self, _: &str, _: Value) -> HueResult<V2Envelope> {
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
    async fn initialize_pulls_lights_and_scenes() {
        let scenes_env = V2Envelope {
            errors: vec![],
            data: vec![json!({
                "id": "s1",
                "metadata": {"name": "Aksam"},
                "group": {"rid": "r1", "rtype": "room"},
                "actions": []
            })],
        };
        let lights_env = V2Envelope {
            errors: vec![],
            data: vec![json!({
                "id": "l1",
                "owner": {"rid": "d1", "rtype": "device"},
                "on": {"on": true},
                "mode": "normal",
                "type": "light"
            })],
        };
        // pop()-based stub returns scenes then lights, so we push lights last.
        let req = Arc::new(StubReq {
            gets: Mutex::new(vec![scenes_env, lights_env]),
        });
        let mut br = HueBridgeV2::new("10.0.0.1".into(), "appkey".into(), req);
        br.initialize().await.unwrap();
        assert_eq!(br.lights.len(), 1);
        assert_eq!(br.scenes.len(), 1);
    }

    #[tokio::test]
    async fn initialize_pulls_all_resource_controllers() {
        let mk = |data| V2Envelope { errors: vec![], data };
        // pop() yields the last element first; initialize() pulls in order
        // lights, scenes, grouped_light, motion, button — so push in reverse.
        let req = Arc::new(StubReq {
            gets: Mutex::new(vec![
                mk(vec![json!({
                    "id": "btn-1",
                    "owner": {"rid": "d1", "rtype": "device"},
                    "metadata": {"control_id": 1},
                    "button": {},
                    "type": "button"
                })]),
                mk(vec![json!({
                    "id": "motion-1",
                    "owner": {"rid": "d1", "rtype": "device"},
                    "enabled": true,
                    "motion": {"motion_valid": false, "motion_report": null},
                    "type": "motion"
                })]),
                mk(vec![json!({
                    "id": "gl-1",
                    "owner": {"rid": "r1", "rtype": "room"},
                    "on": {"on": true},
                    "type": "grouped_light"
                })]),
                mk(vec![json!({
                    "id": "s1",
                    "metadata": {"name": "Aksam"},
                    "group": {"rid": "r1", "rtype": "room"},
                    "actions": []
                })]),
                mk(vec![json!({
                    "id": "l1",
                    "owner": {"rid": "d1", "rtype": "device"},
                    "on": {"on": true},
                    "mode": "normal",
                    "type": "light"
                })]),
            ]),
        });
        let mut br = HueBridgeV2::new("10.0.0.1".into(), "appkey".into(), req);
        br.initialize().await.unwrap();
        assert_eq!(br.lights.len(), 1);
        assert_eq!(br.scenes.len(), 1);
        assert_eq!(br.grouped_lights.len(), 1);
        assert_eq!(br.motion.len(), 1);
        assert_eq!(br.buttons.len(), 1);
    }
}
