// SPDX-License-Identifier: Apache-2.0
//! NamespaceLifecycle admission controller — RED phase scaffold.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! plugin/pkg/admission/namespace/lifecycle/admission.go::Lifecycle

use async_trait::async_trait;

use crate::admission::{AdmissionController, AdmissionError, AdmissionResult};
use crate::types::{AdmissionAttributes, Verb};

/// Rejects mutations against namespaces that are missing or terminating.
///
/// Phase 2 simplification: we maintain an explicit "live" set instead of
/// watching the Namespace informer. Callers register / unregister via
/// `note_namespace_created` and `note_namespace_terminating` as they
/// process Namespace CRUD events.
pub struct NamespaceLifecycle {
    live_namespaces: parking_lot::RwLock<Vec<String>>,
    terminating_namespaces: parking_lot::RwLock<Vec<String>>,
}

impl NamespaceLifecycle {
    /// Construct a controller pre-seeded with the system namespaces.
    #[must_use]
    pub fn new(initial: Vec<String>) -> Self {
        Self {
            live_namespaces: parking_lot::RwLock::new(initial),
            terminating_namespaces: parking_lot::RwLock::new(Vec::new()),
        }
    }

    /// Mark a namespace live (called after a successful CREATE on a
    /// Namespace object).
    pub fn note_namespace_created(&self, ns: impl Into<String>) {
        let n = ns.into();
        let mut guard = self.live_namespaces.write();
        if !guard.contains(&n) {
            guard.push(n);
        }
    }

    /// Mark a namespace terminating (called when its deletionTimestamp is
    /// set). Subsequent mutating operations on objects in the namespace
    /// will be rejected.
    pub fn note_namespace_terminating(&self, ns: impl Into<String>) {
        let n = ns.into();
        let mut guard = self.terminating_namespaces.write();
        if !guard.contains(&n) {
            guard.push(n);
        }
    }

    fn is_live(&self, ns: &str) -> bool {
        self.live_namespaces.read().iter().any(|n| n == ns)
    }
    fn is_terminating(&self, ns: &str) -> bool {
        self.terminating_namespaces.read().iter().any(|n| n == ns)
    }
}

#[async_trait]
impl AdmissionController for NamespaceLifecycle {
    async fn validate(&self, attrs: &AdmissionAttributes) -> AdmissionResult {
        // Cluster-scoped resources are never gated by this controller.
        if attrs.resource.namespace.is_empty() {
            return Ok(());
        }
        // Allow GET / LIST / WATCH always (read-only).
        if matches!(attrs.verb, Verb::Get | Verb::List | Verb::Watch) {
            return Ok(());
        }
        // Operations on Namespace objects themselves are exempt.
        if attrs.resource.resource == "namespaces" {
            return Ok(());
        }
        let ns = &attrs.resource.namespace;
        if !self.is_live(ns) {
            return Err(AdmissionError::Rejected(format!(
                "namespace {ns} does not exist"
            )));
        }
        if self.is_terminating(ns) {
            return Err(AdmissionError::Rejected(format!(
                "namespace {ns} is terminating; CREATE/UPDATE/DELETE forbidden"
            )));
        }
        Ok(())
    }
    fn name(&self) -> &str {
        "NamespaceLifecycle"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AdmissionAttributes, ResourceRef, UserInfo};

    fn pod_attrs(ns: &str, verb: Verb) -> AdmissionAttributes {
        AdmissionAttributes {
            resource: ResourceRef::namespaced("", "v1", "pods", ns, "p"),
            verb,
            user: UserInfo::new("alice"),
            object: None,
            old_object: None,
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn rejects_create_in_missing_namespace() {
        let c = NamespaceLifecycle::new(vec!["default".into()]);
        let res = c.validate(&pod_attrs("missing", Verb::Create)).await;
        assert!(matches!(res, Err(AdmissionError::Rejected(_))));
    }

    #[tokio::test]
    async fn allows_create_in_live_namespace() {
        let c = NamespaceLifecycle::new(vec!["default".into()]);
        c.validate(&pod_attrs("default", Verb::Create))
            .await
            .expect("ok");
    }

    #[tokio::test]
    async fn rejects_create_in_terminating_namespace() {
        let c = NamespaceLifecycle::new(vec!["default".into()]);
        c.note_namespace_terminating("default");
        let res = c.validate(&pod_attrs("default", Verb::Create)).await;
        assert!(matches!(res, Err(AdmissionError::Rejected(_))));
    }

    #[tokio::test]
    async fn allows_read_in_terminating_namespace() {
        let c = NamespaceLifecycle::new(vec!["default".into()]);
        c.note_namespace_terminating("default");
        c.validate(&pod_attrs("default", Verb::Get))
            .await
            .expect("get allowed");
        c.validate(&pod_attrs("default", Verb::List))
            .await
            .expect("list allowed");
    }

    #[tokio::test]
    async fn does_not_gate_cluster_scoped_resources() {
        let c = NamespaceLifecycle::new(vec![]);
        let mut a = pod_attrs("", Verb::Create);
        a.resource = ResourceRef::cluster("", "v1", "nodes", "n1");
        c.validate(&a).await.expect("nodes are cluster-scoped");
    }
}
