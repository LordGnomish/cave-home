// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/sensors.py (ButtonController)
//! v2 button controller. Mirrors `aiohue.v2.controllers.sensors`'s
//! `ButtonController`. Buttons are read/event-only (no PUT surface): the
//! controller tracks the typed map and folds in live presses delivered over
//! the EventStream via `apply_event`.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::HueResult;
    use crate::v2::controllers::base::{V2Envelope, V2Request};
    use crate::v2::models::feature::ButtonReportEvent;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    struct StubReq {
        gets: Mutex<Vec<V2Envelope>>,
    }
    #[async_trait]
    impl V2Request for StubReq {
        async fn get(&self, _p: &str) -> HueResult<V2Envelope> {
            Ok(self.gets.lock().unwrap().pop().unwrap_or_default())
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

    fn button_json(id: &str, event: &str) -> Value {
        json!({
            "id": id,
            "owner": {"rid": "dev-1", "rtype": "device"},
            "metadata": {"control_id": 2},
            "button": {"button_report": {"updated": "2026-06-07T20:00:00Z", "event": event}},
            "type": "button"
        })
    }

    #[tokio::test]
    async fn update_loads_buttons() {
        let env = V2Envelope {
            errors: vec![],
            data: vec![button_json("btn-1", "initial_press")],
        };
        let req = StubReq {
            gets: Mutex::new(vec![env]),
        };
        let mut ctrl = ButtonController::new();
        ctrl.update(&req).await.unwrap();
        assert_eq!(ctrl.len(), 1);
        assert_eq!(ctrl.get("btn-1").unwrap().metadata.control_id, 2);
    }

    #[tokio::test]
    async fn apply_event_updates_last_button_event() {
        // Simulates an SSE button-press event.
        let mut ctrl = ButtonController::new();
        ctrl.apply_event(button_json("btn-1", "short_release"))
            .unwrap();
        assert_eq!(
            ctrl.last_event("btn-1"),
            Some(ButtonReportEvent::ShortRelease)
        );
        assert_eq!(ctrl.last_event("missing"), None);
    }
}
