// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/groups.py (GroupedLightController)
//! v2 grouped-light controller — the resource behind "control every light in
//! a room / zone at once". Mirrors `aiohue.v2.controllers.groups`'s
//! `GroupedLightController`: room-level on/off + brightness PUTs.

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

    #[tokio::test]
    async fn set_on_puts_on_feature_to_grouped_light_path() {
        let req = StubReq::default();
        let ctrl = GroupedLightController::new();
        ctrl.set_on(&req, "room-1", true).await.unwrap();
        let puts = req.puts.lock().unwrap().clone();
        assert_eq!(puts.len(), 1);
        assert_eq!(puts[0].0, "resource/grouped_light/room-1");
        let body = puts[0].1.as_object().unwrap();
        assert_eq!(body["on"]["on"], json!(true));
        assert!(body.get("dimming").is_none());
    }

    #[tokio::test]
    async fn set_brightness_puts_dimming() {
        let req = StubReq::default();
        let ctrl = GroupedLightController::new();
        ctrl.set_brightness(&req, "zone-9", 42.0).await.unwrap();
        let puts = req.puts.lock().unwrap().clone();
        assert_eq!(puts[0].0, "resource/grouped_light/zone-9");
        let body = puts[0].1.as_object().unwrap();
        assert_eq!(body["dimming"]["brightness"], json!(42.0));
        assert!(body.get("on").is_none());
    }

    #[tokio::test]
    async fn update_loads_grouped_lights() {
        let env = V2Envelope {
            errors: vec![],
            data: vec![json!({
                "id": "gl-1",
                "owner": {"rid": "room-1", "rtype": "room"},
                "on": {"on": true},
                "type": "grouped_light"
            })],
        };
        let req = StubReq {
            gets: Mutex::new(vec![env]),
            puts: Mutex::new(Vec::new()),
        };
        let mut ctrl = GroupedLightController::new();
        ctrl.update(&req).await.unwrap();
        assert_eq!(ctrl.len(), 1);
        assert!(ctrl.get("gl-1").unwrap().on.on);
    }

    #[tokio::test]
    async fn apply_event_updates_grouped_light_state() {
        let mut ctrl = GroupedLightController::new();
        ctrl.apply_event(json!({
            "id": "gl-1",
            "owner": {"rid": "room-1", "rtype": "room"},
            "on": {"on": false},
            "type": "grouped_light"
        }))
        .unwrap();
        assert!(!ctrl.get("gl-1").unwrap().on.on);
    }
}
