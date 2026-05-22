// SPDX-License-Identifier: Apache-2.0
//! RBAC authorization.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/api/rbac/v1/types.go
//! - plugin/pkg/auth/authorizer/rbac/rbac.go

use async_trait::async_trait;
use thiserror::Error;

use crate::types::{ResourceRef, UserInfo, Verb};

/// AuthZ-layer errors.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AuthzError {
    #[error("internal: {0}")]
    Internal(String),
}

/// Decision returned by the authorizer.
///
/// Source: staging/src/k8s.io/apiserver/pkg/authorization/authorizer/interfaces.go::Decision
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthzDecision {
    Allow,
    Deny,
    NoOpinion,
}

/// Convenience alias.
pub type AuthzResult = Result<AuthzDecision, AuthzError>;

/// A single RBAC rule.
///
/// Source: staging/src/k8s.io/api/rbac/v1/types.go::PolicyRule
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rule {
    pub api_groups: Vec<String>,
    pub resources: Vec<String>,
    pub resource_names: Vec<String>,
    pub verbs: Vec<String>,
}

impl Rule {
    /// Does this rule match the requested attributes?
    #[must_use]
    pub fn matches(&self, group: &str, resource: &str, name: &str, verb: &str) -> bool {
        Self::matches_field(&self.api_groups, group)
            && Self::matches_field(&self.resources, resource)
            && Self::matches_field(&self.verbs, verb)
            && (self.resource_names.is_empty()
                || self.resource_names.iter().any(|n| n == name))
    }

    fn matches_field(values: &[String], requested: &str) -> bool {
        values.iter().any(|v| v == "*" || v == requested)
    }
}

/// A bound rule set scoped to a namespace (or cluster if empty).
#[derive(Clone, Debug, Default)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
    pub namespace: String,
}

/// Authorizer trait.
///
/// Source: staging/src/k8s.io/apiserver/pkg/authorization/authorizer/interfaces.go::Authorizer
#[async_trait]
pub trait Authorizer: Send + Sync {
    async fn authorize(
        &self,
        user: &UserInfo,
        resource: &ResourceRef,
        verb: Verb,
    ) -> AuthzResult;
}

/// RBAC authorizer.
///
/// Source: plugin/pkg/auth/authorizer/rbac/rbac.go::RBACAuthorizer
pub struct RbacAuthorizer {
    /// `(subject, rules)` pairs, cluster-scoped.
    cluster_rules: Vec<(String, Vec<Rule>)>,
    /// `((subject, namespace), rules)` pairs.
    namespace_rules: Vec<((String, String), Vec<Rule>)>,
}

impl RbacAuthorizer {
    /// Construct an empty authorizer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cluster_rules: Vec::new(),
            namespace_rules: Vec::new(),
        }
    }

    /// Bind a cluster-scoped rule set to a user name or group.
    pub fn bind_cluster(&mut self, subject: impl Into<String>, rules: Vec<Rule>) {
        self.cluster_rules.push((subject.into(), rules));
    }

    /// Bind a namespace-scoped rule set to a subject within one namespace.
    pub fn bind_namespace(
        &mut self,
        subject: impl Into<String>,
        namespace: impl Into<String>,
        rules: Vec<Rule>,
    ) {
        self.namespace_rules
            .push(((subject.into(), namespace.into()), rules));
    }

    fn subjects(user: &UserInfo) -> Vec<String> {
        let mut out = vec![user.name.clone()];
        out.extend(user.groups.iter().cloned());
        out
    }
}

