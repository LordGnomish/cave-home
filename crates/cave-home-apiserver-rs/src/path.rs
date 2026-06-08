// SPDX-License-Identifier: Apache-2.0
//! REST path model: parse and build the apiserver resource URL grammar.
//!
//! Behavioural reference: Kubernetes API conventions (`api-conventions.md`,
//! "Resource Paths"). The grammar served by the apiserver is:
//!
//! - core group:  `/api/{version}/...`
//! - named group: `/apis/{group}/{version}/...`
//!
//! followed by an optional `namespaces/{ns}/` segment then
//! `{resource}[/{name}[/{subresource}]]`.
//!
//! Clean-room reimplementation of the documented path grammar. The HTTP server
//! that dispatches these paths is deferred (see `parity.manifest.toml`).

use crate::gvk::{self, GroupVersionResource};
use crate::status::{Status, StatusReason};

/// A parsed REST request path. The scope (collection vs single object,
/// namespaced vs cluster) is derived from which fields are populated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePath {
    /// API group; empty for the core group.
    pub group: String,
    /// API version.
    pub version: String,
    /// Namespace; empty for cluster-scoped or all-namespaces requests.
    pub namespace: String,
    /// Plural resource.
    pub resource: String,
    /// Object name; empty for collection (list/create) requests.
    pub name: String,
    /// Optional subresource (`status`, `scale`, …); empty if none.
    pub subresource: String,
}

impl ResourcePath {
    /// The GVR named by this path.
    #[must_use]
    pub fn gvr(&self) -> GroupVersionResource {
        GroupVersionResource::new(self.group.clone(), self.version.clone(), self.resource.clone())
    }

    /// True if the path addresses a single named object (vs a collection).
    #[must_use]
    pub fn is_named(&self) -> bool {
        !self.name.is_empty()
    }

    /// True if the path carries a namespace.
    #[must_use]
    pub fn is_namespaced(&self) -> bool {
        !self.namespace.is_empty()
    }

    /// Reconstruct the canonical URL path for this resource reference.
    #[must_use]
    pub fn to_path(&self) -> String {
        let mut p = if self.group.is_empty() {
            format!("/api/{}", self.version)
        } else {
            format!("/apis/{}/{}", self.group, self.version)
        };
        if !self.namespace.is_empty() {
            p.push_str("/namespaces/");
            p.push_str(&self.namespace);
        }
        p.push('/');
        p.push_str(&self.resource);
        if !self.name.is_empty() {
            p.push('/');
            p.push_str(&self.name);
            if !self.subresource.is_empty() {
                p.push('/');
                p.push_str(&self.subresource);
            }
        }
        p
    }
}

/// Parse a REST resource path into a [`ResourcePath`].
///
/// Returns `BadRequest` on a structurally invalid path, and `NotFound` when the
/// path is well-formed but names an unregistered resource. A namespaced path on
/// a cluster-scoped resource (or vice-versa) is rejected as `BadRequest`.
///
/// # Errors
/// Returns a [`Status`] with reason `BadRequest` or `NotFound`.
pub fn parse(path: &str) -> Result<ResourcePath, Status> {
    let trimmed = path.trim_start_matches('/');
    let segs: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();

    let mut it = segs.iter().copied();
    let (group, version): (String, String) = match it.next() {
        Some("api") => {
            let v = it
                .next()
                .ok_or_else(|| Status::bad_request("missing version after /api"))?;
            (String::new(), v.to_string())
        }
        Some("apis") => {
            let g = it
                .next()
                .ok_or_else(|| Status::bad_request("missing group after /apis"))?;
            let v = it
                .next()
                .ok_or_else(|| Status::bad_request("missing version after group"))?;
            (g.to_string(), v.to_string())
        }
        _ => return Err(Status::bad_request("path must start with /api or /apis")),
    };

    // Optional namespaces/{ns}
    let mut namespace = String::new();
    let mut next = it.next();
    if next == Some("namespaces") {
        let ns = it
            .next()
            .ok_or_else(|| Status::bad_request("missing namespace name"))?;
        // `/namespaces` alone (no resource after) addresses the namespaces
        // collection itself; but `namespaces/{ns}` with nothing after is the
        // single Namespace object — handle that below.
        namespace = ns.to_string();
        next = it.next();
    }

    let resource = match next {
        Some(r) => r.to_string(),
        None => {
            // `/namespaces/{ns}` with no trailing resource: this is a GET of the
            // single Namespace object named {ns} in the core group.
            if !namespace.is_empty() {
                return finish(
                    String::new(),
                    version,
                    String::new(),
                    "namespaces".to_string(),
                    namespace,
                    String::new(),
                );
            }
            return Err(Status::bad_request("missing resource segment"));
        }
    };

    let name = it.next().unwrap_or_default().to_string();
    let subresource = it.next().unwrap_or_default().to_string();
    if it.next().is_some() {
        return Err(Status::bad_request("path has trailing segments"));
    }

    finish(group, version, namespace, resource, name, subresource)
}

