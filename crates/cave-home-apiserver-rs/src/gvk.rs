// SPDX-License-Identifier: Apache-2.0
//! Resource identity: `GroupVersionKind`, `GroupVersionResource`, and the
//! kind ⇄ resource mapping (RESTMapper-style).
//!
//! Behavioural reference: Kubernetes API conventions (`api-conventions.md`,
//! "Resources") and the documented GVK/GVR model. Clean-room reimplementation
//! of the documented contract.

/// A `(group, version, kind)` triple. The *kind* is the CamelCase type name
/// that appears in an object's `kind` field (e.g. `Pod`, `Deployment`).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GroupVersionKind {
    /// API group; empty string for the core/legacy group.
    pub group: String,
    /// API version (e.g. `v1`, `v1beta1`).
    pub version: String,
    /// CamelCase kind name.
    pub kind: String,
}

impl GroupVersionKind {
    /// Construct a GVK.
    pub fn new(group: impl Into<String>, version: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            group: group.into(),
            version: version.into(),
            kind: kind.into(),
        }
    }

    /// The `apiVersion` wire string: `version` for the core group, otherwise
    /// `group/version`.
    #[must_use]
    pub fn api_version(&self) -> String {
        if self.group.is_empty() {
            self.version.clone()
        } else {
            format!("{}/{}", self.group, self.version)
        }
    }

    /// Parse an `apiVersion` + `kind` into a GVK. `apiVersion` is either
    /// `version` (core group) or `group/version`.
    #[must_use]
    pub fn from_api_version(api_version: &str, kind: &str) -> Self {
        match api_version.split_once('/') {
            Some((g, v)) => Self::new(g, v, kind),
            None => Self::new("", api_version, kind),
        }
    }
}

/// A `(group, version, resource)` triple. The *resource* is the lowercase
/// plural that appears in REST paths (e.g. `pods`, `deployments`).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GroupVersionResource {
    /// API group; empty for the core group.
    pub group: String,
    /// API version.
    pub version: String,
    /// Lowercase plural resource name.
    pub resource: String,
}

impl GroupVersionResource {
    /// Construct a GVR.
    pub fn new(
        group: impl Into<String>,
        version: impl Into<String>,
        resource: impl Into<String>,
    ) -> Self {
        Self {
            group: group.into(),
            version: version.into(),
            resource: resource.into(),
        }
    }

    /// The `apiVersion`-style prefix for this GVR.
    #[must_use]
    pub fn group_version(&self) -> String {
        if self.group.is_empty() {
            self.version.clone()
        } else {
            format!("{}/{}", self.group, self.version)
        }
    }
}

/// One registered kind: its plural resource, CamelCase kind, and whether it is
/// namespaced.
#[derive(Clone, Copy, Debug)]
struct KindEntry {
    group: &'static str,
    version: &'static str,
    resource: &'static str,
    kind: &'static str,
    namespaced: bool,
}

/// The built-in kind table the decision core serves. A real apiserver builds
/// this dynamically from scheme registration + CRDs; we ship a static
/// documented subset (core/v1, apps/v1, batch/v1). CRD-driven dynamic
/// registration is deferred (see `parity.manifest.toml`).
const KINDS: &[KindEntry] = &[
    // core/v1 (group = "")
    KindEntry { group: "", version: "v1", resource: "pods", kind: "Pod", namespaced: true },
    KindEntry { group: "", version: "v1", resource: "services", kind: "Service", namespaced: true },
    KindEntry { group: "", version: "v1", resource: "endpoints", kind: "Endpoints", namespaced: true },
    KindEntry { group: "", version: "v1", resource: "configmaps", kind: "ConfigMap", namespaced: true },
    KindEntry { group: "", version: "v1", resource: "secrets", kind: "Secret", namespaced: true },
    KindEntry { group: "", version: "v1", resource: "serviceaccounts", kind: "ServiceAccount", namespaced: true },
    KindEntry { group: "", version: "v1", resource: "namespaces", kind: "Namespace", namespaced: false },
    KindEntry { group: "", version: "v1", resource: "nodes", kind: "Node", namespaced: false },
    KindEntry { group: "", version: "v1", resource: "persistentvolumes", kind: "PersistentVolume", namespaced: false },
    // apps/v1
    KindEntry { group: "apps", version: "v1", resource: "deployments", kind: "Deployment", namespaced: true },
    KindEntry { group: "apps", version: "v1", resource: "replicasets", kind: "ReplicaSet", namespaced: true },
    KindEntry { group: "apps", version: "v1", resource: "statefulsets", kind: "StatefulSet", namespaced: true },
    KindEntry { group: "apps", version: "v1", resource: "daemonsets", kind: "DaemonSet", namespaced: true },
    // batch/v1
    KindEntry { group: "batch", version: "v1", resource: "jobs", kind: "Job", namespaced: true },
    KindEntry { group: "batch", version: "v1", resource: "cronjobs", kind: "CronJob", namespaced: true },
];