impl Default for RbacAuthorizer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Authorizer for RbacAuthorizer {
    async fn authorize(
        &self,
        user: &UserInfo,
        resource: &ResourceRef,
        verb: Verb,
    ) -> AuthzResult {
        let subjects = Self::subjects(user);
        let verb = verb.as_str();

        // Cluster bindings apply to any namespace.
        for (subject, rules) in &self.cluster_rules {
            if !subjects.iter().any(|s| s == subject) {
                continue;
            }
            for rule in rules {
                if rule.matches(&resource.group, &resource.resource, &resource.name, verb) {
                    return Ok(AuthzDecision::Allow);
                }
            }
        }

        // Namespace bindings only apply if scope matches.
        for ((subject, ns), rules) in &self.namespace_rules {
            if !subjects.iter().any(|s| s == subject) {
                continue;
            }
            if !resource.namespace.is_empty() && ns != &resource.namespace {
                continue;
            }
            for rule in rules {
                if rule.matches(&resource.group, &resource.resource, &resource.name, verb) {
                    return Ok(AuthzDecision::Allow);
                }
            }
        }

        Ok(AuthzDecision::NoOpinion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Verb;

    #[tokio::test]
    async fn empty_authorizer_returns_no_opinion() {
        let a = RbacAuthorizer::new();
        let user = UserInfo::new("alice");
        let ref_ = ResourceRef::namespaced("", "v1", "pods", "default", "p");
        let d = a.authorize(&user, &ref_, Verb::Get).await.expect("ok");
        assert_eq!(d, AuthzDecision::NoOpinion);
    }

    #[tokio::test]
    async fn cluster_admin_can_do_anything() {
        let mut a = RbacAuthorizer::new();
        a.bind_cluster(
            "alice",
            vec![Rule {
                api_groups: vec!["*".into()],
                resources: vec!["*".into()],
                verbs: vec!["*".into()],
                resource_names: vec![],
            }],
        );
        let user = UserInfo::new("alice");
        let ref_ = ResourceRef::namespaced("", "v1", "pods", "default", "p");
        let d = a.authorize(&user, &ref_, Verb::Delete).await.expect("ok");
        assert_eq!(d, AuthzDecision::Allow);
    }

    #[tokio::test]
    async fn namespace_binding_only_allows_within_scope() {
        let mut a = RbacAuthorizer::new();
        a.bind_namespace(
            "alice",
            "default",
            vec![Rule {
                api_groups: vec!["".into()],
                resources: vec!["pods".into()],
                verbs: vec!["get".into()],
                resource_names: vec![],
            }],
        );
        let user = UserInfo::new("alice");
        let in_default = ResourceRef::namespaced("", "v1", "pods", "default", "p");
        let in_kube = ResourceRef::namespaced("", "v1", "pods", "kube-system", "p");
        assert_eq!(
            a.authorize(&user, &in_default, Verb::Get).await.expect("ok"),
            AuthzDecision::Allow
        );
        assert_eq!(
            a.authorize(&user, &in_kube, Verb::Get).await.expect("ok"),
            AuthzDecision::NoOpinion
        );
    }

    #[tokio::test]
    async fn group_binding_applies_to_group_members() {
        let mut a = RbacAuthorizer::new();
        a.bind_cluster(
            "system:masters",
            vec![Rule {
                api_groups: vec!["*".into()],
                resources: vec!["*".into()],
                verbs: vec!["*".into()],
                resource_names: vec![],
            }],
        );
        let mut user = UserInfo::new("bob");
        user.groups.push("system:masters".to_string());
        let ref_ = ResourceRef::namespaced("apps", "v1", "deployments", "default", "d");
        let d = a.authorize(&user, &ref_, Verb::Create).await.expect("ok");
        assert_eq!(d, AuthzDecision::Allow);
    }

    #[tokio::test]
    async fn resource_names_filter_works() {
        let mut a = RbacAuthorizer::new();
        a.bind_namespace(
            "alice",
            "default",
            vec![Rule {
                api_groups: vec!["".into()],
                resources: vec!["secrets".into()],
                resource_names: vec!["my-secret".into()],
                verbs: vec!["get".into()],
            }],
        );
        let user = UserInfo::new("alice");
        let allowed = ResourceRef::namespaced("", "v1", "secrets", "default", "my-secret");
        let denied = ResourceRef::namespaced("", "v1", "secrets", "default", "other");
        assert_eq!(
            a.authorize(&user, &allowed, Verb::Get).await.expect("ok"),
            AuthzDecision::Allow
        );
        assert_eq!(
            a.authorize(&user, &denied, Verb::Get).await.expect("ok"),
            AuthzDecision::NoOpinion
        );
    }
}
