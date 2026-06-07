// SPDX-License-Identifier: Apache-2.0
//! Discovery API — what groups, versions and resources the server serves.
//!
//! Behavioural reference: Kubernetes API conventions and the documented
//! discovery endpoints. This is a clean-room reimplementation of the *documented*
//! shapes computed over the static built-in RESTMapper kind table ([`crate::gvk`]):
//!
//! - `/api`  → the legacy core-group version list (`APIVersions`): `["v1"]`;
//! - `/apis` → the non-core group list (`APIGroupList`): one [`ApiGroup`] per
//!   registered group (`apps`, `batch`, ...), each with its versions and a
//!   preferred version. The core group (`""`) is intentionally absent — it is
//!   served under `/api`, not `/apis`;
//! - `/api/{v}` and `/apis/{g}/{v}` → the resource list (`APIResourceList`): one
//!   [`ApiResource`] per registered kind in that group/version, carrying the
//!   plural name, the singular name (the lowercased kind), the namespaced flag,
//!   the kind, and the standard verb set.
//!
//! Subresource discovery (`pods/status`, `pods/log`), CRD-contributed groups,
//! the aggregated-discovery (`APIGroupDiscoveryList`) document and OpenAPI v3
//! are deferred (see `parity.manifest.toml`).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_api_versions_is_v1() {
        assert_eq!(api_versions(), vec!["v1".to_string()]);
    }

    #[test]
    fn api_groups_lists_non_core_groups_only() {
        let names: Vec<String> = api_groups().into_iter().map(|g| g.name).collect();
        assert!(names.contains(&"apps".to_string()));
        assert!(names.contains(&"batch".to_string()));
        // The core group is served under /api, never listed in /apis.
        assert!(!names.contains(&String::new()));
    }

    #[test]
    fn api_groups_report_preferred_version() {
        let apps = api_groups().into_iter().find(|g| g.name == "apps").expect("apps group");
        assert_eq!(apps.preferred_version.version, "v1");
        assert_eq!(apps.preferred_version.group_version, "apps/v1");
        assert!(apps.versions.iter().any(|v| v.group_version == "apps/v1"));
    }

    #[test]
    fn core_resources_include_pods_with_metadata() {
        let res = api_resources("", "v1");
        let pod = res.iter().find(|r| r.name == "pods").expect("pods");
        assert_eq!(pod.kind, "Pod");
        assert_eq!(pod.singular_name, "pod");
        assert!(pod.namespaced);
        assert_eq!(pod.group, "");
        assert_eq!(pod.version, "v1");
    }

    #[test]
    fn cluster_scoped_resources_are_flagged() {
        let res = api_resources("", "v1");
        let node = res.iter().find(|r| r.name == "nodes").expect("nodes");
        assert!(!node.namespaced);
    }

    #[test]
    fn apps_resources_include_deployments_and_not_core() {
        let res = api_resources("apps", "v1");
        assert!(res.iter().any(|r| r.name == "deployments" && r.kind == "Deployment"));
        // Core resources must not leak into the apps group.
        assert!(!res.iter().any(|r| r.name == "pods"));
    }

    #[test]
    fn resources_carry_the_standard_verb_set() {
        let res = api_resources("", "v1");
        let pod = res.iter().find(|r| r.name == "pods").expect("pods");
        for v in ["get", "list", "watch", "create", "update", "patch", "delete", "deletecollection"] {
            assert!(pod.verbs.iter().any(|x| x == v), "missing verb {v}");
        }
    }

    #[test]
    fn unknown_group_version_has_no_resources() {
        assert!(api_resources("nope.example.com", "v9").is_empty());
        assert!(api_resources("", "v2").is_empty());
    }

    #[test]
    fn resources_are_sorted_by_name() {
        let res = api_resources("", "v1");
        let names: Vec<&str> = res.iter().map(|r| r.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn groups_are_sorted_by_name() {
        let names: Vec<String> = api_groups().into_iter().map(|g| g.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
