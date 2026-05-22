// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4 homeassistant/components/hue/bridge.py
//! `HueBridge` — the high-level wrapper that ties either a v1 or a v2 bridge
//! into the cave-home runtime. Mirrors `homeassistant.components.hue.bridge.HueBridge`.
//!
//! HA's wrapper does roughly four things:
//!
//! 1. Picks v1 vs v2 based on the entry config.
//! 2. Calls `api.initialize()` with a timeout.
//! 3. Catches LinkButtonNotPressed / Unauthorized and re-runs the
//!    config-flow (we surface those as [`HueError::LinkButtonNotPressed`] /
//!    [`HueError::Unauthorized`] so the Portal admin module + cavectl can
//!    drive a re-pair UI).
//! 4. Forwards platform setup to the per-resource controllers.
//!
//! We model the same four steps without HA's `hass`/`ConfigEntry` types.

use crate::errors::{HueError, HueResult};
use crate::v1::api::V1Request;
use crate::v1::bridge::HueBridgeV1;
use crate::v2::bridge::HueBridgeV2;
use crate::v2::controllers::base::V2Request;
use std::sync::Arc;
use std::time::Duration;

/// Source: `homeassistant.components.hue.bridge.HUB_BUSY_SLEEP` (500 ms).
pub const HUB_BUSY_SLEEP: Duration = Duration::from_millis(500);

/// Bridge config — what the cave-home config-entry equivalent carries.
/// Mirrors the keys used by `homeassistant.components.hue.config_flow`:
/// `CONF_HOST`, `CONF_API_KEY`, `CONF_API_VERSION`.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Hostname or IP of the bridge.
    pub host: String,
    /// Application-key (a.k.a. `username` in v1, `hue-application-key` in v2).
    pub app_key: String,
    /// 1 = legacy v1 bridge, 2 = v2 CLIP bridge.
    pub api_version: u8,
}

/// Either-of wrapper. Source: HA's `HueBridge.api` is `HueBridgeV1 | HueBridgeV2`.
pub enum HueApi {
    V1(HueBridgeV1),
    V2(HueBridgeV2),
}

/// High-level Hue bridge. Source: `homeassistant.components.hue.bridge.HueBridge`.
pub struct HueBridge {
    pub config: BridgeConfig,
    pub authorized: bool,
    pub api: HueApi,
    /// Initialization timeout, mirrors HA's `asyncio.timeout(10)`.
    pub init_timeout: Duration,
}

impl HueBridge {
    /// Build a bridge from a config + transport. The caller (the cave-home
    /// binary or test) chooses the transport implementation.
    #[must_use]
    pub fn new(
        config: BridgeConfig,
        v1_request: Arc<dyn V1Request>,
        v2_request: Arc<dyn V2Request>,
    ) -> Self {
        let api = match config.api_version {
            2 => HueApi::V2(HueBridgeV2::new(
                config.host.clone(),
                config.app_key.clone(),
                v2_request,
            )),
            _ => HueApi::V1(HueBridgeV1::new(
                config.host.clone(),
                config.app_key.clone(),
                v1_request,
            )),
        };
        Self {
            config,
            authorized: false,
            api,
            init_timeout: Duration::from_secs(10),
        }
    }

    /// Source: `HueBridge.async_initialize_bridge`. Returns `Ok(true)` on
    /// successful setup, `Ok(false)` on LinkButtonNotPressed / Unauthorized
    /// (caller should re-pair), `Err` on transport failure.
    pub async fn initialize(&mut self) -> HueResult<bool> {
        let result = tokio::time::timeout(self.init_timeout, async {
            match &mut self.api {
                HueApi::V1(b) => b.initialize().await,
                HueApi::V2(b) => b.initialize().await,
            }
        })
        .await;

        match result {
            Ok(Ok(())) => {
                self.authorized = true;
                Ok(true)
            }
            Ok(Err(HueError::LinkButtonNotPressed(_) | HueError::Unauthorized(_))) => Ok(false),
            Ok(Err(other)) => Err(other),
            Err(_) => Err(HueError::Transport("initialize timed out".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::HueResult;
    use crate::v2::controllers::base::V2Envelope;
    use async_trait::async_trait;
    use serde_json::Value;

    struct AlwaysUnauthorized;
    #[async_trait]
    impl V2Request for AlwaysUnauthorized {
        async fn get(&self, _: &str) -> HueResult<V2Envelope> {
            Err(HueError::Unauthorized("not paired".into()))
        }
        async fn put(&self, _: &str, _: Value) -> HueResult<V2Envelope> {
            Err(HueError::Unauthorized("not paired".into()))
        }
        async fn post(&self, _: &str, _: Value) -> HueResult<V2Envelope> {
            Err(HueError::Unauthorized("not paired".into()))
        }
        async fn delete(&self, _: &str) -> HueResult<V2Envelope> {
            Err(HueError::Unauthorized("not paired".into()))
        }
    }

    struct NeverCalled;
    #[async_trait]
    impl V1Request for NeverCalled {
        async fn get(&self, _: &str) -> HueResult<Value> {
            unreachable!()
        }
        async fn put(&self, _: &str, _: Value) -> HueResult<Value> {
            unreachable!()
        }
        async fn post(&self, _: &str, _: Value) -> HueResult<Value> {
            unreachable!()
        }
        async fn delete(&self, _: &str) -> HueResult<Value> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn unauthorized_returns_ok_false() {
        let mut bridge = HueBridge::new(
            BridgeConfig {
                host: "10.0.0.1".into(),
                app_key: "k".into(),
                api_version: 2,
            },
            Arc::new(NeverCalled),
            Arc::new(AlwaysUnauthorized),
        );
        let outcome = bridge.initialize().await.unwrap();
        assert!(!outcome);
        assert!(!bridge.authorized);
    }
}
