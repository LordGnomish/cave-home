// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The async HTTP transport seam.
//!
//! Everything above this module (auth, the three API surfaces) speaks
//! [`HttpRequest`] → [`HttpResponse`] over the [`HttpTransport`] trait and never
//! touches a socket directly. Two implementations live here:
//!
//! - [`ReqwestTransport`] — the real one: a `reqwest` client over `rustls`,
//!   built **self-signed-cert tolerant** because every UniFi OS console and
//!   Cloud Key ships a self-signed certificate on its local management
//!   interface. It exposes raw response headers (we need `set-cookie`).
//! - [`MockTransport`] — a fully offline, deterministic transport that replays
//!   queued responses and records every request, so the auth flow and all
//!   three API surfaces are unit-testable with no network.
//!
//! Keeping the seam this thin is what lets the same auth/session/CSRF logic run
//! identically under a real console, under `wiremock` in the e2e test, and under
//! `MockTransport` in unit tests.

use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::VecDeque;

use crate::error::{Result, UnifiError};

/// The HTTP verbs the UniFi local APIs use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// A read.
    Get,
    /// A create / command.
    Post,
    /// A full update.
    Put,
    /// A removal.
    Delete,
}

impl HttpMethod {
    /// The uppercase wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

/// A transport-ready request: an absolute URL, a method, headers and an
/// optional body. The auth layer fills in cookies / CSRF / bearer headers; the
/// API layer fills in URL + body. The transport just executes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The HTTP method.
    pub method: HttpMethod,
    /// The absolute URL (scheme://host[:port]/path?query).
    pub url: String,
    /// Request headers as ordered (name, value) pairs. Names are matched
    /// case-insensitively by the transport.
    pub headers: Vec<(String, String)>,
    /// The raw request body, if any (already serialized — usually JSON).
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    /// A bodyless request.
    #[must_use]
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Builder: add a header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Builder: attach a raw body.
    #[must_use]
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// Builder: attach a JSON body and the matching `Content-Type` header.
    ///
    /// # Errors
    /// Fails if `value` cannot be serialized.
    pub fn json<T: serde::Serialize>(mut self, value: &T) -> Result<Self> {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| UnifiError::Decode(format!("serialize body: {e}")))?;
        self.headers
            .push(("Content-Type".into(), "application/json".into()));
        self.body = Some(bytes);
        Ok(self)
    }

    /// Look up a request header value case-insensitively.
    #[must_use]
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A transport response: status, headers (we keep them all — `set-cookie`
/// matters), and the raw body bytes.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// Response headers as (name, value) pairs, lower-cased names. A header
    /// that appears more than once (e.g. `set-cookie`) appears once per value.
    pub headers: Vec<(String, String)>,
    /// The raw response body.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// A response with a status and JSON body and a `content-type` header.
    #[must_use]
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.into(),
        }
    }

    /// Builder: add a response header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .push((name.into().to_ascii_lowercase(), value.into()));
        self
    }

    /// Whether the status is in the 2xx success range.
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// All values of a (case-insensitive) header. `set-cookie` is the reason
    /// this returns a list rather than a single value.
    #[must_use]
    pub fn header_all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The first value of a (case-insensitive) header.
    #[must_use]
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.header_all(name).into_iter().next()
    }

    /// The body decoded as UTF-8 (lossy), capped to `max` bytes for diagnostics.
    #[must_use]
    pub fn body_text_capped(&self, max: usize) -> String {
        let end = self.body.len().min(max);
        String::from_utf8_lossy(&self.body[..end]).into_owned()
    }

    /// Deserialize the JSON body into `T`.
    ///
    /// # Errors
    /// Fails with [`UnifiError::Decode`] if the body is not valid JSON for `T`.
    pub fn json_body<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|e| {
            UnifiError::Decode(format!(
                "{e}; body was: {}",
                self.body_text_capped(256)
            ))
        })
    }
}

/// The async transport seam every higher layer depends on.
#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// Execute a request and return the response, or a transport-level error if
    /// no response could be obtained. A non-2xx status is **not** an error here
    /// — it is returned as an [`HttpResponse`] for the caller to classify.
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse>;
}

/// A deterministic, fully offline transport for unit tests.
///
/// Queue responses with [`MockTransport::push`]; each [`HttpTransport::execute`]
/// pops the next one and records the request. When the queue is empty it
/// returns the configured [`MockTransport::fallback`] (default: 200 `{}`), so a
/// test only has to queue the responses it cares about ordering for.
#[derive(Default)]
pub struct MockTransport {
    queue: Mutex<VecDeque<HttpResponse>>,
    recorded: Mutex<Vec<HttpRequest>>,
    fallback: Mutex<Option<HttpResponse>>,
}

impl MockTransport {
    /// An empty mock transport.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response to be returned by the next `execute`.
    pub fn push(&self, response: HttpResponse) {
        self.queue.lock().push_back(response);
    }

    /// Queue a response and return `self` for fluent setup.
    #[must_use]
    pub fn with(self, response: HttpResponse) -> Self {
        self.push(response);
        self
    }

    /// Set the response returned once the queue is drained (default 200 `{}`).
    pub fn set_fallback(&self, response: HttpResponse) {
        *self.fallback.lock() = Some(response);
    }

    /// Every request that has been executed, in order — for assertions.
    #[must_use]
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.recorded.lock().clone()
    }

    /// The most recently executed request, if any.
    #[must_use]
    pub fn last_request(&self) -> Option<HttpRequest> {
        self.recorded.lock().last().cloned()
    }

    /// How many requests have been executed.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.recorded.lock().len()
    }
}

