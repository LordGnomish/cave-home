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
