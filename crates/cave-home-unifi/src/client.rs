// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The console client: login + authenticated request orchestration.
//!
//! [`ConsoleClient`] is the single object the three API surfaces are built on.
//! It owns the [`Console`] (where things are), the [`HttpTransport`] (how to
//! reach them), the [`Credentials`] and the live [`Session`], and it provides:
//!
//! - [`ConsoleClient::login`] — perform the login POST (or, for an API key,
//!   prime the session), capturing the cookie + CSRF.
//! - [`ConsoleClient::send`] — authorize a request from the session, execute
//!   it, time it into [`Metrics`], and on a `401` **transparently re-login once
//!   and retry** (a UniFi session cookie expires silently; this is the single
//!   place that recovery lives so no API call has to think about it).
//! - [`ConsoleClient::get_json`] / [`ConsoleClient::post_json`] — typed JSON
//!   convenience wrappers.
//!
//! Non-2xx responses are classified through [`UnifiError::from_status`], with a
//! best-effort human message lifted from the body (`meta.msg`, `error`, or
//! `message`). The Network application's `{meta:{rc,msg}, data}` envelope is
//! handled one layer up in [`crate::network`], because Protect and Access do not
//! use it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::auth::{Credentials, Session};
use crate::console::Console;
use crate::error::{Result, UnifiError};
use crate::metrics::Metrics;
use crate::transport::{HttpMethod, HttpRequest, HttpResponse, HttpTransport};

/// The authenticated console client every API surface is built on.
pub struct ConsoleClient<T: HttpTransport> {
    console: Console,
    transport: T,
    credentials: Credentials,
    session: Mutex<Session>,
    metrics: Arc<Metrics>,
}

impl<T: HttpTransport> ConsoleClient<T> {
    /// Build a client. An API-key credential primes the session immediately
    /// (there is no login round-trip); a password credential leaves the session
    /// empty until [`ConsoleClient::login`] runs.
    #[must_use]
    pub fn new(console: Console, transport: T, credentials: Credentials) -> Self {
        let session = match &credentials {
            Credentials::ApiKey(key) => Session::from_api_key(key),
            Credentials::Password { .. } => Session::new(),
        };
        Self {
            console,
            transport,
            credentials,
            session: Mutex::new(session),
            metrics: Arc::new(Metrics::new()),
        }
    }

