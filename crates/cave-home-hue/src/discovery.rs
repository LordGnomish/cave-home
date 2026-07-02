// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/discovery.py
//! Hue bridge discovery. Ports `aiohue.discovery` line-by-line.
//!
//! Three discovery paths, all of which the upstream supports:
//!
//! 1. **NUPNP** (`https://discovery.meethue.com/`) — the cloud directory.
//! 2. **Direct probe** of `http://<host>/api/config` — every bridge returns
//!    JSON containing `bridgeid`.
//! 3. **v2 capability check** — bridges that support v2 return HTTP 403 on
//!    `https://<host>/clip/v2/resource` when no key is supplied (because the
//!    endpoint requires `hue-application-key`).
//!
//! The transport is abstracted behind [`HueHttpClient`] so we can both
//! line-by-line-mirror the aiohttp-using upstream *and* keep the crate
//! transport-agnostic (the cave-home binary wires this to its shared
//! reqwest/hyper client).

use crate::errors::{HueError, HueResult};
use crate::util::normalize_bridge_id;
use async_trait::async_trait;
use serde::Deserialize;

/// NUPNP endpoint. Source: `aiohue.discovery.URL_NUPNP`.
pub const URL_NUPNP: &str = "https://discovery.meethue.com/";

/// Outcome of a discovery probe. Source: `aiohue.discovery.DiscoveredHueBridge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredHueBridge {
    /// Resolved host or IP. Mirrors upstream `host`.
    pub host: String,
    /// Normalised bridge ID (12 lowercase hex characters). Mirrors `id`.
    pub id: String,
    /// True iff the bridge speaks the v2 CLIP API. Mirrors `supports_v2`.
    pub supports_v2: bool,
}

/// One entry in the NUPNP JSON array. Source: response shape of
/// `https://discovery.meethue.com/`, see Philips developer-portal docs.
#[derive(Debug, Clone, Deserialize)]
pub struct NupnpEntry {
    #[serde(rename = "internalipaddress")]
    pub internal_ip_address: String,
    #[serde(default)]
    pub id: Option<String>,
}

