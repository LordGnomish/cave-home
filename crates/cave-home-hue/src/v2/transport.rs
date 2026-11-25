// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/__init__.py (request layer)
//! Real CLIP-v2 HTTP transport — a [`V2Request`] implementation backed by
//! `reqwest` + rustls.
//!
//! The Hue bridge exposes the v2 API at `https://<bridge>/clip/v2/...` behind a
//! **self-signed** certificate generated on the bridge itself (no Hue cloud is
//! involved — everything is LAN-local). Authentication is a single
//! `hue-application-key` header carrying the key the bridge minted at pairing
//! time. This module wires those facts into a concrete client so the
//! controllers in [`super::controllers`] can talk to a real bridge.
//!
//! Gated behind the default-on `runtime` feature; with it off the crate is a
//! pure, dependency-light core tested against in-process stubs.

use crate::errors::{HueError, HueResult};
use crate::v2::controllers::base::{V2Envelope, V2Request};
use async_trait::async_trait;
use reqwest::{Client, Method};
use serde_json::Value;

/// CLIP-v2 path prefix shared by every resource endpoint.
const CLIP_V2_PREFIX: &str = "clip/v2";

/// The application-key auth header the bridge expects on every v2 request.
/// Source: Hue developer-portal "Authentication" — `hue-application-key`.
pub const APP_KEY_HEADER: &str = "hue-application-key";

/// A live [`V2Request`] transport pointed at one Hue bridge.
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: Client,
    base_url: String,
    app_key: String,
}