    /// Build a client sharing an existing metrics registry (so several pillars
    /// report into one exposition).
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        if self.credentials.is_api_key() {
            metrics.set_authenticated(true);
        }
        self.metrics = metrics;
        self
    }

    /// The console this client talks to.
    #[must_use]
    pub fn console(&self) -> &Console {
        &self.console
    }

    /// The shared metrics registry.
    #[must_use]
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    /// Whether the session can currently authenticate a request.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.session.lock().is_authenticated()
    }

    /// Perform the login (or prime the API-key session). Idempotent: calling it
    /// again re-logs-in and refreshes the cookie + CSRF.
    ///
    /// # Errors
    /// [`UnifiError::Login`] on bad credentials / missing session token, or a
    /// transport error.
    pub async fn login(&self) -> Result<()> {
        let Some(req) = self.credentials.login_request(&self.console)? else {
            // API key: nothing to POST; the session is already primed.
            self.metrics.record_login();
            return Ok(());
        };

        let resp = self.transport.execute(req).await?;
        if !resp.is_success() {
            self.metrics.record_login_failure();
            self.metrics.record_error(resp.status);
            return Err(UnifiError::Login(format!(
                "console returned {} — {}",
                resp.status,
                extract_message(&resp).unwrap_or_else(|| "check username/password".into())
            )));
        }

        {
            let mut session = self.session.lock();
            session.ingest_response_headers(
                resp.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            );
            if !session.is_authenticated() {
                self.metrics.record_login_failure();
                return Err(UnifiError::Login(
                    "login succeeded but no session cookie was returned".into(),
                ));
            }
        }
        self.metrics.record_login();
        Ok(())
    }

    /// Authorize and execute a request, recording metrics and recovering from a
    /// single `401` by re-logging-in and retrying.
    ///
    /// `endpoint` is the metrics label (e.g. `network/clients`). On success the
    /// raw [`HttpResponse`] is returned; a non-2xx status becomes a classified
    /// [`UnifiError`].
    ///
    /// # Errors
    /// Transport errors, or a classified HTTP error for a non-2xx response.
    pub async fn send(&self, request: HttpRequest, endpoint: &str) -> Result<HttpResponse> {
        let resp = self.execute_authorized(request.clone(), endpoint).await?;

        // A silently-expired session cookie shows up as 401. Re-login once and
        // retry — but never for an API key (a 401 there is a real key problem).
        if resp.status == 401 && !self.credentials.is_api_key() {
            self.metrics.record_reauth();
            self.login().await?;
            let retry = self.execute_authorized(request, endpoint).await?;
            return Self::classify(retry, &self.metrics);
        }
        Self::classify(resp, &self.metrics)
    }

    async fn execute_authorized(
        &self,
        request: HttpRequest,
        endpoint: &str,
    ) -> Result<HttpResponse> {
        let authorized = self.session.lock().authorize(request);
        let started = Instant::now();
        let resp = self.transport.execute(authorized).await;
        let elapsed = started.elapsed().as_secs_f64();
        self.metrics.record_request(endpoint, elapsed);
        resp
    }

    fn classify(resp: HttpResponse, metrics: &Metrics) -> Result<HttpResponse> {
        if resp.is_success() {
            return Ok(resp);
        }
        metrics.record_error(resp.status);
        let message =
            extract_message(&resp).unwrap_or_else(|| format!("HTTP {}", resp.status));
        Err(UnifiError::from_status(
            resp.status,
            message,
            resp.body_text_capped(512),
        ))
    }

    /// GET `url` and decode the JSON body into `R`.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors.
    pub async fn get_json<R: serde::de::DeserializeOwned>(
        &self,
        url: String,
        endpoint: &str,
    ) -> Result<R> {
        let resp = self
            .send(HttpRequest::new(HttpMethod::Get, url), endpoint)
            .await?;
        resp.json_body()
    }

    /// POST `body` as JSON to `url` and decode the JSON response into `R`.
    ///
    /// # Errors
    /// Transport / HTTP / serialize / decode errors.
    pub async fn post_json<B: serde::Serialize + Sync, R: serde::de::DeserializeOwned>(
        &self,
        url: String,
        body: &B,
        endpoint: &str,
    ) -> Result<R> {
        let req = HttpRequest::new(HttpMethod::Post, url).json(body)?;
        let resp = self.send(req, endpoint).await?;
        resp.json_body()
    }

    /// Log out, clearing the local session (best-effort POST; the local session
    /// is cleared regardless of the result).
    pub async fn logout(&self) {
        if !self.credentials.is_api_key() {
            let req = HttpRequest::new(HttpMethod::Post, self.console.logout_url());
            let _ = self.execute_authorized(req, "auth/logout").await;
        }
        *self.session.lock() = match &self.credentials {
            Credentials::ApiKey(key) => Session::from_api_key(key),
            Credentials::Password { .. } => Session::new(),
        };
        self.metrics
            .set_authenticated(self.credentials.is_api_key());
    }
}

