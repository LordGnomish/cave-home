// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/__init__.py (request layer)
//! Real CLIP-v2 HTTP transport — a [`V2Request`] implementation backed by
//! `reqwest` + rustls.
//!
//! The Hue bridge exposes the v2 API at `https://<bridge>/clip/v2/...` behind a
//! certificate no public CA vouches for: on current firmware it is issued by
//! the Signify bridge root CA with CN = bridge id; older bridges self-sign
//! (no Hue cloud is involved — everything is LAN-local). Authentication is a
//! single `hue-application-key` header carrying the key the bridge minted at
//! pairing time, so the transport verifies the bridge first —
//! [`signify_bridge_trust`] or a pinned fingerprint — and fails closed. This
//! module wires those facts into a concrete client so the controllers in
//! [`super::controllers`] can talk to a real bridge.
//!
//! Gated behind the default-on `runtime` feature; with it off the crate is a
//! pure, dependency-light core tested against in-process stubs.

use crate::errors::{HueError, HueResult};
use crate::tls::TlsTrust;
use cave_home_tls_pin::rustls_pki::pem::PemObject as _;
use cave_home_tls_pin::rustls_pki::CertificateDer;
use crate::v2::controllers::base::{V2Envelope, V2Request};
use async_trait::async_trait;
use reqwest::{Client, Method};
use serde_json::Value;

/// The Signify (Philips Hue) bridge root CA.
///
/// `C=NL, O=Philips Hue, CN=root-bridge`, ECDSA P-256, valid 2017-01-01 …
/// 2038-01-19, as published in the Hue developer documentation ("Using
/// HTTPS" / bridge certificate). Bridges on current firmware present a leaf issued by this root whose
/// subject CN is the bridge id. Its self-signature verifies, and its SHA-256
/// is `F0:BD:8E:65:09:E8:2F:77:4D:63:BC:00:9D:53:88:C9:69:FE:3D:CF:7D:6D:54:1D:63:51:B7:2B:89:8D:8A:CF`
/// (pinned by a test). Older bridges with a self-signed certificate are not
/// covered by this root — pin those instead ([`TlsTrust::Pinned`]).
pub const SIGNIFY_HUE_ROOT_CA_PEM: &str = "-----BEGIN CERTIFICATE-----
MIICMjCCAdigAwIBAgIUO7FSLbaxikuXAljzVaurLXWmFw4wCgYIKoZIzj0EAwIw
OTELMAkGA1UEBhMCTkwxFDASBgNVBAoMC1BoaWxpcHMgSHVlMRQwEgYDVQQDDAty
b290LWJyaWRnZTAiGA8yMDE3MDEwMTAwMDAwMFoYDzIwMzgwMTE5MDMxNDA3WjA5
MQswCQYDVQQGEwJOTDEUMBIGA1UECgwLUGhpbGlwcyBIdWUxFDASBgNVBAMMC3Jv
b3QtYnJpZGdlMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEjNw2tx2AplOf9x86
aTdvEcL1FU65QDxziKvBpW9XXSIcibAeQiKxegpq8Exbr9v6LBnYbna2VcaK0G22
jOKkTqOBuTCBtjAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIBhjAdBgNV
HQ4EFgQUZ2ONTFrDT6o8ItRnKfqWKnHFGmQwdAYDVR0jBG0wa4AUZ2ONTFrDT6o8
ItRnKfqWKnHFGmShPaQ7MDkxCzAJBgNVBAYTAk5MMRQwEgYDVQQKDAtQaGlsaXBz
IEh1ZTEUMBIGA1UEAwwLcm9vdC1icmlkZ2WCFDuxUi22sYpLlwJY81Wrqy11phcO
MAoGCCqGSM49BAMCA0gAMEUCIEBYYEOsa07TH7E5MJnGw557lVkORgit2Rm1h3B2
sFgDAiEA1Fj/C3AN5psFMjo0//mrQebo0eKd3aWRx+pQY08mk48=
-----END CERTIFICATE-----
";

/// [`SIGNIFY_HUE_ROOT_CA_PEM`] as DER.
///
/// # Errors
/// Only if the embedded PEM were corrupt (a test guards it).
pub fn signify_root_ca_der() -> HueResult<CertificateDer<'static>> {
    CertificateDer::from_pem_slice(SIGNIFY_HUE_ROOT_CA_PEM.as_bytes())
        .map_err(|e| HueError::Generic(format!("embedded Signify root CA: {e}")))
}