/// Map a GVR to its GVK (RESTMapper `KindFor`). Returns `None` for unknown
/// resources.
#[must_use]
pub fn kind_for(gvr: &GroupVersionResource) -> Option<GroupVersionKind> {
    KINDS
        .iter()
        .find(|e| e.group == gvr.group && e.version == gvr.version && e.resource == gvr.resource)
        .map(|e| GroupVersionKind::new(e.group, e.version, e.kind))
}

/// Map a GVK to its GVR (RESTMapper `ResourceFor`). Returns `None` for unknown
/// kinds.
#[must_use]
pub fn resource_for(gvk: &GroupVersionKind) -> Option<GroupVersionResource> {
    KINDS
        .iter()
        .find(|e| e.group == gvk.group && e.version == gvk.version && e.kind == gvk.kind)
        .map(|e| GroupVersionResource::new(e.group, e.version, e.resource))
}

/// Whether the GVR names a namespaced resource. `None` if the resource is
/// unknown.
#[must_use]
pub fn is_namespaced(gvr: &GroupVersionResource) -> Option<bool> {
    KINDS
        .iter()
        .find(|e| e.group == gvr.group && e.version == gvr.version && e.resource == gvr.resource)
        .map(|e| e.namespaced)
}

/// True if the GVR is registered.
#[must_use]
pub fn is_known(gvr: &GroupVersionResource) -> bool {
    kind_for(gvr).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_api_version_omits_group() {
        let gvk = GroupVersionKind::new("", "v1", "Pod");
        assert_eq!(gvk.api_version(), "v1");
    }

    #[test]
    fn grouped_api_version_includes_group() {
        let gvk = GroupVersionKind::new("apps", "v1", "Deployment");
        assert_eq!(gvk.api_version(), "apps/v1");
    }

    #[test]
    fn from_api_version_parses_both_forms() {
        assert_eq!(
            GroupVersionKind::from_api_version("v1", "Pod"),
            GroupVersionKind::new("", "v1", "Pod")
        );
        assert_eq!(
            GroupVersionKind::from_api_version("apps/v1", "Deployment"),
            GroupVersionKind::new("apps", "v1", "Deployment")
        );
    }

    #[test]
    fn kind_for_maps_pods_to_pod() {
        let gvr = GroupVersionResource::new("", "v1", "pods");
        assert_eq!(kind_for(&gvr), Some(GroupVersionKind::new("", "v1", "Pod")));
    }

    #[test]
    fn resource_for_maps_deployment_to_deployments() {
        let gvk = GroupVersionKind::new("apps", "v1", "Deployment");
        assert_eq!(
            resource_for(&gvk),
            Some(GroupVersionResource::new("apps", "v1", "deployments"))
        );
    }

    #[test]
    fn namespaces_and_nodes_are_cluster_scoped() {
        assert_eq!(is_namespaced(&GroupVersionResource::new("", "v1", "namespaces")), Some(false));
        assert_eq!(is_namespaced(&GroupVersionResource::new("", "v1", "nodes")), Some(false));
        assert_eq!(is_namespaced(&GroupVersionResource::new("", "v1", "pods")), Some(true));
    }

    #[test]
    fn unknown_resource_is_not_known() {
        let gvr = GroupVersionResource::new("example.com", "v1", "widgets");
        assert!(!is_known(&gvr));
        assert!(kind_for(&gvr).is_none());
        assert!(is_namespaced(&gvr).is_none());
    }
}