impl ReqwestTransport {
    /// Connect to a bridge over HTTPS on the LAN, accepting its self-signed
    /// certificate. The bridge mints this cert itself; there is no public CA in
    /// the chain, so standard verification would always fail. Traffic never
    /// leaves the local network.
    pub fn new(host: &str, app_key: &str) -> HueResult<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| HueError::Transport(format!("client build: {e}")))?;
        Ok(Self {
            client,
            base_url: format!("https://{host}"),
            app_key: app_key.to_string(),
        })
    }

    /// Connect to an explicit base URL (scheme included), verifying TLS the
    /// usual way. Used by tests (`http://…`) and by callers who pin a CA or
    /// terminate TLS in front of the bridge.
    pub fn with_base_url(base_url: impl Into<String>, app_key: &str) -> HueResult<Self> {
        let client = Client::builder()
            .build()
            .map_err(|e| HueError::Transport(format!("client build: {e}")))?;
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        Ok(Self {
            client,
            base_url,
            app_key: app_key.to_string(),
        })
    }

    /// The bridge base URL (no trailing slash), e.g. `https://10.0.0.5`.
    // Not `const fn`: `String::as_str` is only const since 1.87, MSRV is 1.85.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The long-lived Server-Sent Events endpoint.
    #[must_use]
    pub fn eventstream_url(&self) -> String {
        format!("{}/eventstream/{CLIP_V2_PREFIX}", self.base_url)
    }

    /// The application key (used by the SSE client, which shares it).
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn app_key(&self) -> &str {
        &self.app_key
    }

    /// The shared `reqwest` client (the SSE client reuses it).
    #[must_use]
    pub const fn client(&self) -> &Client {
        &self.client
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{CLIP_V2_PREFIX}/{}",
            self.base_url,
            path.trim_start_matches('/')
        )
    }

    async fn execute(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> HueResult<V2Envelope> {
        let url = self.url(path);
        let mut rb = self
            .client
            .request(method, &url)
            .header(APP_KEY_HEADER, &self.app_key);
        if let Some(body) = body {
            rb = rb.json(&body);
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| HueError::Transport(format!("request {url}: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| HueError::Transport(format!("read body {url}: {e}")))?;
        if status.is_success() {
            if text.trim().is_empty() {
                return Ok(V2Envelope::default());
            }
            serde_json::from_str(&text).map_err(|e| HueError::Generic(format!("v2 decode: {e}")))
        } else {
            Err(map_status(status.as_u16(), &text))
        }
    }
}

/// Map a non-2xx HTTP status (plus the bridge's body) onto a typed [`HueError`].
fn map_status(status: u16, body: &str) -> HueError {
    let desc = serde_json::from_str::<V2Envelope>(body)
        .ok()
        .and_then(|e| e.errors.into_iter().next())
        .map_or_else(|| body.trim().to_string(), |e| e.description);
    match status {
        401 | 403 => HueError::Unauthorized(desc),
        429 | 503 => HueError::BridgeBusy(desc),
        _ => HueError::Generic(format!("HTTP {status}: {desc}")),
    }
}

#[async_trait]
impl V2Request for ReqwestTransport {
    async fn get(&self, path: &str) -> HueResult<V2Envelope> {
        self.execute(Method::GET, path, None).await
    }
    async fn put(&self, path: &str, body: Value) -> HueResult<V2Envelope> {
        self.execute(Method::PUT, path, Some(body)).await
    }
    async fn post(&self, path: &str, body: Value) -> HueResult<V2Envelope> {
        self.execute(Method::POST, path, Some(body)).await
    }
    async fn delete(&self, path: &str) -> HueResult<V2Envelope> {
        self.execute(Method::DELETE, path, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::models::feature::OnFeature;
    use crate::v2::models::light::LightPut;
    use crate::v2::test_support::{first_request, http_ok, http_status, spawn_mock};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn get_lights_round_trips_over_real_http() -> TestResult {
        let body = r#"{"errors":[],"data":[{"id":"l1","owner":{"rid":"d","rtype":"device"},"on":{"on":true},"mode":"normal","type":"light"}]}"#;
        let (base, caps) = spawn_mock(vec![http_ok(body)]).await?;
        let t = ReqwestTransport::with_base_url(base, "secret-key")?;

        let env = t.get("resource/light").await?;
        let data = env.into_data()?;
        assert_eq!(data.len(), 1);

        let req = first_request(&caps)?;
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/clip/v2/resource/light");
        assert_eq!(req.header("hue-application-key"), Some("secret-key"));
        Ok(())
    }

    #[tokio::test]
    async fn put_light_sends_json_body_and_key() -> TestResult {
        let (base, caps) = spawn_mock(vec![http_ok(
            r#"{"errors":[],"data":[{"rid":"l1","rtype":"light"}]}"#,
        )])
        .await?;
        let t = ReqwestTransport::with_base_url(base, "k")?;

        let put = LightPut {
            on: Some(OnFeature { on: false }),
            ..Default::default()
        };
        t.put("resource/light/l1", serde_json::to_value(&put)?)
            .await?;

        let req = first_request(&caps)?;
        assert_eq!(req.method, "PUT");
        assert_eq!(req.path, "/clip/v2/resource/light/l1");
        assert!(req.body.contains("\"on\":false"), "body={}", req.body);
        assert_eq!(req.header("hue-application-key"), Some("k"));
        Ok(())
    }

    #[tokio::test]
    async fn forbidden_status_maps_to_unauthorized() -> TestResult {
        let (base, _caps) = spawn_mock(vec![http_status(
            403,
            r#"{"errors":[{"description":"unauthorized user"}]}"#,
        )])
        .await?;
        let t = ReqwestTransport::with_base_url(base, "k")?;
        let err = t
            .get("resource/light")
            .await
            .err()
            .ok_or("`get` succeeded against a 403 response")?;
        assert!(matches!(err, HueError::Unauthorized(_)), "got {err:?}");
        Ok(())
    }

    #[tokio::test]
    async fn service_unavailable_maps_to_bridge_busy() -> TestResult {
        let (base, _caps) = spawn_mock(vec![http_status(503, "overloaded")]).await?;
        let t = ReqwestTransport::with_base_url(base, "k")?;
        let err = t
            .get("resource/light")
            .await
            .err()
            .ok_or("`get` succeeded against a 503 response")?;
        assert!(matches!(err, HueError::BridgeBusy(_)), "got {err:?}");
        Ok(())
    }

    #[tokio::test]
    async fn envelope_error_in_200_surfaces_via_into_data() -> TestResult {
        let (base, _caps) = spawn_mock(vec![http_ok(
            r#"{"errors":[{"description":"device busy"}],"data":[]}"#,
        )])
        .await?;
        let t = ReqwestTransport::with_base_url(base, "k")?;
        let env = t.get("resource/light").await?;
        assert!(matches!(env.into_data(), Err(HueError::Generic(_))));
        Ok(())
    }

    #[test]
    fn new_targets_https_self_signed_bridge() -> TestResult {
        let t = ReqwestTransport::new("10.0.0.5", "k")?;
        assert_eq!(t.base_url(), "https://10.0.0.5");
        assert_eq!(t.eventstream_url(), "https://10.0.0.5/eventstream/clip/v2");
        Ok(())
    }

    #[test]
    fn with_base_url_trims_trailing_slash() -> TestResult {
        let t = ReqwestTransport::with_base_url("http://host:8080/", "k")?;
        assert_eq!(t.base_url(), "http://host:8080");
        Ok(())
    }
}
