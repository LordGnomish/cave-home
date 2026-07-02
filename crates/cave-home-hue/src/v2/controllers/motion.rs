// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/sensors.py (MotionController)
//! v2 motion-sensor controller. Mirrors `aiohue.v2.controllers.sensors`'s
//! `MotionController`: tracks the typed motion map, the `enabled` PUT, and
//! ingests live state via the EventStream router (`apply_event`).

use crate::errors::HueResult;
use crate::v2::controllers::base::{ResourcesController, V2Request};
use crate::v2::models::motion::Motion;
use serde_json::json;

/// `aiohue.v2.controllers.sensors.MotionController`.
pub struct MotionController {
    inner: ResourcesController<Motion>,
}

impl Default for MotionController {
    fn default() -> Self {
        Self::new()
    }
}

impl MotionController {
    /// Wire up against `/clip/v2/resource/motion`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: ResourcesController::new("motion"),
        }
    }

    /// Pull the current motion-sensor snapshot from the bridge.
    pub async fn update(&mut self, req: &dyn V2Request) -> HueResult<()> {
        self.inner.update(req).await
    }

    /// Iterate motion sensors.
    pub fn iter(&self) -> impl Iterator<Item = &Motion> {
        self.inner.iter()
    }

    /// Lookup by UUID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Motion> {
        self.inner.get(id)
    }

    /// Number of motion sensors tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Enable / disable a motion sensor. Source:
    /// `MotionController.set_enabled` — PUTs `{"enabled": <bool>}`.
    pub async fn set_enabled(
        &self,
        req: &dyn V2Request,
        id: &str,
        enabled: bool,
    ) -> HueResult<()> {
        let _ = req
            .put(&format!("resource/motion/{id}"), json!({ "enabled": enabled }))
            .await?;
        Ok(())
    }

    /// Apply one event payload (called by the event router) — this is how
    /// live SSE motion-state changes land in the controller.
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
    use crate::errors::HueResult;
    use crate::v2::controllers::base::{V2Envelope, V2Request};
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
        async fn get(&self, _p: &str) -> HueResult<V2Envelope> {
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

    fn motion_json(id: &str, motion: bool) -> Value {
        json!({
            "id": id,
            "owner": {"rid": "device-1", "rtype": "device"},
            "enabled": true,
            "motion": {
                "motion_valid": true,
                "motion_report": {"changed": "2026-06-07T20:00:00.000Z", "motion": motion}
            },
            "type": "motion"
        })
    }

    #[tokio::test]
    async fn update_loads_motion_sensors() {
        let env = V2Envelope {
            errors: vec![],
            data: vec![motion_json("motion-1", true)],
        };
        let req = StubReq {
            gets: Mutex::new(vec![env]),
            puts: Mutex::new(Vec::new()),
        };
        let mut ctrl = MotionController::new();
        ctrl.update(&req).await.unwrap();
        assert_eq!(ctrl.len(), 1);
        assert_eq!(ctrl.get("motion-1").unwrap().is_motion(), Some(true));
    }

    #[tokio::test]
    async fn set_enabled_puts_enabled_flag_to_motion_path() {
        let req = StubReq::default();
        let ctrl = MotionController::new();
        ctrl.set_enabled(&req, "motion-1", false).await.unwrap();
        let puts = req.puts.lock().unwrap().clone();
        assert_eq!(puts.len(), 1);
        assert_eq!(puts[0].0, "resource/motion/motion-1");
        assert_eq!(puts[0].1, json!({"enabled": false}));
    }

    #[tokio::test]
    async fn apply_event_updates_motion_state() {
        // Simulates an SSE `update` event flipping the sensor to "motion".
        let mut ctrl = MotionController::new();
        ctrl.apply_event(motion_json("motion-1", false)).unwrap();
        assert_eq!(ctrl.get("motion-1").unwrap().is_motion(), Some(false));
        ctrl.apply_event(motion_json("motion-1", true)).unwrap();
        assert_eq!(ctrl.get("motion-1").unwrap().is_motion(), Some(true));
    }
}
