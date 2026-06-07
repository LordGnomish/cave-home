// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The UniFi Access developer REST surface + notification WebSocket URL.
//!
//! Access is its own appliance: a dedicated port (12445 by default), a
//! `Authorization: Bearer <token>` scheme, and the `{code,msg,data}` envelope.
//! It does **not** ride the Network/Protect console session, so [`AccessClient`]
//! is a small self-contained client over the shared [`HttpTransport`] seam and
//! the shared [`Metrics`] registry — not a [`ConsoleClient`](crate::client::ConsoleClient).
//!
//! It exposes the doors a household controls, the visitors it has issued, the
//! access-event log, the **intercom unlock** (answering a doorbell call by
//! releasing the door), and the URL of the real-time notifications WebSocket the
//! [`crate::ws`] engine subscribes to.

use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use cave_home_unifi_access::AccessEvent;

use super::types::{
    AccessEnvelope, DoorStatus, Visitor, WireAccessLog, WireDoor, WireVisitor,
};
use crate::error::{Result, UnifiError};
use crate::metrics::Metrics;
use crate::transport::{HttpMethod, HttpRequest, HttpResponse, HttpTransport};

/// The default UniFi Access developer-API port.
pub const DEFAULT_ACCESS_PORT: u16 = 12445;

/// How to reach a UniFi Access appliance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessConfig {
    host: String,
    port: u16,
    token: String,
    tls: bool,
}

impl AccessConfig {
    /// A config for `host` with the given bearer `token`, on the default port
    /// 12445 over TLS.
    #[must_use]
    pub fn new(host: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: DEFAULT_ACCESS_PORT,
            token: token.into(),
            tls: true,
        }
    }

    /// Builder: override the port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Builder: choose `http`/`ws` instead of `https`/`wss` (e2e / mock only).
    #[must_use]
    pub fn with_tls(mut self, tls: bool) -> Self {
        self.tls = tls;
        self
    }

    /// The HTTP origin.
    #[must_use]
    pub fn base_url(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}:{}", self.host, self.port)
    }

    /// A developer-API URL for `sub` (the part after `/api/v1/developer/`).
    #[must_use]
    pub fn developer_url(&self, sub: &str) -> String {
        format!("{}/api/v1/developer/{sub}", self.base_url())
    }

    /// The notifications WebSocket URL.
    #[must_use]
    pub fn notifications_ws_url(&self) -> String {
        let scheme = if self.tls { "wss" } else { "ws" };
        format!(
            "{scheme}://{}:{}/api/v1/developer/devices/notifications",
            self.host, self.port
        )
    }

    /// The bearer token (e.g. for the WebSocket `Authorization` header).
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// A temporary lock-rule the Access API understands (`PUT doors/{id}/lock_rule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockRule {
    /// Unlock now, then auto-relock after `minutes`.
    Custom {
        /// Minutes to stay unlocked.
        minutes: u32,
    },
    /// Keep the door locked indefinitely.
    KeepLock,
    /// Keep the door unlocked indefinitely.
    KeepUnlock,
    /// Clear any active rule (back to schedule).
    Reset,
}

impl LockRule {
    /// The wire `type` token.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Custom { .. } => "custom",
            Self::KeepLock => "keep_lock",
            Self::KeepUnlock => "keep_unlock",
            Self::Reset => "reset",
        }
    }

    /// The `interval` (minutes) the rule carries, if any.
    #[must_use]
    pub const fn interval(self) -> Option<u32> {
        match self {
            Self::Custom { minutes } => Some(minutes),
            _ => None,
        }
    }
}

/// The UniFi Access developer-API client.
pub struct AccessClient<T: HttpTransport> {
    config: AccessConfig,
    transport: T,
    metrics: Arc<Metrics>,
}

impl<T: HttpTransport> AccessClient<T> {
    /// Build a client. The bearer token is set authenticated immediately.
    #[must_use]
    pub fn new(config: AccessConfig, transport: T) -> Self {
        let metrics = Arc::new(Metrics::new());
        metrics.set_authenticated(true);
        Self {
            config,
            transport,
            metrics,
        }
    }

