// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v1/__init__.py
//! v1 bridge — top-level wrapper that ties the v1 controllers to a host + key.
//!
//! Mirrors `aiohue.v1.HueBridgeV1` — owns the request callable and exposes
//! one [`Config`], [`Lights`], [`Groups`], [`Scenes`] and [`Sensors`] map.

use crate::errors::HueResult;
use crate::v1::api::V1Request;
use crate::v1::config::Config;
use crate::v1::groups::{Groups, new_groups};
use crate::v1::lights::{Lights, new_lights};
use crate::v1::scenes::{Scenes, new_scenes};
use crate::v1::sensors::{Sensors, new_sensors};
use std::sync::Arc;

/// v1 HueBridge — host + app-key + the controllers fetched from the bridge.
pub struct HueBridgeV1 {
    pub host: String,
    pub app_key: String,
    /// Caller-supplied transport (`/api/{app_key}/...`).
    pub request: Arc<dyn V1Request>,
    pub config: Config,
    pub lights: Lights,
    pub groups: Groups,
    pub scenes: Scenes,
    pub sensors: Sensors,
}

impl HueBridgeV1 {
    /// Construct the bridge wrapper without doing IO.
    #[must_use]
    pub fn new(host: String, app_key: String, request: Arc<dyn V1Request>) -> Self {
        Self {
            host,
            app_key,
            request,
            config: Config::default(),
            lights: new_lights(),
            groups: new_groups(),
            scenes: new_scenes(),
            sensors: new_sensors(),
        }
    }

    /// Fetch `/` once and dispatch each section to its controller. Mirrors
    /// `aiohue.v1.HueBridgeV1.initialize`: a single GET pulls the entire
    /// resource tree and we split it across controllers.
    pub async fn initialize(&mut self) -> HueResult<()> {
        let raw = self.request.get("").await?;
        let serde_json::Value::Object(map) = raw else {
            return Ok(());
        };
        if let Some(serde_json::Value::Object(c)) = map.get("config").cloned() {
            self.config = Config::from_raw(c);
        }
        if let Some(serde_json::Value::Object(l)) = map.get("lights").cloned() {
            self.lights.process_raw(l);
        }
        if let Some(serde_json::Value::Object(g)) = map.get("groups").cloned() {
            self.groups.process_raw(g);
        }
        if let Some(serde_json::Value::Object(s)) = map.get("scenes").cloned() {
            self.scenes.process_raw(s);
        }
        if let Some(serde_json::Value::Object(sn)) = map.get("sensors").cloned() {
            self.sensors.process_raw(sn);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::HueResult;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    struct StubReq {
        full: Mutex<Option<Value>>,
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl V1Request for StubReq {
        async fn get(&self, path: &str) -> HueResult<Value> {
            self.calls.lock().unwrap().push(format!("get {path}"));
            Ok(self.full.lock().unwrap().clone().unwrap_or(Value::Null))
        }
        async fn put(&self, path: &str, _b: Value) -> HueResult<Value> {
            self.calls.lock().unwrap().push(format!("put {path}"));
            Ok(Value::Null)
        }
        async fn post(&self, path: &str, _b: Value) -> HueResult<Value> {
            self.calls.lock().unwrap().push(format!("post {path}"));
            Ok(Value::Null)
        }
        async fn delete(&self, path: &str) -> HueResult<Value> {
            self.calls.lock().unwrap().push(format!("delete {path}"));
            Ok(Value::Null)
        }
    }

    #[tokio::test]
    async fn initialize_pulls_full_tree_into_controllers() {
        let req = Arc::new(StubReq {
            full: Mutex::new(Some(json!({
                "config": {"bridgeid": "001788ABCDEF", "name": "Cave"},
                "lights": {"1": {"name": "L1"}, "2": {"name": "L2"}},
                "groups": {"5": {"name": "Salon"}},
                "scenes": {"a": {"name": "Aksam"}},
                "sensors": {"3": {"name": "Dimmer"}},
            }))),
            calls: Mutex::new(Vec::new()),
        });
        let mut br = HueBridgeV1::new("10.0.0.1".into(), "appkey".into(), req.clone());
        br.initialize().await.unwrap();
        assert_eq!(br.config.bridge_id(), Some("001788ABCDEF"));
        assert_eq!(br.lights.len(), 2);
        assert_eq!(br.groups.len(), 1);
        assert_eq!(br.scenes.len(), 1);
        assert_eq!(br.sensors.len(), 1);
        let calls = req.calls.lock().unwrap().clone();
        // initialize() is one full-tree GET, not five separate ones.
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "get ");
    }
}