fn finish(
    group: String,
    version: String,
    namespace: String,
    resource: String,
    name: String,
    subresource: String,
) -> Result<ResourcePath, Status> {
    let gvr = GroupVersionResource::new(group.clone(), version.clone(), resource.clone());
    let ns_scoped = gvk::is_namespaced(&gvr)
        .ok_or_else(|| Status::new(StatusReason::NotFound, format!("the server could not find the requested resource ({resource})")))?;

    if ns_scoped && namespace.is_empty() && name.is_empty() {
        // collection across all namespaces — allowed (list/watch).
    }
    if !ns_scoped && !namespace.is_empty() {
        return Err(Status::bad_request(format!(
            "resource {resource} is cluster-scoped but a namespace was given"
        )));
    }

    Ok(ResourcePath {
        group,
        version,
        namespace,
        resource,
        name,
        subresource,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_core_namespaced_named() {
        let p = parse("/api/v1/namespaces/default/pods/nginx").expect("parse");
        assert_eq!(p.group, "");
        assert_eq!(p.version, "v1");
        assert_eq!(p.namespace, "default");
        assert_eq!(p.resource, "pods");
        assert_eq!(p.name, "nginx");
        assert!(p.is_named());
        assert!(p.is_namespaced());
    }

    #[test]
    fn parse_core_collection_all_namespaces() {
        let p = parse("/api/v1/pods").expect("parse");
        assert_eq!(p.namespace, "");
        assert_eq!(p.resource, "pods");
        assert!(!p.is_named());
    }

    #[test]
    fn parse_grouped_named() {
        let p = parse("/apis/apps/v1/namespaces/web/deployments/site").expect("parse");
        assert_eq!(p.group, "apps");
        assert_eq!(p.version, "v1");
        assert_eq!(p.namespace, "web");
        assert_eq!(p.resource, "deployments");
        assert_eq!(p.name, "site");
    }

    #[test]
    fn parse_cluster_scoped_named() {
        let p = parse("/api/v1/nodes/worker-1").expect("parse");
        assert_eq!(p.resource, "nodes");
        assert_eq!(p.name, "worker-1");
        assert!(!p.is_namespaced());
    }

    #[test]
    fn parse_single_namespace_object() {
        let p = parse("/api/v1/namespaces/kube-system").expect("parse");
        assert_eq!(p.resource, "namespaces");
        assert_eq!(p.name, "kube-system");
        assert_eq!(p.namespace, "");
    }

    #[test]
    fn parse_subresource() {
        let p = parse("/apis/apps/v1/namespaces/web/deployments/site/status").expect("parse");
        assert_eq!(p.subresource, "status");
    }

    #[test]
    fn build_round_trips_parse() {
        for raw in [
            "/api/v1/namespaces/default/pods/nginx",
            "/api/v1/pods",
            "/apis/apps/v1/namespaces/web/deployments/site",
            "/api/v1/nodes/worker-1",
            "/apis/apps/v1/namespaces/web/deployments/site/status",
        ] {
            let p = parse(raw).expect("parse");
            assert_eq!(p.to_path(), raw, "round-trip for {raw}");
        }
    }

    #[test]
    fn unknown_resource_is_not_found() {
        let err = parse("/apis/example.com/v1/widgets").expect_err("should fail");
        assert_eq!(err.reason, StatusReason::NotFound);
        assert_eq!(err.code, 404);
    }

    #[test]
    fn cluster_scoped_with_namespace_is_bad_request() {
        let err = parse("/api/v1/namespaces/default/nodes/n1").expect_err("should fail");
        assert_eq!(err.reason, StatusReason::BadRequest);
    }

    #[test]
    fn bad_prefix_is_bad_request() {
        assert_eq!(parse("/foo/v1/pods").unwrap_err().reason, StatusReason::BadRequest);
        assert_eq!(parse("/api").unwrap_err().reason, StatusReason::BadRequest);
    }
}
