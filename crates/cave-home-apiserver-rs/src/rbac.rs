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

use crate::status::{Result, Status};

/// The authorizer's verdict. RBAC is purely additive: it only ever returns
/// [`Decision::Allow`] (a rule matched) or [`Decision::NoOpinion`] (nothing
/// matched — the chain falls through to the next authorizer, or to a default
/// deny). [`Decision::Deny`] exists for completeness of the chain contract but
/// is never produced by this authorizer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    /// A rule explicitly grants the request.
    Allow,
    /// An authorizer explicitly forbids the request (not produced by RBAC).
    Deny,
    /// No applicable rule; defer to the next authorizer / default deny.
    NoOpinion,
}

/// The authenticated identity a request carries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserInfo {
    /// The user name (for a ServiceAccount this is
    /// `system:serviceaccount:<namespace>:<name>`).
    pub name: String,
    /// The groups the user belongs to.
    pub groups: Vec<String>,
}

impl UserInfo {
    /// A user with no groups.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), groups: Vec::new() }
    }

    /// Attach group memberships.
    #[must_use]
    pub fn with_groups(mut self, groups: &[&str]) -> Self {
        self.groups = groups.iter().map(|g| (*g).to_string()).collect();
        self
    }
}

/// The attributes of a single request to authorize. Either a *resource* request
/// (apiGroup / resource / subresource / namespace / name) or a *non-resource*
/// request (a raw URL `path`), distinguished by `resource_request`.
#[derive(Clone, Debug, Default)]
pub struct Attributes {
    /// The authenticated caller.
    pub user: UserInfo,
    /// The verb (`get`, `list`, `watch`, `create`, `update`, `patch`,
    /// `delete`, `deletecollection`, ... or an HTTP method for non-resource).
    pub verb: String,
    /// True for a resource request; false for a non-resource URL request.
    pub resource_request: bool,
    /// API group (`""` is the core group). Resource requests only.
    pub api_group: String,
    /// Plural resource name (`pods`, `deployments`). Resource requests only.
    pub resource: String,
    /// Subresource (`log`, `status`, `exec`), if any. Resource requests only.
    pub subresource: String,
    /// Object name (empty for collection verbs like list). Resource requests.
    pub name: String,
    /// Namespace (empty for cluster-scoped resources). Resource requests.
    pub namespace: String,
    /// The URL path. Non-resource requests only.
    pub path: String,
}

impl Attributes {
    /// A resource request.
    #[must_use]
    pub fn resource(
        user: UserInfo,
        verb: impl Into<String>,
        api_group: impl Into<String>,
        resource: impl Into<String>,
        namespace: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            user,
            verb: verb.into(),
            resource_request: true,
            api_group: api_group.into(),
            resource: resource.into(),
            subresource: String::new(),
            name: name.into(),
            namespace: namespace.into(),
            path: String::new(),
        }
    }

    /// A non-resource (raw URL) request.
    #[must_use]
    pub fn non_resource(user: UserInfo, verb: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            user,
            verb: verb.into(),
            resource_request: false,
            path: path.into(),
            ..Self::default()
        }
    }

    /// Set the subresource on a resource request.
    #[must_use]
    pub fn with_subresource(mut self, subresource: impl Into<String>) -> Self {
        self.subresource = subresource.into();
        self
    }

    /// The `resource[/subresource]` string a rule's `resources` list is matched
    /// against.
    fn combined_resource(&self) -> String {
        if self.subresource.is_empty() {
            self.resource.clone()
        } else {
            format!("{}/{}", self.resource, self.subresource)
        }
    }
}

/// A single grant. A rule is either a *resource* rule (verbs × apiGroups ×
/// resources, optionally narrowed by `resource_names`) or a *non-resource* rule
/// (verbs × `non_resource_urls`). The two forms are kept on one struct to mirror
/// the documented `PolicyRule`; a given rule normally populates only one form.
#[derive(Clone, Debug, Default)]
pub struct PolicyRule {
    /// Verbs granted (`"*"` = all).
    pub verbs: Vec<String>,
    /// API groups (`"*"` = all, `""` = core). Resource rules.
    pub api_groups: Vec<String>,
    /// Resources (`"*"` = all; `resource/subresource` and `*/subresource`
    /// forms supported). Resource rules.
    pub resources: Vec<String>,
    /// If non-empty, restrict to these object names. Resource rules.
    pub resource_names: Vec<String>,
    /// URL paths (`"*"` = all, trailing `*` = prefix). Non-resource rules.
    pub non_resource_urls: Vec<String>,
}