/// A handy default per-request timeout for the real transport.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// Best-effort human message from a UniFi error body: `meta.msg`, then `error`,
/// then `message`.
fn extract_message(resp: &HttpResponse) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(&resp.body).ok()?;
    value
        .get("meta")
        .and_then(|m| m.get("msg"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("error").and_then(serde_json::Value::as_str))
        .or_else(|| value.get("message").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn unifi_os() -> Console {
        Console::unifi_os("10.0.0.1")
    }

    #[tokio::test]
    async fn login_captures_session_cookie_and_csrf() {
        let t = MockTransport::new();
        t.push(
            HttpResponse::json(200, br#"{"meta":{"rc":"ok"}}"#.to_vec())
                .with_header("Set-Cookie", "TOKEN=h.payload.s; Path=/; HttpOnly")
                .with_header("x-csrf-token", "csrf-1"),
        );
        let client = ConsoleClient::new(unifi_os(), t, Credentials::password("a", "b"));
        assert!(!client.is_authenticated());
        client.login().await.unwrap();
        assert!(client.is_authenticated());
        assert!(client.metrics().render_prometheus().contains("unifi_logins_total 1"));
    }

    #[tokio::test]
    async fn login_failure_is_login_error_and_counted() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(
            400,
            br#"{"meta":{"rc":"error","msg":"api.err.Invalid"}}"#.to_vec(),
        ));
        let client = ConsoleClient::new(unifi_os(), t, Credentials::password("a", "bad"));
        let err = client.login().await.unwrap_err();
        assert!(matches!(err, UnifiError::Login(_)));
        assert!(err.to_string().contains("api.err.Invalid"));
        assert!(client
            .metrics()
            .render_prometheus()
            .contains("unifi_login_failures_total 1"));
    }

    #[tokio::test]
    async fn login_without_cookie_is_login_error() {
        let t = MockTransport::new();
        // 200 OK but no Set-Cookie: a reverse proxy / MFA wall.
        t.push(HttpResponse::json(200, b"{}".to_vec()));
        let client = ConsoleClient::new(unifi_os(), t, Credentials::password("a", "b"));
        let err = client.login().await.unwrap_err();
        assert!(matches!(err, UnifiError::Login(_)));
    }

    #[tokio::test]
    async fn api_key_is_authenticated_without_login_post() {
        let t = MockTransport::new();
        let client = ConsoleClient::new(unifi_os(), t, Credentials::api_key("KEY"));
        assert!(client.is_authenticated());
        client.login().await.unwrap();
        // No request should have been sent for an API-key "login".
        // (MockTransport recorded nothing.)
    }

    #[tokio::test]
    async fn send_attaches_auth_and_returns_body() {
        let t = MockTransport::new();
        // login response, then the data response.
        t.push(
            HttpResponse::json(200, b"{}".to_vec())
                .with_header("Set-Cookie", "unifises=s; Path=/")
                .with_header("Set-Cookie", "csrf_token=cx; Path=/"),
        );
        t.push(HttpResponse::json(200, br#"{"ok":true}"#.to_vec()));
        let client =
            ConsoleClient::new(Console::legacy("h"), t, Credentials::password("a", "b"));
        client.login().await.unwrap();
        let resp = client
            .send(
                HttpRequest::new(HttpMethod::Get, "https://h:8443/api/s/default/stat/sta"),
                "network/clients",
            )
            .await
            .unwrap();
        assert!(resp.is_success());
    }

    #[tokio::test]
    async fn send_reauths_once_on_401_then_succeeds() {
        let t = MockTransport::new();
        // initial login
        t.push(
            HttpResponse::json(200, b"{}".to_vec())
                .with_header("Set-Cookie", "TOKEN=h.p.s; Path=/")
                .with_header("x-csrf-token", "c1"),
        );
        // first data call -> 401 (cookie expired)
        t.push(HttpResponse::json(401, br#"{"meta":{"msg":"LoginRequired"}}"#.to_vec()));
        // re-login
        t.push(
            HttpResponse::json(200, b"{}".to_vec())
                .with_header("Set-Cookie", "TOKEN=h.p2.s; Path=/")
                .with_header("x-csrf-token", "c2"),
        );
        // retry data call -> 200
        t.push(HttpResponse::json(200, br#"{"data":[]}"#.to_vec()));

        let client = ConsoleClient::new(unifi_os(), t, Credentials::password("a", "b"));
        client.login().await.unwrap();
        let resp = client
            .send(
                HttpRequest::new(HttpMethod::Get, "https://10.0.0.1:443/x"),
                "network/devices",
            )
            .await
            .unwrap();
        assert!(resp.is_success());
        let out = client.metrics().render_prometheus();
        assert!(out.contains("unifi_reauth_total 1"));
        assert!(out.contains("unifi_logins_total 2"));
    }

    #[tokio::test]
    async fn api_key_401_does_not_reauth() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(401, br#"{"error":"bad key"}"#.to_vec()));
        let client = ConsoleClient::new(unifi_os(), t, Credentials::api_key("KEY"));
        let err = client
            .send(
                HttpRequest::new(HttpMethod::Get, "https://10.0.0.1:443/x"),
                "network/clients",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, UnifiError::Unauthorized(_)));
        // exactly one request: no re-login attempt
        assert_eq!(client.transport_request_count(), 1);
    }

    #[tokio::test]
    async fn non_success_status_is_classified_with_message() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(
            500,
            br#"{"meta":{"rc":"error","msg":"server boom"}}"#.to_vec(),
        ));
        let client = ConsoleClient::new(unifi_os(), t, Credentials::api_key("K"));
        let err = client
            .send(
                HttpRequest::new(HttpMethod::Get, "https://10.0.0.1:443/x"),
                "network/devices",
            )
            .await
            .unwrap_err();
        match err {
            UnifiError::Http { status, message, .. } => {
                assert_eq!(status, 500);
                assert_eq!(message, "server boom");
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }

    // A tiny test-only accessor so the api-key test can assert request count.
    impl ConsoleClient<MockTransport> {
        fn transport_request_count(&self) -> usize {
            self.transport.request_count()
        }
    }
}
