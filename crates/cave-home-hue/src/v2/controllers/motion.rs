// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/sensors.py (MotionController)
//! v2 motion-sensor controller. Mirrors `aiohue.v2.controllers.sensors`'s
//! `MotionController`: tracks the typed motion map, the `enabled` PUT, and
//! ingests live state via the EventStream router (`apply_event`).

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