impl PolicyRule {
    /// A resource rule over `(api_groups, resources, verbs)`.
    #[must_use]
    pub fn resource_rule(api_groups: &[&str], resources: &[&str], verbs: &[&str]) -> Self {
        Self {
            verbs: to_strings(verbs),
            api_groups: to_strings(api_groups),
            resources: to_strings(resources),
            resource_names: Vec::new(),
            non_resource_urls: Vec::new(),
        }
    }

    /// A non-resource rule over `(non_resource_urls, verbs)`.
    #[must_use]
    pub fn non_resource_rule(non_resource_urls: &[&str], verbs: &[&str]) -> Self {
        Self {
            verbs: to_strings(verbs),
            api_groups: Vec::new(),
            resources: Vec::new(),
            resource_names: Vec::new(),
            non_resource_urls: to_strings(non_resource_urls),
        }
    }

    /// Narrow a resource rule to specific object names.
    #[must_use]
    pub fn with_resource_names(mut self, names: &[&str]) -> Self {
        self.resource_names = to_strings(names);
        self
    }

    /// Whether this rule grants `attrs`.
    #[must_use]
    pub fn allows(&self, attrs: &Attributes) -> bool {
        if attrs.resource_request {
            verb_matches(&self.verbs, &attrs.verb)
                && api_group_matches(&self.api_groups, &attrs.api_group)
                && resource_matches(&self.resources, &attrs.combined_resource(), &attrs.subresource)
                && resource_name_matches(&self.resource_names, &attrs.name)
        } else {
            verb_matches(&self.verbs, &attrs.verb)
                && self
                    .non_resource_urls
                    .iter()
                    .any(|u| non_resource_url_matches(u, &attrs.path))
        }
    }
}

/// The kind of an RBAC subject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubjectKind {
    /// A user identity.
    User,
    /// A group identity.
    Group,
    /// A service account (matched via its canonical username).
    ServiceAccount,
}

/// A binding subject: who a binding grants its role to.
#[derive(Clone, Debug)]
pub struct Subject {
    /// Subject kind.
    pub kind: SubjectKind,
    /// User name, group name, or ServiceAccount name.
    pub name: String,
    /// Namespace (ServiceAccount subjects only).
    pub namespace: String,
}

impl Subject {
    /// A user subject.
    #[must_use]
    pub fn user(name: impl Into<String>) -> Self {
        Self { kind: SubjectKind::User, name: name.into(), namespace: String::new() }
    }

    /// A group subject.
    #[must_use]
    pub fn group(name: impl Into<String>) -> Self {
        Self { kind: SubjectKind::Group, name: name.into(), namespace: String::new() }
    }

    /// A service-account subject in `namespace`.
    #[must_use]
    pub fn service_account(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: SubjectKind::ServiceAccount,
            name: name.into(),
            namespace: namespace.into(),
        }
    }

    /// Whether this subject names `user`.
    fn matches(&self, user: &UserInfo) -> bool {
        match self.kind {
            SubjectKind::User => self.name == user.name,
            SubjectKind::Group => user.groups.iter().any(|g| g == &self.name),
            SubjectKind::ServiceAccount => {
                user.name == format!("system:serviceaccount:{}:{}", self.namespace, self.name)
            }
        }
    }
}

/// Which rule-holder a binding refers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoleRefKind {
    /// A namespaced [`Role`].
    Role,
    /// A cluster-wide [`ClusterRole`].
    ClusterRole,
}

/// A reference from a binding to a [`Role`] or [`ClusterRole`].
#[derive(Clone, Debug)]
pub struct RoleRef {
    /// Referent kind.
    pub kind: RoleRefKind,
    /// Referent name.
    pub name: String,
}

impl RoleRef {
    /// Reference a namespaced [`Role`].
    #[must_use]
    pub fn role(name: impl Into<String>) -> Self {
        Self { kind: RoleRefKind::Role, name: name.into() }
    }

