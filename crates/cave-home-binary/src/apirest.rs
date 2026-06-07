// SPDX-License-Identifier: Apache-2.0
//! The HTTP ⇄ apiserver glue: maps a parsed [`HttpRequest`] onto the
//! `cave-home-apiserver-rs` [`Registry`] verbs and renders the result.
//!
//! Behavioural reference: the Kubernetes apiserver REST surface
//! (`/api/v1/...`, list/get/watch). This is the read path the unified binary
//! actually serves today — `kubectl get nodes` / `cavehomectl get nodes` hit it.
//! Write verbs (create/update/delete over the wire) need a JSON request-body
//! parser the apiserver crate does not yet provide; until then non-GET methods
//! on a resource path return `405 MethodNotAllowed`, and the binary seeds its
//! own objects (e.g. the local Node) in-process. Health, version and Prometheus
//! `/metrics` endpoints round out the surface.
//!
//! [`handle`] is pure (`&mut Registry` in, [`HttpResponse`] out), so the whole
//! routing/serialization contract is unit-testable without a socket.

use cave_home_apiserver_rs::gvk;
use cave_home_apiserver_rs::json::{self, Value};
use cave_home_apiserver_rs::path;
use cave_home_apiserver_rs::registry::{ListOptions, Registry};
use cave_home_apiserver_rs::status::{Status, StatusReason};

use crate::http::{HttpRequest, HttpResponse};
use crate::version::BuildInfo;

/// Route and serve one request against the registry.
#[must_use]
pub fn handle(reg: &mut Registry, req: &HttpRequest) -> HttpResponse {
    // Non-resource endpoints (liveness/readiness/version/metrics) come first.
    match req.path.as_str() {
        "/healthz" | "/readyz" | "/livez" => return HttpResponse::text(200, "ok"),
        "/version" => return version_response(),
        "/metrics" => return metrics_response(reg),
        _ => {}
    }

    // Everything else is a resource path. Only the read verbs are served.
    if req.method != "GET" {
        return status_response(&Status::new(
            StatusReason::MethodNotAllowed,
            format!("{} is not supported on {}", req.method, req.path),
        ));
    }

    let rp = match path::parse(&req.path) {
        Ok(rp) => rp,
        Err(s) => return status_response(&s),
    };
    let gvr = rp.gvr();

    if rp.is_named() {
        match reg.get(&gvr, &rp.namespace, &rp.name) {
            Ok(obj) => HttpResponse::json(200, obj.to_json_string()),
            Err(s) => status_response(&s),
        }
    } else {
        let opts = ListOptions {
            namespace: (!rp.namespace.is_empty()).then(|| rp.namespace.clone()),
            ..ListOptions::default()
        };
        match reg.list(&gvr, &opts) {
            Ok(list) => {
                let kind = gvk::kind_for(&gvr).map_or_else(|| "List".to_string(), |k| format!("{}List", k.kind));
                let body = json::obj([
                    ("apiVersion", Value::from("v1")),
                    ("kind", Value::from(kind.as_str())),
                    (
                        "metadata",
                        json::obj([("resourceVersion", Value::from(list.resource_version.to_string()))]),
                    ),
                    ("items", Value::Array(list.items)),
                ]);
                HttpResponse::json(200, body.to_json_string())
            }
            Err(s) => status_response(&s),
        }
    }
}

/// The K3s-style `/version` payload.
fn version_response() -> HttpResponse {
    let info = BuildInfo::current();
    let body = json::obj([
        ("major", Value::from("1")),
        ("minor", Value::from("0")),
        ("gitVersion", Value::from(format!("v{}+{}", info.version, info.git_sha).as_str())),
        ("platform", Value::from(std::env::consts::OS)),
    ]);
    HttpResponse::json(200, body.to_json_string())
}