/// The pieces of the `http://<host>/api/config` response we care about.
/// Source: Philips Hue Configuration API docs §7.2.
#[derive(Debug, Clone, Deserialize)]
pub struct BridgeConfig {
    #[serde(rename = "bridgeid")]
    pub bridge_id: String,
    #[serde(default, rename = "apiversion")]
    pub api_version: Option<String>,
    #[serde(default)]
    pub modelid: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// Transport abstraction. Implementations bridge to the project's chosen
/// HTTP client. The aiohttp upstream uses one `ClientSession`; we use a
/// trait so cave-home can wire reqwest, hyper, or a stub for tests.
#[async_trait]
pub trait HueHttpClient: Send + Sync {
    /// GET a URL and return the raw body bytes.
    async fn get_bytes(&self, url: &str) -> HueResult<Vec<u8>>;
    /// GET a URL and return the HTTP status. Used for the v2 capability
    /// probe (which expects a 403 with no auth header).
    async fn get_status(&self, url: &str) -> HueResult<u16>;
}

/// Probe a single host, returning what we know about the bridge there.
/// Source: `aiohue.discovery.discover_bridge`.
pub async fn discover_bridge(
    client: &dyn HueHttpClient,
    host: &str,
) -> HueResult<DiscoveredHueBridge> {
    let bridge_id = is_hue_bridge(client, host).await?;
    let supports_v2 = is_v2_bridge(client, host).await;
    Ok(DiscoveredHueBridge {
        host: host.to_string(),
        id: bridge_id,
        supports_v2,
    })
}

/// Walk the NUPNP directory. Source: `aiohue.discovery.discover_nupnp`.
///
/// Returns the bridges the directory listed *and* that we could probe.
/// Per upstream policy, NUPNP entries we couldn't probe (e.g. behind a NAT
/// the cave-home node can't reach) are silently dropped from the result.
pub async fn discover_nupnp(client: &dyn HueHttpClient) -> HueResult<Vec<DiscoveredHueBridge>> {
    let body = client.get_bytes(URL_NUPNP).await?;
    let entries: Vec<NupnpEntry> = serde_json::from_slice(&body).map_err(|err| {
        HueError::Transport(format!("nupnp decode: {err}"))
    })?;

    let mut result = Vec::new();
    for entry in entries {
        // Suppress per-entry probe failures, matching upstream `with contextlib.suppress`.
        if let Ok(bridge) = discover_bridge(client, &entry.internal_ip_address).await {
            result.push(bridge);
        }
    }
    Ok(result)
}

/// Check whether the host on the other end of `<host>` is a Hue bridge,
/// returning the normalised bridge ID. Source: `aiohue.discovery.is_hue_bridge`.
///
/// The bridge exposes its config on `/api/config` *without* an application
/// key — this is by design (`developers.meethue.com/develop/hue-api/`).
pub async fn is_hue_bridge(client: &dyn HueHttpClient, host: &str) -> HueResult<String> {
    let url = format!("http://{host}/api/config");
    let bytes = client.get_bytes(&url).await?;
    let parsed: BridgeConfig = serde_json::from_slice(&bytes).map_err(|_| {
        // Upstream raises ClientConnectionError "Invalid API response, not a
        // real Hue bridge?" — we report it as a Transport error so the
        // caller drops the host.
        HueError::Transport("Invalid API response, not a real Hue bridge?".into())
    })?;
    Ok(normalize_bridge_id(&parsed.bridge_id))
}

/// Probe whether the bridge supports the v2 CLIP API.
/// Source: `aiohue.discovery.is_v2_bridge`. v2 returns HTTP 403 when called
/// without an `hue-application-key` header.
pub async fn is_v2_bridge(client: &dyn HueHttpClient, host: &str) -> bool {
    let url = format!("https://{host}/clip/v2/resource");
    match client.get_status(&url).await {
        Ok(403) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Programmable stub. Each test pre-loads URLs -> responses.
    #[derive(Default)]
    struct StubClient {
        body_responses: Mutex<HashMap<String, Vec<u8>>>,
        status_responses: Mutex<HashMap<String, u16>>,
    }

    impl StubClient {
        fn with_body(self, url: &str, body: &[u8]) -> Self {
            self.body_responses
                .lock()
                .unwrap()
                .insert(url.to_string(), body.to_vec());
            self
        }
        fn with_status(self, url: &str, status: u16) -> Self {
            self.status_responses
                .lock()
                .unwrap()
                .insert(url.to_string(), status);
            self
        }
    }

    #[async_trait]
    impl HueHttpClient for StubClient {
        async fn get_bytes(&self, url: &str) -> HueResult<Vec<u8>> {
            self.body_responses
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| HueError::Transport(format!("stub: no body for {url}")))
        }
        async fn get_status(&self, url: &str) -> HueResult<u16> {
            self.status_responses
                .lock()
                .unwrap()
                .get(url)
                .copied()
                .ok_or_else(|| HueError::Transport(format!("stub: no status for {url}")))
        }
    }

    #[tokio::test]
    async fn discover_bridge_returns_normalised_id_and_v2_flag() {
        let stub = StubClient::default()
            .with_body(
                "http://10.0.0.1/api/config",
                br#"{"bridgeid":"001788FFFEABCDEF","apiversion":"1.66.0"}"#,
            )
            .with_status("https://10.0.0.1/clip/v2/resource", 403);
        let bridge = discover_bridge(&stub, "10.0.0.1").await.unwrap();
        assert_eq!(bridge.host, "10.0.0.1");
        assert_eq!(bridge.id, "001788abcdef");
        assert!(bridge.supports_v2);
    }

    #[tokio::test]
    async fn is_v2_bridge_false_when_status_not_403() {
        let stub = StubClient::default()
            .with_status("https://10.0.0.1/clip/v2/resource", 200);
        assert!(!is_v2_bridge(&stub, "10.0.0.1").await);
        let stub = StubClient::default()
            .with_status("https://10.0.0.1/clip/v2/resource", 404);
        assert!(!is_v2_bridge(&stub, "10.0.0.1").await);
        // Even on transport error we say "not v2" — matches upstream.
        let empty = StubClient::default();
        assert!(!is_v2_bridge(&empty, "10.0.0.1").await);
    }

    #[tokio::test]
    async fn nupnp_drops_unreachable_entries_but_keeps_reachable() {
        let stub = StubClient::default()
            .with_body(
                URL_NUPNP,
                br#"[{"id":"abc","internalipaddress":"10.0.0.1"},{"id":"def","internalipaddress":"10.0.0.2"}]"#,
            )
            .with_body(
                "http://10.0.0.1/api/config",
                br#"{"bridgeid":"001788FFFE000001"}"#,
            )
            .with_status("https://10.0.0.1/clip/v2/resource", 403);
        // Note: 10.0.0.2 has nothing wired, so probe fails. Result keeps only .1.
        let bridges = discover_nupnp(&stub).await.unwrap();
        assert_eq!(bridges.len(), 1);
        assert_eq!(bridges[0].id, "001788000001");
        assert!(bridges[0].supports_v2);
    }

    #[tokio::test]
    async fn is_hue_bridge_rejects_non_hue_responses() {
        let stub = StubClient::default()
            .with_body("http://10.0.0.1/api/config", b"<html>not a bridge</html>");
        assert!(matches!(
            is_hue_bridge(&stub, "10.0.0.1").await,
            Err(HueError::Transport(_))
        ));
    }
}