    /// Reference a [`ClusterRole`].
    #[must_use]
    pub fn cluster_role(name: impl Into<String>) -> Self {
        Self { kind: RoleRefKind::ClusterRole, name: name.into() }
    }
}

/// A namespaced collection of rules.
#[derive(Clone, Debug)]
pub struct Role {
    /// Namespace the role lives in.
    pub namespace: String,
    /// Role name.
    pub name: String,
    /// Granted rules.
    pub rules: Vec<PolicyRule>,
}

/// A cluster-wide collection of rules.
#[derive(Clone, Debug)]
pub struct ClusterRole {
    /// Role name.
    pub name: String,
    /// Granted rules.
    pub rules: Vec<PolicyRule>,
}

/// Binds subjects to a role within a single namespace.
#[derive(Clone, Debug)]
pub struct RoleBinding {
    /// Namespace the binding (and any same-namespace Role) lives in.
    pub namespace: String,
    /// Binding name.
    pub name: String,
    /// Subjects granted the referenced role.
    pub subjects: Vec<Subject>,
    /// The Role or ClusterRole whose rules are granted.
    pub role_ref: RoleRef,
}

/// Binds subjects to a ClusterRole cluster-wide.
#[derive(Clone, Debug)]
pub struct ClusterRoleBinding {
    /// Binding name.
    pub name: String,
    /// Subjects granted the referenced ClusterRole.
    pub subjects: Vec<Subject>,
    /// The ClusterRole whose rules are granted (a Role ref is meaningless here).
    pub role_ref: RoleRef,
}

/// The additive RBAC authorizer over a fixed set of roles and bindings.
#[derive(Clone, Debug, Default)]
pub struct RbacAuthorizer {
    /// Namespaced roles.
    pub roles: Vec<Role>,
    /// Cluster roles.
    pub cluster_roles: Vec<ClusterRole>,
    /// Namespaced bindings.
    pub role_bindings: Vec<RoleBinding>,
    /// Cluster bindings.
    pub cluster_role_bindings: Vec<ClusterRoleBinding>,
}

impl RbacAuthorizer {
    /// An empty authorizer (grants nothing).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a [`Role`].
    #[must_use]
    pub fn with_role(mut self, role: Role) -> Self {
        self.roles.push(role);
        self
    }

    /// Register a [`ClusterRole`].
    #[must_use]
    pub fn with_cluster_role(mut self, role: ClusterRole) -> Self {
        self.cluster_roles.push(role);
        self
    }

    /// Register a [`RoleBinding`].
    #[must_use]
    pub fn with_role_binding(mut self, binding: RoleBinding) -> Self {
        self.role_bindings.push(binding);
        self
    }

    /// Register a [`ClusterRoleBinding`].
    #[must_use]
    pub fn with_cluster_role_binding(mut self, binding: ClusterRoleBinding) -> Self {
        self.cluster_role_bindings.push(binding);
        self
    }

    /// Authorize `attrs`. Returns [`Decision::Allow`] if any applicable rule
    /// matches, otherwise [`Decision::NoOpinion`].
    #[must_use]
    pub fn authorize(&self, attrs: &Attributes) -> Decision {
        // 1. ClusterRoleBindings grant cluster-wide: every namespace, every
        //    cluster-scoped resource, and every non-resource URL.
        for crb in &self.cluster_role_bindings {
            if !crb.subjects.iter().any(|s| s.matches(&attrs.user)) {
                continue;
            }
            if crb.role_ref.kind == RoleRefKind::ClusterRole {
                if let Some(role) = self.cluster_role(&crb.role_ref.name) {
                    if role.rules.iter().any(|r| r.allows(attrs)) {
                        return Decision::Allow;
                    }
                }
            }
        }

        // 2. RoleBindings grant only namespaced *resource* access within their
        //    own namespace (never cluster-scoped resources or non-resource URLs).
        if attrs.resource_request && !attrs.namespace.is_empty() {
            for rb in &self.role_bindings {
                if rb.namespace != attrs.namespace {
                    continue;
                }
                if !rb.subjects.iter().any(|s| s.matches(&attrs.user)) {
                    continue;
                }
                let rules = match rb.role_ref.kind {
                    RoleRefKind::Role => self
                        .role(&rb.namespace, &rb.role_ref.name)
                        .map(|r| r.rules.as_slice()),
                    RoleRefKind::ClusterRole => self
                        .cluster_role(&rb.role_ref.name)
                        .map(|r| r.rules.as_slice()),
                };
                if let Some(rules) = rules {
                    if rules.iter().any(|r| r.allows(attrs)) {
                        return Decision::Allow;
                    }
                }
            }
        }

        Decision::NoOpinion
    }

