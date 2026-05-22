// SPDX-License-Identifier: Apache-2.0
//! core/v1 resource registry.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! staging/src/k8s.io/api/core/v1/register.go

/// The kinds the API server serves under `/api/v1/`.
///
/// Each tuple is `(plural-resource, kind, is_namespaced)`. Plural is what
/// appears in the URL; kind is what appears in the JSON `kind` field.
pub const KINDS: &[(&str, &str, bool)] = &[
    ("pods", "Pod", true),
    ("services", "Service", true),
    ("endpoints", "Endpoints", true),
    ("namespaces", "Namespace", false),
    ("configmaps", "ConfigMap", true),
    ("secrets", "Secret", true),
    ("serviceaccounts", "ServiceAccount", true),
    ("nodes", "Node", false),
];

/// Return the kind for a given plural resource, or `None` if unknown.
#[must_use]
pub fn kind_of(resource: &str) -> Option<&'static str> {
    KINDS.iter().find_map(|(r, k, _)| (*r == resource).then_some(*k))
}

/// Whether the resource is namespaced.
#[must_use]
pub fn is_namespaced(resource: &str) -> Option<bool> {
    KINDS.iter().find_map(|(r, _, ns)| (*r == resource).then_some(*ns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pods_are_namespaced() {
        assert_eq!(is_namespaced("pods"), Some(true));
        assert_eq!(kind_of("pods"), Some("Pod"));
    }

    #[test]
    fn nodes_are_cluster_scoped() {
        assert_eq!(is_namespaced("nodes"), Some(false));
        assert_eq!(kind_of("nodes"), Some("Node"));
    }

    #[test]
    fn unknown_returns_none() {
        assert!(kind_of("widgets").is_none());
        assert!(is_namespaced("widgets").is_none());
    }
}
