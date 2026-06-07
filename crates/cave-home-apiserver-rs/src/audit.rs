// SPDX-License-Identifier: Apache-2.0
//! Audit logging: a structured record of every request that reaches the handler.
//!
//! Behavioural reference: the Kubernetes audit event shape
//! (`audit.k8s.io/v1` `Event`: `verb`, `user.username`, `objectRef`
//! (group/resource/subresource/namespace/name), `requestURI`,
//! `responseStatus.code`, `stage`). This is a clean-room reimplementation of the
//! documented event fields at the `ResponseComplete` stage. Webhook/file
//! backends and the policy-driven level selection are deferred (see
//! `parity.manifest.toml`); any backend plugs into the [`AuditSink`] trait.

use std::sync::Mutex;

use crate::json::{obj, Value};
use crate::path::ResourcePath;

/// One audit record, captured at the `ResponseComplete` stage.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuditEvent {
    /// Processing stage (always `ResponseComplete` here).
    pub stage: String,
    /// The resolved REST verb (`get`, `list`, `create`, …) or the HTTP method
    /// for non-resource requests.
    pub verb: String,
    /// Authenticated user name (empty before authentication runs).
    pub user: String,
    /// API group (`""` for the core group).
    pub api_group: String,
    /// Plural resource (empty for non-resource requests).
    pub resource: String,
    /// Subresource (`status`, …), if any.
    pub subresource: String,
    /// Namespace (empty for cluster-scoped / non-resource).
    pub namespace: String,
    /// Object name (empty for collection verbs).
    pub name: String,
    /// The raw request URI.
    pub request_uri: String,
    /// The HTTP status code of the response.
    pub response_code: u16,
}

impl AuditEvent {
    /// Start an event from the request's method + URI (resource fields and user
    /// are filled in as the handler resolves them).
    #[must_use]
    pub fn started(method: &str, request_uri: &str) -> Self {
        Self {
            stage: "ResponseComplete".to_string(),
            verb: method.to_ascii_lowercase(),
            request_uri: request_uri.to_string(),
            ..Self::default()
        }
    }

    /// Populate the object-reference fields from a parsed resource path.
    pub fn set_resource(&mut self, rp: &ResourcePath) {
        self.api_group = rp.group.clone();
        self.resource = rp.resource.clone();
        self.subresource = rp.subresource.clone();
        self.namespace = rp.namespace.clone();
        self.name = rp.name.clone();
    }

    /// Render as an `audit.k8s.io/v1` `Event` JSON object.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut object_ref = obj([
            ("resource", Value::from(self.resource.clone())),
            ("apiGroup", Value::from(self.api_group.clone())),
        ]);
        if !self.namespace.is_empty() {
            object_ref.insert("namespace", Value::from(self.namespace.clone()));
        }
        if !self.name.is_empty() {
            object_ref.insert("name", Value::from(self.name.clone()));
        }
        if !self.subresource.is_empty() {
            object_ref.insert("subresource", Value::from(self.subresource.clone()));
        }
        obj([
            ("kind", Value::from("Event")),
            ("apiVersion", Value::from("audit.k8s.io/v1")),
            ("stage", Value::from(self.stage.clone())),
            ("verb", Value::from(self.verb.clone())),
            ("user", obj([("username", Value::from(self.user.clone()))])),
            ("objectRef", object_ref),
            ("requestURI", Value::from(self.request_uri.clone())),
            (
                "responseStatus",
                obj([("code", Value::from(i64::from(self.response_code)))]),
            ),
        ])
    }
}

/// A destination for audit events. Implementors must be `Send + Sync` so a
/// shared server can record from any connection thread.
pub trait AuditSink: Send + Sync {
    /// Record one completed event.
    fn record(&self, event: &AuditEvent);
}

/// A no-op sink (auditing disabled).
#[derive(Debug, Default)]
pub struct NoopAuditSink;

impl AuditSink for NoopAuditSink {
    fn record(&self, _event: &AuditEvent) {}
}

/// An in-memory sink that accumulates events — used by tests and as the simplest
/// backend.
#[derive(Debug, Default)]
pub struct MemoryAuditSink {
    events: Mutex<Vec<AuditEvent>>,
}

impl MemoryAuditSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A snapshot copy of the recorded events.
    #[must_use]
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// The number of recorded events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// True if no events have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AuditSink for MemoryAuditSink {
    fn record(&self, event: &AuditEvent) {
        if let Ok(mut g) = self.events.lock() {
            g.push(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Value;
    use crate::path;

    #[test]
    fn audit_event_to_json_shape() {
        let mut e = AuditEvent::started("POST", "/api/v1/namespaces/default/pods");
        e.user = "alice".to_string();
        e.set_resource(&path::parse("/api/v1/namespaces/default/pods/nginx").expect("parse"));
        e.verb = "create".to_string();
        e.response_code = 201;
        let v = e.to_json();
        assert_eq!(v.pointer("kind"), Some(&Value::from("Event")));
        assert_eq!(v.pointer("apiVersion"), Some(&Value::from("audit.k8s.io/v1")));
        assert_eq!(v.pointer("verb"), Some(&Value::from("create")));
        assert_eq!(v.pointer("user.username"), Some(&Value::from("alice")));
        assert_eq!(v.pointer("objectRef.resource"), Some(&Value::from("pods")));
        assert_eq!(v.pointer("objectRef.namespace"), Some(&Value::from("default")));
        assert_eq!(v.pointer("objectRef.name"), Some(&Value::from("nginx")));
        assert_eq!(v.pointer("responseStatus.code"), Some(&Value::from(201_i64)));
    }

    #[test]
    fn memory_sink_records_in_order() {
        let sink = MemoryAuditSink::new();
        assert!(sink.is_empty());
        sink.record(&AuditEvent::started("GET", "/healthz"));
        sink.record(&AuditEvent::started("GET", "/api/v1/pods"));
        assert_eq!(sink.len(), 2);
        assert_eq!(sink.events()[1].request_uri, "/api/v1/pods");
    }
}