/// Trust a bridge the way the Hue app does.
///
/// Its certificate must chain to the Signify root CA and its subject CN must
/// equal `bridge_id` (the 16-hex-digit id from discovery / `config.bridgeid`,
/// compared case-insensitively).
///
/// # Errors
/// An empty bridge id.
pub fn signify_bridge_trust(bridge_id: &str) -> HueResult<TlsTrust> {
    let root = signify_root_ca_der()?;
    TlsTrust::common_name_ca(vec![root], bridge_id)
        .map_err(|e| HueError::Generic(format!("bridge trust: {e}")))
}

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
    /// Connect to a bridge over HTTPS on the LAN, verifying its certificate
    /// with `trust`: [`signify_bridge_trust`] for a bridge whose certificate is
    /// issued by the Signify root (CN = bridge id), or a [`TlsTrust::Pinned`]
    /// fingerprint captured at pairing. No public CA vouches for a bridge, so
    /// public-CA verification would always fail — and accepting anything would
    /// hand the application key to whoever answers on the LAN. A mismatch fails
    /// the handshake before the key is sent.
    ///
    /// # Errors
    /// An unusable trust policy, or a TLS backend that cannot be initialised.
    pub fn new(host: &str, app_key: &str, trust: &TlsTrust) -> HueResult<Self> {
        let mut tls = trust
            .client_config()
            .map_err(|e| HueError::Transport(format!("TLS trust: {e}")))?;
        // The bridge speaks HTTP/2 (preferred for the eventstream) and 1.1.
        tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let client = Client::builder()
            .use_preconfigured_tls(tls)
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
        format!("{}/{CLIP_V2_PREFIX}/{}", self.base_url, path.trim_start_matches('/'))
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
            .map_err(|e| HueError::Transport(format!("request {url}: {}", error_chain(&e))))?;
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

/// `e` plus its `source()` chain — reqwest's `Display` alone hides the TLS
/// reason (e.g. which certificate failed the pin).
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(inner) = source {
        let text = inner.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = inner.source();
    }
    out
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
    use crate::v2::test_support::{http_ok, http_status, spawn_mock};

    #[tokio::test]
    async fn get_lights_round_trips_over_real_http() {
        let body = r#"{"errors":[],"data":[{"id":"l1","owner":{"rid":"d","rtype":"device"},"on":{"on":true},"mode":"normal","type":"light"}]}"#;
        let (base, caps) = spawn_mock(vec![http_ok(body)]).await;
        let t = ReqwestTransport::with_base_url(base, "secret-key").unwrap();

        let env = t.get("resource/light").await.unwrap();
        let data = env.into_data().unwrap();
        assert_eq!(data.len(), 1);

        let log = caps.lock().unwrap();
        let req = &log[0];
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/clip/v2/resource/light");
        assert_eq!(req.header("hue-application-key"), Some("secret-key"));
    }

    #[tokio::test]
    async fn put_light_sends_json_body_and_key() {
        let (base, caps) =
            spawn_mock(vec![http_ok(r#"{"errors":[],"data":[{"rid":"l1","rtype":"light"}]}"#)]).await;
        let t = ReqwestTransport::with_base_url(base, "k").unwrap();

        let put = LightPut {
            on: Some(OnFeature { on: false }),
            ..Default::default()
        };
        t.put("resource/light/l1", serde_json::to_value(&put).unwrap())
            .await
            .unwrap();

        let log = caps.lock().unwrap();
        let req = &log[0];
        assert_eq!(req.method, "PUT");
        assert_eq!(req.path, "/clip/v2/resource/light/l1");
        assert!(req.body.contains("\"on\":false"), "body={}", req.body);
        assert_eq!(req.header("hue-application-key"), Some("k"));
    }

    #[tokio::test]
    async fn forbidden_status_maps_to_unauthorized() {
        let (base, _caps) = spawn_mock(vec![http_status(
            403,
            r#"{"errors":[{"description":"unauthorized user"}]}"#,
        )])
        .await;
        let t = ReqwestTransport::with_base_url(base, "k").unwrap();
        let err = t.get("resource/light").await.unwrap_err();
        assert!(matches!(err, HueError::Unauthorized(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn service_unavailable_maps_to_bridge_busy() {
        let (base, _caps) = spawn_mock(vec![http_status(503, "overloaded")]).await;
        let t = ReqwestTransport::with_base_url(base, "k").unwrap();
        let err = t.get("resource/light").await.unwrap_err();
        assert!(matches!(err, HueError::BridgeBusy(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn envelope_error_in_200_surfaces_via_into_data() {
        let (base, _caps) =
            spawn_mock(vec![http_ok(r#"{"errors":[{"description":"device busy"}],"data":[]}"#)])
                .await;
        let t = ReqwestTransport::with_base_url(base, "k").unwrap();
        let env = t.get("resource/light").await.unwrap();
        assert!(matches!(env.into_data(), Err(HueError::Generic(_))));
    }

    #[test]
    fn new_targets_https_bridge() {
        let trust = signify_bridge_trust("001788fffe1234ab").unwrap();
        let t = ReqwestTransport::new("10.0.0.5", "k", &trust).unwrap();
        assert_eq!(t.base_url(), "https://10.0.0.5");
        assert_eq!(t.eventstream_url(), "https://10.0.0.5/eventstream/clip/v2");
    }

    #[test]
    fn with_base_url_trims_trailing_slash() {
        let t = ReqwestTransport::with_base_url("http://host:8080/", "k").unwrap();
        assert_eq!(t.base_url(), "http://host:8080");
    }
}
