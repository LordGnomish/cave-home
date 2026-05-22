// SPDX-License-Identifier: Apache-2.0
//! Common API server types.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/apiserver/pkg/authentication/user/user.go::Info
//! - staging/src/k8s.io/apiserver/pkg/endpoints/request/requestinfo.go::RequestInfo
//! - staging/src/k8s.io/apiserver/pkg/admission/attributes.go::Attributes

use std::collections::BTreeMap;

/// Authenticated user identity carried across authn/authz/admission/registry.
///
/// Source: staging/src/k8s.io/apiserver/pkg/authentication/user/user.go::Info
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserInfo {
    /// Human-readable name (e.g. `"admin"`, `"system:serviceaccount:default:default"`).
    pub name: String,
    /// Opaque ID — may be empty.
    pub uid: String,
    /// Group memberships (drive RBAC bindings).
    pub groups: Vec<String>,
    /// Extra free-form key/value attributes.
    pub extra: BTreeMap<String, Vec<String>>,
}

impl UserInfo {
    /// Convenience constructor for tests.
    #[must_use]
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self {
            name: name.into(),
            uid: String::new(),
            groups: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    /// Anonymous-equivalent identity (matches the upstream "system:anonymous").
    #[must_use]
    pub fn anonymous() -> Self {
        Self {
            name: "system:anonymous".to_string(),
            uid: String::new(),
            groups: vec!["system:unauthenticated".to_string()],
            extra: BTreeMap::new(),
        }
    }
}

/// Resource reference used by storage / authz / admission.
///
/// Source: staging/src/k8s.io/apiserver/pkg/endpoints/request/requestinfo.go::RequestInfo
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ResourceRef {
    /// API group ("" for core/v1).
    pub group: String,
    /// API version (e.g. `"v1"`).
    pub version: String,
    /// Plural resource name (e.g. `"pods"`, `"deployments"`).
    pub resource: String,
    /// Namespace (empty for cluster-scoped resources).
    pub namespace: String,
    /// Object name (empty for collection operations).
    pub name: String,
}

impl ResourceRef {
    /// Construct a reference for a namespaced object.
    pub fn namespaced<G, V, R, N, O>(group: G, version: V, resource: R, namespace: N, name: O) -> Self
    where
        G: Into<String>,
        V: Into<String>,
        R: Into<String>,
        N: Into<String>,
        O: Into<String>,
    {
        Self {
            group: group.into(),
            version: version.into(),
            resource: resource.into(),
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// Construct a reference for a cluster-scoped object.
    pub fn cluster<G, V, R, N>(group: G, version: V, resource: R, name: N) -> Self
    where
        G: Into<String>,
        V: Into<String>,
        R: Into<String>,
        N: Into<String>,
    {
        Self {
            group: group.into(),
            version: version.into(),
            resource: resource.into(),
            namespace: String::new(),
            name: name.into(),
        }
    }

    /// Storage key — `<group>/<version>/<resource>/<namespace>/<name>` with
    /// empty fields collapsed.
    #[must_use]
    pub fn storage_key(&self) -> String {
        let g = if self.group.is_empty() { "core" } else { &self.group };
        if self.namespace.is_empty() {
            format!("{g}/{}/{}/{}", self.version, self.resource, self.name)
        } else {
            format!(
                "{g}/{}/{}/{}/{}",
                self.version, self.resource, self.namespace, self.name
            )
        }
    }
}

/// HTTP verb on a resource (drives both authz and admission).
///
/// Source: staging/src/k8s.io/apiserver/pkg/authorization/authorizer/interfaces.go::Attributes
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Verb {
    Get,
    List,
    Watch,
    Create,
    Update,
    Patch,
    Delete,
    DeleteCollection,
}

impl Verb {
    /// Lower-case wire form used by RBAC rules.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Get => "get",
            Verb::List => "list",
            Verb::Watch => "watch",
            Verb::Create => "create",
            Verb::Update => "update",
            Verb::Patch => "patch",
            Verb::Delete => "delete",
            Verb::DeleteCollection => "deletecollection",
        }
    }
}

/// Admission attribute set (what the chain sees on each call).
///
/// Source: staging/src/k8s.io/apiserver/pkg/admission/attributes.go::Attributes
#[derive(Clone, Debug)]
pub struct AdmissionAttributes {
    pub resource: ResourceRef,
    pub verb: Verb,
    pub user: UserInfo,
    /// Object the user is creating / updating; `None` for delete.
    pub object: Option<serde_json::Value>,
    /// Previous version, for update / patch.
    pub old_object: Option<serde_json::Value>,
    /// True for dry-run / validate-only.
    pub dry_run: bool,
}

/// Content-type the client wants back.
///
/// Source: staging/src/k8s.io/apiserver/pkg/server/negotiation
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContentType {
    #[default]
    Json,
    Yaml,
}

impl ContentType {
    /// MIME wire form.
    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            ContentType::Json => "application/json",
            ContentType::Yaml => "application/yaml",
        }
    }

    /// Parse from an HTTP `Accept` / `Content-Type` header.
    #[must_use]
    pub fn parse(header: &str) -> Self {
        let lower = header.trim().to_ascii_lowercase();
        if lower.contains("yaml") {
            ContentType::Yaml
        } else {
            ContentType::Json
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_anonymous_has_unauth_group() {
        let u = UserInfo::anonymous();
        assert_eq!(u.name, "system:anonymous");
        assert!(u.groups.contains(&"system:unauthenticated".to_string()));
    }

    #[test]
    fn resource_storage_key_namespaced() {
        let r = ResourceRef::namespaced("", "v1", "pods", "default", "nginx");
        assert_eq!(r.storage_key(), "core/v1/pods/default/nginx");
    }

    #[test]
    fn resource_storage_key_cluster() {
        let r = ResourceRef::cluster("", "v1", "namespaces", "default");
        assert_eq!(r.storage_key(), "core/v1/namespaces/default");
    }

    #[test]
    fn verb_wire_strings_match_rbac() {
        assert_eq!(Verb::Get.as_str(), "get");
        assert_eq!(Verb::DeleteCollection.as_str(), "deletecollection");
    }

    #[test]
    fn content_type_yaml_detected() {
        assert_eq!(ContentType::parse("application/yaml"), ContentType::Yaml);
        assert_eq!(ContentType::parse("application/json"), ContentType::Json);
        assert_eq!(ContentType::parse("*/*"), ContentType::Json);
    }
}