/// Prometheus text exposition of the apiserver's stored-object counts — the
/// observability track for the control plane.
fn metrics_response(reg: &Registry) -> HttpResponse {
    use cave_home_apiserver_rs::gvk::GroupVersionResource;
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("# HELP cave_home_apiserver_objects Stored objects by resource.\n");
    out.push_str("# TYPE cave_home_apiserver_objects gauge\n");
    for resource in ["nodes", "pods", "namespaces"] {
        let gvr = GroupVersionResource::new("", "v1", resource);
        let count = reg.list(&gvr, &ListOptions::default()).map_or(0, |l| l.items.len());
        let _ = writeln!(out, "cave_home_apiserver_objects{{resource=\"{resource}\"}} {count}");
    }
    out.push_str("# HELP cave_home_apiserver_resource_version Current global resourceVersion.\n");
    out.push_str("# TYPE cave_home_apiserver_resource_version counter\n");
    let _ = writeln!(out, "cave_home_apiserver_resource_version {}", reg.resource_version());
    HttpResponse::text(200, out)
}

/// Render a `Status` failure object as its HTTP response.
fn status_response(status: &Status) -> HttpResponse {
    let body = json::obj([
        ("apiVersion", Value::from("v1")),
        ("kind", Value::from("Status")),
        ("status", Value::from("Failure")),
        ("reason", Value::from(status.reason.as_str())),
        ("code", Value::from(i64::from(status.code))),
        ("message", Value::from(status.message.as_str())),
    ]);
    HttpResponse::json(status.code, body.to_json_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_apiserver_rs::gvk::GroupVersionResource;
    use crate::node::LocalNode;

    fn get(path: &str) -> HttpRequest {
        HttpRequest::parse(format!("GET {path} HTTP/1.1\r\n\r\n").as_bytes()).expect("parse")
    }

    fn seed_node(reg: &mut Registry, name: &str) {
        let nodes = GroupVersionResource::new("", "v1", "nodes");
        reg.create(&nodes, LocalNode::new(name, "10.0.0.5").to_object()).expect("seed");
    }

    #[test]
    fn healthz_is_ok() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/healthz"));
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"ok");
    }

    #[test]
    fn list_nodes_empty_returns_nodelist() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/api/v1/nodes"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"NodeList\""), "{body}");
        assert!(body.contains("\"items\":[]"), "{body}");
    }

    #[test]
    fn list_nodes_after_seed_includes_it() {
        let mut reg = Registry::new();
        seed_node(&mut reg, "hub-01");
        let resp = handle(&mut reg, &get("/api/v1/nodes"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"NodeList\""), "{body}");
        assert!(body.contains("hub-01"), "{body}");
    }

    #[test]
    fn get_named_node_returns_it() {
        let mut reg = Registry::new();
        seed_node(&mut reg, "hub-01");
        let resp = handle(&mut reg, &get("/api/v1/nodes/hub-01"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"Node\""), "{body}");
        assert!(body.contains("hub-01"), "{body}");
    }

    #[test]
    fn get_missing_node_is_404_status() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/api/v1/nodes/ghost"));
        assert_eq!(resp.status, 404);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"Status\""), "{body}");
        assert!(body.contains("\"reason\":\"NotFound\""), "{body}");
    }

    #[test]
    fn list_pods_empty_returns_podlist() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/api/v1/pods"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"PodList\""), "{body}");
        assert!(body.contains("\"items\":[]"), "{body}");
    }

    #[test]
    fn non_get_on_resource_is_405() {
        let mut reg = Registry::new();
        let req = HttpRequest::parse(b"POST /api/v1/nodes HTTP/1.1\r\n\r\n").unwrap();
        let resp = handle(&mut reg, &req);
        assert_eq!(resp.status, 405);
    }

    #[test]
    fn unknown_path_is_400() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/nonsense"));
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn version_endpoint_reports_build() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/version"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("gitVersion"), "{body}");
    }

    #[test]
    fn metrics_endpoint_reports_object_counts() {
        let mut reg = Registry::new();
        seed_node(&mut reg, "hub-01");
        let resp = handle(&mut reg, &get("/metrics"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("cave_home_apiserver_objects"), "{body}");
        assert!(body.contains("resource=\"nodes\""), "{body}");
    }
}