    /// Build sharing an existing metrics registry.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        metrics.set_authenticated(true);
        self.metrics = metrics;
        self
    }

    /// The Access config (host/port/token, WS URL).
    #[must_use]
    pub fn config(&self) -> &AccessConfig {
        &self.config
    }

    /// The shared metrics registry.
    #[must_use]
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }

    /// The underlying transport (chiefly for tests).
    #[must_use]
    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn authorize(&self, mut req: HttpRequest) -> HttpRequest {
        req = req
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("Accept", "application/json");
        req
    }

    async fn send(&self, req: HttpRequest, endpoint: &str) -> Result<HttpResponse> {
        let req = self.authorize(req);
        let started = Instant::now();
        let resp = self.transport.execute(req).await?;
        self.metrics
            .record_request(endpoint, started.elapsed().as_secs_f64());
        if resp.is_success() {
            return Ok(resp);
        }
        self.metrics.record_error(resp.status);
        Err(UnifiError::from_status(
            resp.status,
            format!("Access HTTP {}", resp.status),
            resp.body_text_capped(512),
        ))
    }

    async fn get_data<R: serde::de::DeserializeOwned>(
        &self,
        url: String,
        endpoint: &str,
    ) -> Result<R> {
        let resp = self
            .send(HttpRequest::new(HttpMethod::Get, url), endpoint)
            .await?;
        let env: AccessEnvelope<R> = resp.json_body()?;
        env.into_data()
    }

    /// List the doors and their live lock state (`GET /doors`).
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or a non-`SUCCESS` envelope.
    pub async fn doors(&self) -> Result<Vec<DoorStatus>> {
        let wires: Vec<WireDoor> = self
            .get_data(self.config.developer_url("doors"), "access/doors")
            .await?;
        Ok(wires.into_iter().map(WireDoor::into_status).collect())
    }

    /// List the issued visitors (`GET /visitors`).
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or a non-`SUCCESS` envelope.
    pub async fn visitors(&self) -> Result<Vec<Visitor>> {
        let wires: Vec<WireVisitor> = self
            .get_data(self.config.developer_url("visitors"), "access/visitors")
            .await?;
        Ok(wires.into_iter().map(Visitor::from).collect())
    }

    /// Fetch the recent access-event log (`POST /system/logs`), lowered to
    /// domain [`AccessEvent`]s.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors, or a non-`SUCCESS` envelope.
    pub async fn events(&self, since: u64, until: u64) -> Result<Vec<AccessEvent>> {
        let body = json!({
            "topic": "door_openings",
            "since": since,
            "until": until,
        });
        let req = HttpRequest::new(HttpMethod::Post, self.config.developer_url("system/logs"))
            .json(&body)?;
        let resp = self.send(req, "access/events").await?;
        let env: AccessEnvelope<Vec<WireAccessLog>> = resp.json_body()?;
        Ok(env
            .into_data()?
            .into_iter()
            .map(WireAccessLog::into_domain)
            .collect())
    }

    /// Remotely unlock a door (`PUT /doors/{id}/unlock`). This is the call that
    /// **answers an intercom call** — releasing the door for the visitor.
    ///
    /// # Errors
    /// Empty door id, transport / HTTP / decode errors, or a non-`SUCCESS`
    /// envelope.
    pub async fn unlock(&self, door_id: &str) -> Result<()> {
        if door_id.is_empty() {
            return Err(UnifiError::InvalidArgument("empty door id".into()));
        }
        let url = self.config.developer_url(&format!("doors/{door_id}/unlock"));
        let resp = self
            .send(HttpRequest::new(HttpMethod::Put, url), "access/unlock")
            .await?;
        let env: AccessEnvelope<serde_json::Value> = resp.json_body()?;
        env.into_data().map(|_| ())
    }

    /// Answer an intercom / doorbell call by unlocking the door. A named alias
    /// for [`AccessClient::unlock`] so the intercom flow reads clearly and gets
    /// its own metric label.
    ///
    /// # Errors
    /// As [`AccessClient::unlock`].
    pub async fn answer_intercom(&self, door_id: &str) -> Result<()> {
        if door_id.is_empty() {
            return Err(UnifiError::InvalidArgument("empty door id".into()));
        }
        let url = self.config.developer_url(&format!("doors/{door_id}/unlock"));
        let resp = self
            .send(HttpRequest::new(HttpMethod::Put, url), "access/intercom_unlock")
            .await?;
        let env: AccessEnvelope<serde_json::Value> = resp.json_body()?;
        env.into_data().map(|_| ())
    }

    /// Set a temporary lock rule on a door (`PUT /doors/{id}/lock_rule`).
    ///
    /// # Errors
    /// Empty door id, transport / HTTP / decode errors, or a non-`SUCCESS`
    /// envelope.
    pub async fn set_lock_rule(&self, door_id: &str, rule: LockRule) -> Result<()> {
        if door_id.is_empty() {
            return Err(UnifiError::InvalidArgument("empty door id".into()));
        }
        let mut body = json!({ "type": rule.as_wire() });
        if let Some(interval) = rule.interval() {
            body["interval"] = json!(interval);
        }
        let url = self.config.developer_url(&format!("doors/{door_id}/lock_rule"));
        let req = HttpRequest::new(HttpMethod::Put, url).json(&body)?;
        let resp = self.send(req, "access/lock_rule").await?;
        let env: AccessEnvelope<serde_json::Value> = resp.json_body()?;
        env.into_data().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn client_returning(body: &[u8]) -> AccessClient<MockTransport> {
        let t = MockTransport::new();
        t.push(HttpResponse::json(200, body.to_vec()));
        AccessClient::new(
            AccessConfig::new("10.0.0.2", "TOK").with_tls(false),
            t,
        )
    }

    #[test]
    fn config_urls_use_port_12445() {
        let cfg = AccessConfig::new("nas", "T");
        assert_eq!(cfg.base_url(), "https://nas:12445");
        assert_eq!(
            cfg.developer_url("doors"),
            "https://nas:12445/api/v1/developer/doors"
        );
        assert_eq!(
            cfg.notifications_ws_url(),
            "wss://nas:12445/api/v1/developer/devices/notifications"
        );
    }

    #[tokio::test]
    async fn doors_map_and_carry_bearer_header() {
        let client = client_returning(
            br#"{"code":"SUCCESS","msg":"ok","data":[
                {"id":"d1","name":"Front","door_lock_relay_status":"lock","is_bind_hub":true}
            ]}"#,
        );
        let doors = client.doors().await.unwrap();
        assert_eq!(doors.len(), 1);
        assert_eq!(doors[0].name, "Front");
        let req = client.transport().last_request().unwrap();
        assert_eq!(req.header_value("authorization"), Some("Bearer TOK"));
        assert!(req.url.ends_with("/api/v1/developer/doors"));
    }

    #[tokio::test]
    async fn unlock_targets_unlock_endpoint() {
        let client = client_returning(br#"{"code":"SUCCESS","data":{}}"#);
        client.unlock("d1").await.unwrap();
        let req = client.transport().last_request().unwrap();
        assert_eq!(req.method, HttpMethod::Put);
        assert!(req.url.ends_with("/doors/d1/unlock"));
    }

    #[tokio::test]
    async fn answer_intercom_unlocks_and_labels_metric() {
        let client = client_returning(br#"{"code":"SUCCESS","data":{}}"#);
        client.answer_intercom("front").await.unwrap();
        let req = client.transport().last_request().unwrap();
        assert!(req.url.ends_with("/doors/front/unlock"));
        assert!(client
            .metrics()
            .render_prometheus()
            .contains("unifi_requests_total{endpoint=\"access/intercom_unlock\"} 1"));
    }

    #[tokio::test]
    async fn unlock_rejects_empty_door() {
        let client = client_returning(b"{}");
        let err = client.unlock("").await.unwrap_err();
        assert!(matches!(err, UnifiError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn lock_rule_custom_sends_interval() {
        let client = client_returning(br#"{"code":"SUCCESS","data":{}}"#);
        client
            .set_lock_rule("d1", LockRule::Custom { minutes: 10 })
            .await
            .unwrap();
        let req = client.transport().last_request().unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["type"], "custom");
        assert_eq!(body["interval"], 10);
    }

    #[tokio::test]
    async fn events_map_to_domain() {
        let client = client_returning(
            br#"{"code":"SUCCESS","data":[
                {"actor":"Burak","door_id":"d1","result":"ACCESS","timestamp":1717000000}
            ]}"#,
        );
        let evs = client.events(0, 9_999_999_999).await.unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].who, "Burak");
        assert!(evs[0].outcome.is_granted());
    }

    #[tokio::test]
    async fn non_success_code_is_error() {
        let client = client_returning(
            br#"{"code":"CODE_OPERATION_FORBIDDEN","msg":"forbidden","data":null}"#,
        );
        let err = client.doors().await.unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }

    #[tokio::test]
    async fn http_401_is_unauthorized() {
        let t = MockTransport::new();
        t.push(HttpResponse::json(401, b"{}".to_vec()));
        let client = AccessClient::new(AccessConfig::new("h", "BAD").with_tls(false), t);
        let err = client.doors().await.unwrap_err();
        assert!(matches!(err, UnifiError::Unauthorized(_)));
    }
}