    /// Authorize `attrs`, mapping any non-`Allow` decision to a `Forbidden`
    /// (403) [`Status`].
    ///
    /// # Errors
    /// A [`Status`] with reason `Forbidden` when the request is not allowed.
    pub fn authorize_or_forbid(&self, attrs: &Attributes) -> Result<()> {
        if self.authorize(attrs) == Decision::Allow {
            Ok(())
        } else {
            Err(Status::new(
                crate::status::StatusReason::Forbidden,
                forbidden_message(attrs),
            ))
        }
    }

    fn role(&self, namespace: &str, name: &str) -> Option<&Role> {
        self.roles
            .iter()
            .find(|r| r.namespace == namespace && r.name == name)
    }

    fn cluster_role(&self, name: &str) -> Option<&ClusterRole> {
        self.cluster_roles.iter().find(|r| r.name == name)
    }
}

// ---------------------------------------------------------------------------
// Matching primitives (documented RBAC semantics).
// ---------------------------------------------------------------------------

fn verb_matches(verbs: &[String], verb: &str) -> bool {
    verbs.iter().any(|v| v == "*" || v == verb)
}

fn api_group_matches(groups: &[String], group: &str) -> bool {
    groups.iter().any(|g| g == "*" || g == group)
}

/// `combined` is `resource[/subresource]`; `subresource` is the bare
/// subresource (empty if none). Matches `"*"`, an exact `resource`/
/// `resource/subresource`, or the `*/subresource` wildcard form.
fn resource_matches(resources: &[String], combined: &str, subresource: &str) -> bool {
    resources.iter().any(|r| {
        r == "*"
            || r == combined
            || (!subresource.is_empty()
                && r.starts_with("*/")
                && &r[2..] == subresource)
    })
}

fn resource_name_matches(resource_names: &[String], name: &str) -> bool {
    if resource_names.is_empty() {
        return true;
    }
    !name.is_empty() && resource_names.iter().any(|n| n == name)
}

/// Matches `"*"`, an exact path, or a trailing-`*` prefix (`/api/*`).
fn non_resource_url_matches(rule: &str, path: &str) -> bool {
    if rule == "*" || rule == path {
        return true;
    }
    if let Some(prefix) = rule.strip_suffix('*') {
        return path.starts_with(prefix);
    }
    false
}

fn forbidden_message(attrs: &Attributes) -> String {
    if attrs.resource_request {
        let scope = if attrs.namespace.is_empty() {
            "cluster scope".to_string()
        } else {
            format!("namespace \"{}\"", attrs.namespace)
        };
        format!(
            "{} cannot {} resource \"{}\" in API group \"{}\" at the {}",
            quote_user(&attrs.user.name),
            attrs.verb,
            attrs.combined_resource(),
            attrs.api_group,
            scope
        )
    } else {
        format!(
            "{} cannot {} path \"{}\"",
            quote_user(&attrs.user.name),
            attrs.verb,
            attrs.path
        )
    }
}

fn quote_user(name: &str) -> String {
    format!("user \"{name}\"")
}

fn to_strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

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
        // "*/status" matches any resource's status subresource. apiGroup is "*"
        // here so the group is not the variable under test.
        let az = single_cluster_grant(PolicyRule::resource_rule(&["*"], &["*/status"], &["patch"]));
        let status = Attributes::resource(alice(), "patch", "apps", "deployments", "web", "d")
            .with_subresource("status");
        assert_eq!(az.authorize(&status), Decision::Allow);
        // ...but not a different subresource, nor the bare resource.
        let scale = Attributes::resource(alice(), "patch", "apps", "deployments", "web", "d")
            .with_subresource("scale");
        assert_eq!(az.authorize(&scale), Decision::NoOpinion);
        let bare = Attributes::resource(alice(), "patch", "apps", "deployments", "web", "d");
        assert_eq!(az.authorize(&bare), Decision::NoOpinion);
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
