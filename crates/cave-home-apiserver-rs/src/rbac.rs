// SPDX-License-Identifier: Apache-2.0
//! Authorization — RBAC (Role / ClusterRole / RoleBinding / ClusterRoleBinding).
//!
//! Behavioural reference: Kubernetes docs "Using RBAC Authorization" and the
//! documented RBAC matching contract. This is a clean-room reimplementation of
//! the *documented* authorizer semantics, not a transcription of upstream Go
//! source (k8s/k3s are Apache-2.0; ADR-002/ADR-004):
//!
//! - a request is described by [`Attributes`] (the authenticated [`UserInfo`],
//!   the verb, and either a resource target — apiGroup/resource/subresource/
//!   namespace/name — or a non-resource URL path);
//! - [`PolicyRule`] grants access; `"*"` is the wildcard for verbs, apiGroups
//!   and resources; an empty `resource_names` means "all names"; a
//!   `resource/subresource` (and `*/subresource`) form matches subresources;
//!   non-resource rules match URL paths with an optional trailing `*`;
//! - a [`Role`] is namespaced, a [`ClusterRole`] is cluster-wide; bindings
//!   ([`RoleBinding`] / [`ClusterRoleBinding`]) attach a rule-holder to a set of
//!   [`Subject`]s (User / Group / ServiceAccount);
//! - the [`RbacAuthorizer`] is **additive**: it returns [`Decision::Allow`] if
//!   any applicable rule matches, otherwise [`Decision::NoOpinion`] (RBAC never
//!   emits an explicit Deny — the surrounding authorizer chain turns a final
//!   NoOpinion into a 403). A ClusterRoleBinding grants cluster-wide (every
//!   namespace, cluster-scoped resources, and non-resource URLs); a RoleBinding
//!   grants only namespaced *resource* access within its own namespace, even
//!   when it references a ClusterRole.
//!
//! Aggregated ClusterRoles, the Node authorizer, and the dynamic
//! webhook/SubjectAccessReview surfaces are deferred (see `parity.manifest.toml`).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::StatusReason;

    fn alice() -> UserInfo {
        UserInfo::new("alice")
    }

    // ----- resource-request matching via a ClusterRoleBinding ---------------

    #[test]
    fn cluster_role_binding_grants_cluster_wide() {
        let az = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "pod-reader".into(),
                rules: vec![PolicyRule::resource_rule(&[""], &["pods"], &["get", "list"])],
            })
            .with_cluster_role_binding(ClusterRoleBinding {
                name: "alice-reads-pods".into(),
                subjects: vec![Subject::user("alice")],
                role_ref: RoleRef::cluster_role("pod-reader"),
            });
        // Works in default...
        let a = Attributes::resource(alice(), "get", "", "pods", "default", "web");
        assert_eq!(az.authorize(&a), Decision::Allow);
        // ...and in any other namespace.
        let b = Attributes::resource(alice(), "list", "", "pods", "kube-system", "");
        assert_eq!(az.authorize(&b), Decision::Allow);
    }

    #[test]
    fn no_binding_is_no_opinion() {
        let az = RbacAuthorizer::new();
        let a = Attributes::resource(alice(), "get", "", "pods", "default", "web");
        assert_eq!(az.authorize(&a), Decision::NoOpinion);
    }

    #[test]
    fn wildcard_verb_allows_any_verb() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["pods"], &["*"]));
        for verb in ["get", "delete", "patch", "create"] {
            let a = Attributes::resource(alice(), verb, "", "pods", "default", "web");
            assert_eq!(az.authorize(&a), Decision::Allow, "verb {verb}");
        }
    }

    #[test]
    fn wildcard_resource_allows_any_resource() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["*"], &["get"]));
        let a = Attributes::resource(alice(), "get", "", "configmaps", "default", "cm");
        assert_eq!(az.authorize(&a), Decision::Allow);
    }

    #[test]
    fn wildcard_api_group_allows_any_group() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&["*"], &["deployments"], &["get"]));
        let a = Attributes::resource(alice(), "get", "apps", "deployments", "default", "d");
        assert_eq!(az.authorize(&a), Decision::Allow);
    }

    #[test]
    fn verb_not_in_rule_is_no_opinion() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["pods"], &["get"]));
        let a = Attributes::resource(alice(), "delete", "", "pods", "default", "web");
        assert_eq!(az.authorize(&a), Decision::NoOpinion);
    }

    #[test]
    fn api_group_mismatch_is_no_opinion() {
        // A core-group ("") rule must not grant access to an apps-group resource.
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["deployments"], &["get"]));
        let a = Attributes::resource(alice(), "get", "apps", "deployments", "default", "d");
        assert_eq!(az.authorize(&a), Decision::NoOpinion);
    }

    #[test]
    fn resource_mismatch_is_no_opinion() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["pods"], &["get"]));
        let a = Attributes::resource(alice(), "get", "", "services", "default", "svc");
        assert_eq!(az.authorize(&a), Decision::NoOpinion);
    }

    // ----- namespaced RoleBinding scope -------------------------------------

    #[test]
    fn role_binding_grants_only_within_its_namespace() {
        let az = RbacAuthorizer::new()
            .with_role(Role {
                namespace: "web".into(),
                name: "pod-reader".into(),
                rules: vec![PolicyRule::resource_rule(&[""], &["pods"], &["get"])],
            })
            .with_role_binding(RoleBinding {
                namespace: "web".into(),
                name: "alice-web".into(),
                subjects: vec![Subject::user("alice")],
                role_ref: RoleRef::role("pod-reader"),
            });
        // Allowed in the binding's namespace...
        let here = Attributes::resource(alice(), "get", "", "pods", "web", "p");
        assert_eq!(az.authorize(&here), Decision::Allow);
        // ...denied elsewhere.
        let there = Attributes::resource(alice(), "get", "", "pods", "kube-system", "p");
        assert_eq!(az.authorize(&there), Decision::NoOpinion);
    }

    #[test]
    fn role_binding_to_cluster_role_is_scoped_to_namespace() {
        // A RoleBinding referencing a ClusterRole grants that ClusterRole's
        // rules, but only inside the binding's namespace.
        let az = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "edit".into(),
                rules: vec![PolicyRule::resource_rule(&[""], &["configmaps"], &["*"])],
            })
            .with_role_binding(RoleBinding {
                namespace: "web".into(),
                name: "alice-edit".into(),
                subjects: vec![Subject::user("alice")],
                role_ref: RoleRef::cluster_role("edit"),
            });
        let here = Attributes::resource(alice(), "update", "", "configmaps", "web", "cm");
        assert_eq!(az.authorize(&here), Decision::Allow);
        let there = Attributes::resource(alice(), "update", "", "configmaps", "prod", "cm");
        assert_eq!(az.authorize(&there), Decision::NoOpinion);
    }

    // ----- subject kinds ----------------------------------------------------

    #[test]
    fn group_subject_matches_via_user_groups() {
        let az = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "viewer".into(),
                rules: vec![PolicyRule::resource_rule(&[""], &["pods"], &["get"])],
            })
            .with_cluster_role_binding(ClusterRoleBinding {
                name: "devs-view".into(),
                subjects: vec![Subject::group("developers")],
                role_ref: RoleRef::cluster_role("viewer"),
            });
        let user = UserInfo::new("bob").with_groups(&["developers"]);
        let a = Attributes::resource(user, "get", "", "pods", "default", "p");
        assert_eq!(az.authorize(&a), Decision::Allow);
        // A user not in the group gets nothing.
        let outsider = Attributes::resource(UserInfo::new("eve"), "get", "", "pods", "default", "p");
        assert_eq!(az.authorize(&outsider), Decision::NoOpinion);
    }

    #[test]
    fn service_account_subject_matches_canonical_username() {
        let az = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "reader".into(),
                rules: vec![PolicyRule::resource_rule(&[""], &["secrets"], &["get"])],
            })
            .with_cluster_role_binding(ClusterRoleBinding {
                name: "sa-read".into(),
                subjects: vec![Subject::service_account("kube-system", "builder")],
                role_ref: RoleRef::cluster_role("reader"),
            });
        // A ServiceAccount authenticates as system:serviceaccount:<ns>:<name>.
        let sa = UserInfo::new("system:serviceaccount:kube-system:builder");
        let a = Attributes::resource(sa, "get", "", "secrets", "default", "s");
        assert_eq!(az.authorize(&a), Decision::Allow);
        // A different SA name must not match.
        let other = UserInfo::new("system:serviceaccount:kube-system:deployer");
        let b = Attributes::resource(other, "get", "", "secrets", "default", "s");
        assert_eq!(az.authorize(&b), Decision::NoOpinion);
    }

    // ----- resourceNames ----------------------------------------------------

    #[test]
    fn resource_names_restrict_to_named_object() {
        let rule = PolicyRule::resource_rule(&[""], &["configmaps"], &["get"])
            .with_resource_names(&["app-config"]);
        let az = single_cluster_grant(rule);
        // The named object is allowed.
        let named = Attributes::resource(alice(), "get", "", "configmaps", "default", "app-config");
        assert_eq!(az.authorize(&named), Decision::Allow);
        // A different name is not.
        let other = Attributes::resource(alice(), "get", "", "configmaps", "default", "other");
        assert_eq!(az.authorize(&other), Decision::NoOpinion);
        // A request with no name (e.g. list) cannot match a resourceNames rule.
        let listed = Attributes::resource(alice(), "get", "", "configmaps", "default", "");
        assert_eq!(az.authorize(&listed), Decision::NoOpinion);
    }

    // ----- subresources -----------------------------------------------------

    #[test]
    fn subresource_rule_matches_subresource_request() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["pods/log"], &["get"]));
        let a = Attributes::resource(alice(), "get", "", "pods", "default", "web")
            .with_subresource("log");
        assert_eq!(az.authorize(&a), Decision::Allow);
        // A bare "pods" rule must NOT grant the pods/log subresource.
        let bare = single_cluster_grant(PolicyRule::resource_rule(&[""], &["pods"], &["get"]));
        assert_eq!(bare.authorize(&a), Decision::NoOpinion);
    }

    #[test]
    fn wildcard_subresource_rule_matches() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["*/status"], &["patch"]));
        let a = Attributes::resource(alice(), "patch", "apps", "deployments", "web", "d")
            .with_subresource("status");
        // "*/status" matches any resource's status subresource.
        let a = Attributes { api_group: "apps".into(), ..a };
        assert_eq!(az.authorize(&a), Decision::Allow);
    }

    // ----- non-resource URLs ------------------------------------------------

    #[test]
    fn non_resource_url_exact_match() {
        let az = single_cluster_grant(PolicyRule::non_resource_rule(&["/healthz"], &["get"]));
        let a = Attributes::non_resource(alice(), "get", "/healthz");
        assert_eq!(az.authorize(&a), Decision::Allow);
        let miss = Attributes::non_resource(alice(), "get", "/metrics");
        assert_eq!(az.authorize(&miss), Decision::NoOpinion);
    }

    #[test]
    fn non_resource_url_prefix_wildcard() {
        let az = single_cluster_grant(PolicyRule::non_resource_rule(&["/api/*"], &["get"]));
        let a = Attributes::non_resource(alice(), "get", "/api/v1/pods");
        assert_eq!(az.authorize(&a), Decision::Allow);
    }

    #[test]
    fn non_resource_not_granted_by_role_binding() {
        // RoleBindings are namespaced; they can never authorize a non-resource
        // (clusterwide) URL, even if the referenced role lists it.
        let az = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "health".into(),
                rules: vec![PolicyRule::non_resource_rule(&["/healthz"], &["get"])],
            })
            .with_role_binding(RoleBinding {
                namespace: "web".into(),
                name: "alice-health".into(),
                subjects: vec![Subject::user("alice")],
                role_ref: RoleRef::cluster_role("health"),
            });
        let a = Attributes::non_resource(alice(), "get", "/healthz");
        assert_eq!(az.authorize(&a), Decision::NoOpinion);
    }

    #[test]
    fn cluster_scoped_resource_not_granted_by_role_binding() {
        // A RoleBinding (namespaced) cannot grant access to a cluster-scoped
        // resource request (namespace == "").
        let az = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "node-reader".into(),
                rules: vec![PolicyRule::resource_rule(&[""], &["nodes"], &["get"])],
            })
            .with_role_binding(RoleBinding {
                namespace: "web".into(),
                name: "alice-nodes".into(),
                subjects: vec![Subject::user("alice")],
                role_ref: RoleRef::cluster_role("node-reader"),
            });
        let a = Attributes::resource(alice(), "get", "", "nodes", "", "node-1");
        assert_eq!(az.authorize(&a), Decision::NoOpinion);
    }

    // ----- rule union -------------------------------------------------------

    #[test]
    fn rules_union_within_a_role() {
        let az = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "mixed".into(),
                rules: vec![
                    PolicyRule::resource_rule(&[""], &["pods"], &["get"]),
                    PolicyRule::resource_rule(&[""], &["services"], &["list"]),
                ],
            })
            .with_cluster_role_binding(ClusterRoleBinding {
                name: "alice-mixed".into(),
                subjects: vec![Subject::user("alice")],
                role_ref: RoleRef::cluster_role("mixed"),
            });
        assert_eq!(
            az.authorize(&Attributes::resource(alice(), "get", "", "pods", "default", "p")),
            Decision::Allow
        );
        assert_eq!(
            az.authorize(&Attributes::resource(alice(), "list", "", "services", "default", "")),
            Decision::Allow
        );
        // But a verb/resource combination spanning two rules is NOT granted.
        assert_eq!(
            az.authorize(&Attributes::resource(alice(), "list", "", "pods", "default", "")),
            Decision::NoOpinion
        );
    }

    // ----- conversion to a Status ------------------------------------------

    #[test]
    fn authorize_or_forbid_maps_decision_to_status() {
        let az = single_cluster_grant(PolicyRule::resource_rule(&[""], &["pods"], &["get"]));
        let ok = Attributes::resource(alice(), "get", "", "pods", "default", "p");
        assert!(az.authorize_or_forbid(&ok).is_ok());

        let denied = Attributes::resource(alice(), "delete", "", "pods", "default", "p");
        let err = az.authorize_or_forbid(&denied).expect_err("forbidden");
        assert_eq!(err.reason, StatusReason::Forbidden);
        assert_eq!(err.code, 403);
    }

    // ----- helper -----------------------------------------------------------

    /// Build an authorizer that grants `rule` cluster-wide to `alice`.
    fn single_cluster_grant(rule: PolicyRule) -> RbacAuthorizer {
        RbacAuthorizer::new()
            .with_cluster_role(ClusterRole { name: "r".into(), rules: vec![rule] })
            .with_cluster_role_binding(ClusterRoleBinding {
                name: "b".into(),
                subjects: vec![Subject::user("alice")],
                role_ref: RoleRef::cluster_role("r"),
            })
    }
}