#[async_trait]
impl HttpTransport for MockTransport {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.recorded.lock().push(request);
        let queued = self.queue.lock().pop_front();
        if let Some(resp) = queued {
            return Ok(resp);
        }
        let fallback = self.fallback.lock().clone();
        Ok(fallback.unwrap_or_else(|| HttpResponse::json(200, b"{}".to_vec())))
    }
}

/// The real transport: a `reqwest` client over `rustls`.
///
/// Built **once** and shared (it pools connections). Crucially it is configured
/// to accept the self-signed certificate that every UniFi OS console / Cloud
/// Key presents on its local management port — there is no public CA in the
/// loop on a LAN appliance, and Charter §9 keeps us off the Ubiquiti cloud that
/// would otherwise broker trust.
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl std::fmt::Debug for ReqwestTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReqwestTransport").finish_non_exhaustive()
    }
}

impl ReqwestTransport {
    /// Build a transport that trusts the console's self-signed certificate and
    /// applies a per-request timeout.
    ///
    /// # Errors
    /// Fails if the underlying TLS backend cannot be initialised.
    pub fn new(timeout: std::time::Duration) -> Result<Self> {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(timeout)
            .user_agent("cave-home-unifi/0.0")
            .build()
            .map_err(|e| UnifiError::Transport(format!("build client: {e}")))?;
        Ok(Self { client })
    }

    /// Build from an already-configured `reqwest` client (e.g. one that pins a
    /// CA in a hardened deployment).
    #[must_use]
    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    fn method(m: HttpMethod) -> reqwest::Method {
        match m {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
        }
    }
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let mut builder = self
            .client
            .request(Self::method(request.method), &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let resp = builder
            .send()
            .await
            .map_err(|e| UnifiError::Transport(e.to_string()))?;

        let status = resp.status().as_u16();
        let mut headers = Vec::with_capacity(resp.headers().len());
        for (name, value) in resp.headers() {
            if let Ok(v) = value.to_str() {
                headers.push((name.as_str().to_ascii_lowercase(), v.to_string()));
            }
        }
        let body = resp
            .bytes()
            .await
            .map_err(|e| UnifiError::Transport(format!("read body: {e}")))?
            .to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_wire_names() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
    }

    #[test]
    fn request_header_lookup_is_case_insensitive() {
        let r = HttpRequest::new(HttpMethod::Get, "https://x/")
            .header("X-CSRF-Token", "abc");
        assert_eq!(r.header_value("x-csrf-token"), Some("abc"));
        assert_eq!(r.header_value("missing"), None);
    }

    #[test]
    fn json_request_sets_content_type_and_body() {
        let r = HttpRequest::new(HttpMethod::Post, "https://x/")
            .json(&serde_json::json!({"a": 1}))
            .unwrap();
        assert_eq!(r.header_value("content-type"), Some("application/json"));
        assert_eq!(r.body.as_deref(), Some(&b"{\"a\":1}"[..]));
    }

    #[test]
    fn response_set_cookie_returns_all_values() {
        let resp = HttpResponse::json(200, b"{}".to_vec())
            .with_header("Set-Cookie", "TOKEN=abc; Path=/")
            .with_header("set-cookie", "csrf_token=def; Path=/");
        let cookies = resp.header_all("set-cookie");
        assert_eq!(cookies.len(), 2);
        assert!(cookies.iter().any(|c| c.starts_with("TOKEN=")));
        assert!(cookies.iter().any(|c| c.starts_with("csrf_token=")));
    }

    #[test]
    fn response_json_body_decodes() {
        #[derive(serde::Deserialize)]
        struct D {
            n: u32,
        }
        let resp = HttpResponse::json(200, br#"{"n":7}"#.to_vec());
        let d: D = resp.json_body().unwrap();
        assert_eq!(d.n, 7);
    }

    #[test]
    fn response_json_body_bad_json_is_decode_error_with_snippet() {
        let resp = HttpResponse::json(200, b"not json".to_vec());
        let err = resp.json_body::<serde_json::Value>().unwrap_err();
        assert!(matches!(err, UnifiError::Decode(_)));
        assert!(err.to_string().contains("not json"));
    }

    #[tokio::test]
    async fn mock_replays_queued_then_falls_back() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(201, b"{\"first\":true}".to_vec()));
        let r1 = t
            .execute(HttpRequest::new(HttpMethod::Get, "https://x/1"))
            .await
            .unwrap();
        assert_eq!(r1.status, 201);
        // queue drained -> default fallback 200 {}
        let r2 = t
            .execute(HttpRequest::new(HttpMethod::Get, "https://x/2"))
            .await
            .unwrap();
        assert_eq!(r2.status, 200);
        assert_eq!(r2.body, b"{}");
    }

    #[tokio::test]
    async fn mock_records_requests_in_order() {
        let t = MockTransport::new();
        t.execute(HttpRequest::new(HttpMethod::Get, "https://x/a"))
            .await
            .unwrap();
        t.execute(HttpRequest::new(HttpMethod::Post, "https://x/b"))
            .await
            .unwrap();
        let reqs = t.requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].url, "https://x/a");
        assert_eq!(reqs[1].method, HttpMethod::Post);
        assert_eq!(t.last_request().unwrap().url, "https://x/b");
    }

    #[tokio::test]
    async fn mock_custom_fallback() {
        let t = MockTransport::new();
        t.set_fallback(HttpResponse::json(404, b"gone".to_vec()));
        let r = t
            .execute(HttpRequest::new(HttpMethod::Get, "https://x/"))
            .await
            .unwrap();
        assert_eq!(r.status, 404);
    }
}
