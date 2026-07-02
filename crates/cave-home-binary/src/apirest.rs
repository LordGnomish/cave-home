// SPDX-License-Identifier: Apache-2.0
//! The HTTP ⇄ apiserver glue: maps a parsed [`HttpRequest`] onto the
//! `cave-home-apiserver-rs` [`Registry`] verbs and renders the result.
//!
//! Behavioural reference: the Kubernetes apiserver REST surface
//! (`/api/v1/...`, get/list/create/update/patch/delete). This is the full read
//! *and* write path the unified binary serves — real `kubectl get`, `kubectl
//! apply`, `kubectl delete` hit it. Request bodies are decoded with
//! [`Value::parse`](cave_home_apiserver_rs::json::Value::parse); the API
//! discovery documents (`/api`, `/apis`, `/api/v1`, `/apis/{g}/{v}`) and an
//! `OpenAPI` v3 surface (`/openapi/v3...`) let kubectl map nouns to paths and
//! validate. Health, version and Prometheus `/metrics` round out the surface.
//!
//! [`handle`] is pure (`&mut Registry` in, [`HttpResponse`] out), so the whole
//! routing/serialization contract is unit-testable without a socket.

use cave_home_apiserver_rs::gvk;
use cave_home_apiserver_rs::json::{self, Value};
use cave_home_apiserver_rs::path;
use cave_home_apiserver_rs::registry::{ListOptions, ListResult, Registry};
use cave_home_apiserver_rs::status::{Status, StatusReason};

use crate::http::{HttpRequest, HttpResponse};
use crate::version::BuildInfo;

/// Route and serve one request against the registry.
#[must_use]
pub fn handle(reg: &mut Registry, req: &HttpRequest) -> HttpResponse {
    // Non-resource endpoints (liveness/readiness/version/metrics/discovery).
    match req.path.as_str() {
        "/healthz" | "/readyz" | "/livez" => return HttpResponse::text(200, "ok"),
        "/version" => return version_response(),
        "/metrics" => return metrics_response(reg),
        "/api" => return api_versions_response(),
        "/apis" => return api_groups_response(),
        "/api/v1" => return resource_list_response("", "v1"),
        _ => {}
    }
    // Grouped discovery: GET /apis/{group}/{version} → its APIResourceList.
    if let Some(rest) = req.path.strip_prefix("/apis/") {
        let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        if segs.len() == 2 {
            return resource_list_response(segs[0], segs[1]);
        }
    }
    // OpenAPI v3 discovery: kubectl downloads the root then each group/version's
    // schema document before `apply`, and resolves the object's kind through the
    // x-kubernetes-group-version-kind extension. We serve real (permissive)
    // schemas so the GVK resolves and client-side validation passes; the legacy
    // v2 swagger stays absent.
    if req.path == "/openapi/v3" {
        return openapi_v3_root_response();
    }
    if let Some(rest) = req.path.strip_prefix("/openapi/v3/") {
        return openapi_v3_gv_response(rest);
    }
    if req.path.starts_with("/openapi") {
        return status_response(&Status::not_found("openapi schema is not served"));
    }

    // Everything else is a resource path, dispatched by verb.
    let rp = match path::parse(&req.path) {
        Ok(rp) => rp,
        Err(s) => return status_response(&s),
    };
    let gvr = rp.gvr();

    match req.method.as_str() {
        "GET" => get_or_list(reg, &rp, &gvr, req.header("accept")),
        "POST" => create(reg, &rp, &gvr, req),
        "PUT" => replace(reg, &gvr, req),
        "PATCH" => patch(reg, &rp, &gvr, req),
        "DELETE" => remove(reg, &rp, &gvr),
        other => status_response(&Status::new(
            StatusReason::MethodNotAllowed,
            format!("{other} is not supported on {}", req.path),
        )),
    }
}

/// GET: a single named object, or a (optionally namespace-scoped) collection.
///
/// When the client's `Accept` header requests `as=Table` (what `kubectl get`
/// sends), the result is rendered as a `meta.k8s.io/v1` Table so kubectl prints
/// the kind's native columns; otherwise the raw object / `{Kind}List` is returned.
fn get_or_list(reg: &Registry, rp: &path::ResourcePath, gvr: &gvk::GroupVersionResource, accept: Option<&str>) -> HttpResponse {
    if rp.is_named() && rp.subresource == "log" {
        return pod_log_response(reg, rp, gvr);
    }
    let as_table = crate::table::wants_table(accept);
    if rp.is_named() {
        return match reg.get(gvr, &rp.namespace, &rp.name) {
            Ok(obj) if as_table => {
                HttpResponse::json(200, crate::table::to_table(gvr, &[obj], crate::table::now_epoch()).to_json_string())
            }
            Ok(obj) => HttpResponse::json(200, obj.to_json_string()),
            Err(s) => status_response(&s),
        };
    }
    let opts = ListOptions {
        namespace: (!rp.namespace.is_empty()).then(|| rp.namespace.clone()),
        ..ListOptions::default()
    };
    match reg.list(gvr, &opts) {
        Ok(list) if as_table => {
            HttpResponse::json(200, crate::table::to_table(gvr, &list.items, crate::table::now_epoch()).to_json_string())
        }
        Ok(list) => HttpResponse::json(200, list_envelope(gvr, &list).to_json_string()),
        Err(s) => status_response(&s),
    }
}

/// Wrap a list result in its `{Kind}List` envelope.
fn list_envelope(gvr: &gvk::GroupVersionResource, list: &ListResult) -> Value {
    let kind = gvk::kind_for(gvr).map_or_else(|| "List".to_string(), |k| format!("{}List", k.kind));
    json::obj([
        ("apiVersion", Value::from(api_version_of(gvr).as_str())),
        ("kind", Value::from(kind.as_str())),
        (
            "metadata",
            json::obj([("resourceVersion", Value::from(list.resource_version.to_string()))]),
        ),
        ("items", Value::Array(list.items.clone())),
    ])
}

/// POST: create an object from the request body on a collection path.
fn create(reg: &mut Registry, rp: &path::ResourcePath, gvr: &gvk::GroupVersionResource, req: &HttpRequest) -> HttpResponse {
    if rp.is_named() {
        return status_response(&Status::new(
            StatusReason::MethodNotAllowed,
            "POST is not supported on an individual object path; create posts to the collection",
        ));
    }
    let mut object = match parse_object_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // The path namespace is authoritative for namespaced resources.
    if !rp.namespace.is_empty() {
        inject_namespace(&mut object, &rp.namespace);
    }
    // Stamp a server-side creation time so the Age column is real (the registry,
    // being clock-free, preserves but never sets this).
    stamp_creation_timestamp(&mut object);
    match reg.create(gvr, object) {
        Ok(obj) => HttpResponse::json(201, obj.to_json_string()),
        Err(s) => status_response(&s),
    }
}

/// PUT: replace an existing object with the request body.
fn replace(reg: &mut Registry, gvr: &gvk::GroupVersionResource, req: &HttpRequest) -> HttpResponse {
    let object = match parse_object_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match reg.update(gvr, object) {
        Ok(obj) => HttpResponse::json(200, obj.to_json_string()),
        Err(s) => status_response(&s),
    }
}

/// PATCH: apply a merge patch (RFC 7396) to a named object. Strategic-merge is
/// treated as a merge patch for the flat objects this surface serves; JSON Patch
/// (`application/json-patch+json`) is not yet wired over the wire.
fn patch(reg: &mut Registry, rp: &path::ResourcePath, gvr: &gvk::GroupVersionResource, req: &HttpRequest) -> HttpResponse {
    if !rp.is_named() {
        return status_response(&Status::bad_request("PATCH requires a named object"));
    }
    let ctype = req.header("content-type").unwrap_or("application/merge-patch+json");
    if ctype.contains("json-patch") {
        return status_response(&Status::new(
            StatusReason::MethodNotAllowed,
            "application/json-patch+json is not supported; use a merge patch",
        ));
    }
    let patch_doc = match parse_object_body(req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match reg.patch_merge(gvr, &rp.namespace, &rp.name, &patch_doc) {
        Ok(obj) => HttpResponse::json(200, obj.to_json_string()),
        Err(s) => status_response(&s),
    }
}

/// DELETE: remove a named object, returning the deleted object.
fn remove(reg: &mut Registry, rp: &path::ResourcePath, gvr: &gvk::GroupVersionResource) -> HttpResponse {
    if !rp.is_named() {
        return status_response(&Status::bad_request("DELETE requires a named object"));
    }
    match reg.delete(gvr, &rp.namespace, &rp.name) {
        Ok((obj, _removed)) => HttpResponse::json(200, obj.to_json_string()),
        Err(s) => status_response(&s),
    }
}

/// `GET .../pods/{name}/log` → the container logs. The in-process mock CRI does
/// not capture container stdout, so we emit honest, deterministic synthetic
/// lines describing what the mock runtime ran (one block per container) rather
/// than fabricating application output.
fn pod_log_response(reg: &Registry, rp: &path::ResourcePath, gvr: &gvk::GroupVersionResource) -> HttpResponse {
    use std::fmt::Write as _;
    let pod = match reg.get(gvr, &rp.namespace, &rp.name) {
        Ok(p) => p,
        Err(s) => return status_response(&s),
    };
    let mut out = String::new();
    let containers = pod.pointer("spec.containers").and_then(Value::as_array).unwrap_or(&[]);
    if containers.is_empty() {
        out.push_str("[cave-home mock-cri] pod has no containers\n");
    }
    for c in containers {
        let name = c.get("name").and_then(Value::as_str).unwrap_or("?");
        let image = c.get("image").and_then(Value::as_str).unwrap_or("?");
        let _ = writeln!(out, "[cave-home mock-cri] container {name:?} (image {image}) started");
        let _ = writeln!(
            out,
            "[cave-home mock-cri] the in-process mock runtime does not capture application stdout"
        );
    }
    HttpResponse::text(200, out)
}

/// Parse a request body into a JSON object `Value`, mapping failures onto the
/// HTTP responses a client expects (`400` for bad bytes/JSON, `400` for a
/// non-object top-level value).
fn parse_object_body(req: &HttpRequest) -> Result<Value, HttpResponse> {
    let text = std::str::from_utf8(&req.body)
        .map_err(|_| HttpResponse::json(400, bad_request_body("request body is not valid UTF-8")))?;
    let value = Value::parse(text)
        .map_err(|e| HttpResponse::json(400, bad_request_body(&format!("malformed JSON body: {e}"))))?;
    if value.as_object().is_none() {
        return Err(HttpResponse::json(400, bad_request_body("request body must be a JSON object")));
    }
    Ok(value)
}

/// Render a `BadRequest` Status body (used for transport-level parse failures
/// before a `Registry` verb is ever reached).
fn bad_request_body(message: &str) -> String {
    status_body(&Status::bad_request(message))
}

/// Force `metadata.namespace` to the path-derived namespace, creating the
/// `metadata` object if the body omitted it.
fn inject_namespace(object: &mut Value, namespace: &str) {
    if let Value::Object(map) = object {
        let meta = map.entry("metadata".to_string()).or_insert_with(Value::object);
        if let Value::Object(m) = meta {
            m.insert("namespace".to_string(), Value::from(namespace));
        }
    }
}

/// Set `metadata.creationTimestamp` to now if the body did not carry one.
fn stamp_creation_timestamp(object: &mut Value) {
    if let Value::Object(map) = object {
        let meta = map.entry("metadata".to_string()).or_insert_with(Value::object);
        if let Value::Object(m) = meta {
            m.entry("creationTimestamp".to_string())
                .or_insert_with(|| Value::from(crate::table::now_rfc3339()));
        }
    }
}

/// `GET /api` → the core `APIVersions` document.
fn api_versions_response() -> HttpResponse {
    let body = json::obj([
        ("kind", Value::from("APIVersions")),
        ("versions", Value::Array(vec![Value::from("v1")])),
    ]);
    HttpResponse::json(200, body.to_json_string())
}

/// `GET /apis` → the `APIGroupList` of every non-core group the core serves.
fn api_groups_response() -> HttpResponse {
    let mut groups: Vec<Value> = Vec::new();
    for (group, version) in gvk::registered_group_versions() {
        if group.is_empty() {
            continue; // the core group is discovered via /api, not /apis
        }
        let gv = format!("{group}/{version}");
        let version_entry = json::obj([
            ("groupVersion", Value::from(gv.as_str())),
            ("version", Value::from(version)),
        ]);
        groups.push(json::obj([
            ("name", Value::from(group)),
            ("versions", Value::Array(vec![version_entry.clone()])),
            ("preferredVersion", version_entry),
        ]));
    }
    let body = json::obj([
        ("kind", Value::from("APIGroupList")),
        ("apiVersion", Value::from("v1")),
        ("groups", Value::Array(groups)),
    ]);
    HttpResponse::json(200, body.to_json_string())
}

/// `GET /api/v1` or `GET /apis/{group}/{version}` → the `APIResourceList`
/// `kubectl` reads to map a CLI noun (`pods`) onto its REST path + scope.
fn resource_list_response(group: &str, version: &str) -> HttpResponse {
    let gv = if group.is_empty() { version.to_string() } else { format!("{group}/{version}") };
    let resources: Vec<Value> = gvk::registered_resources()
        .into_iter()
        .filter(|r| r.group == group && r.version == version)
        .map(|r| {
            json::obj([
                ("name", Value::from(r.resource)),
                ("singularName", Value::from(r.kind.to_ascii_lowercase().as_str())),
                ("namespaced", Value::from(r.namespaced)),
                ("kind", Value::from(r.kind)),
                ("verbs", Value::Array(VERBS.iter().map(|v| Value::from(*v)).collect())),
            ])
        })
        .collect();
    if resources.is_empty() {
        return status_response(&Status::not_found(format!("no resources for group/version {gv}")));
    }
    let body = json::obj([
        ("kind", Value::from("APIResourceList")),
        ("apiVersion", Value::from("v1")),
        ("groupVersion", Value::from(gv.as_str())),
        ("resources", Value::Array(resources)),
    ]);
    HttpResponse::json(200, body.to_json_string())
}

/// The verbs every served resource supports (used in discovery).
const VERBS: &[&str] = &["get", "list", "create", "update", "patch", "delete", "watch"];

/// `GET /openapi/v3` → the discovery root mapping each served group/version to
/// its schema document URL (`api/v1`, `apis/{group}/{version}`).
fn openapi_v3_root_response() -> HttpResponse {
    let mut paths = std::collections::BTreeMap::new();
    for (group, version) in gvk::registered_group_versions() {
        let (key, url) = if group.is_empty() {
            (format!("api/{version}"), format!("/openapi/v3/api/{version}"))
        } else {
            (format!("apis/{group}/{version}"), format!("/openapi/v3/apis/{group}/{version}"))
        };
        paths.insert(key, json::obj([("serverRelativeURL", Value::from(url.as_str()))]));
    }
    let body = json::obj([("paths", Value::Object(paths))]);
    HttpResponse::json(200, body.to_json_string())
}

/// `GET /openapi/v3/{api/v1|apis/group/version}` → an `OpenAPI` 3.0 document for
/// that group/version. It carries, per kind:
///
/// * a permissive object schema (we model no field rules, so any object is
///   valid) tagged with `x-kubernetes-group-version-kind`; and
/// * `paths` entries whose `post`/`put`/`patch` operations advertise the
///   `fieldValidation` query parameter and the operation's GVK extension.
///
/// The `paths` section is what kubectl's query-param verifier requires — without
/// it the document is rejected as "Invalid `OpenAPI` V3 document" and `apply`
/// fails. With it, kubectl uses server-side field validation (which this surface
/// accepts) and proceeds to create/patch the object.
fn openapi_v3_gv_response(rest: &str) -> HttpResponse {
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let (group, version) = match segs.as_slice() {
        ["api", v] => ("", *v),
        ["apis", g, v] => (*g, *v),
        _ => return status_response(&Status::not_found("unknown openapi group/version")),
    };
    let resources = gvk::registered_resources();
    let in_gv: Vec<_> = resources.iter().filter(|r| r.group == group && r.version == version).collect();
    if in_gv.is_empty() {
        return status_response(&Status::not_found("no schemas for that group/version"));
    }

    let base = if group.is_empty() { format!("/api/{version}") } else { format!("/apis/{group}/{version}") };
    let mut schemas = std::collections::BTreeMap::new();
    let mut paths = std::collections::BTreeMap::new();
    for r in &in_gv {
        let gvk_tag = json::obj([
            ("group", Value::from(r.group)),
            ("version", Value::from(r.version)),
            ("kind", Value::from(r.kind)),
        ]);
        let dns = if r.group.is_empty() { "core".to_string() } else { r.group.replace('.', "_") };
        let schema_key = format!("cave.home.{dns}.{}.{}", r.version, r.kind);
        schemas.insert(
            schema_key.clone(),
            json::obj([
                ("type", Value::from("object")),
                ("x-kubernetes-group-version-kind", Value::Array(vec![gvk_tag.clone()])),
            ]),
        );

        let op = openapi_operation(&schema_key, &gvk_tag);
        let (collection, item) = if r.namespaced {
            (format!("{base}/namespaces/{{namespace}}/{}", r.resource), format!("{base}/namespaces/{{namespace}}/{}/{{name}}", r.resource))
        } else {
            (format!("{base}/{}", r.resource), format!("{base}/{}/{{name}}", r.resource))
        };
        paths.insert(collection, json::obj([("post", op.clone())]));
        paths.insert(item, json::obj([("put", op.clone()), ("patch", op)]));
    }

    let body = json::obj([
        ("openapi", Value::from("3.0.0")),
        ("info", json::obj([("title", Value::from("cave-home")), ("version", Value::from("v1"))])),
        ("paths", Value::Object(paths)),
        ("components", json::obj([("schemas", Value::Object(schemas))])),
    ]);
    HttpResponse::json(200, body.to_json_string())
}

/// One `OpenAPI` operation advertising the `fieldValidation` query parameter, a
/// JSON request body referencing the kind's schema, and the operation's GVK
/// extension — the shape kubectl's query-param verifier matches against.
fn openapi_operation(schema_key: &str, gvk_tag: &Value) -> Value {
    let field_validation = json::obj([
        ("name", Value::from("fieldValidation")),
        ("in", Value::from("query")),
        ("schema", json::obj([("type", Value::from("string"))])),
    ]);
    let body_schema = json::obj([("$ref", Value::from(format!("#/components/schemas/{schema_key}").as_str()))]);
    let request_body = json::obj([(
        "content",
        json::obj([("application/json", json::obj([("schema", body_schema)]))]),
    )]);
    json::obj([
        ("parameters", Value::Array(vec![field_validation])),
        ("requestBody", request_body),
        ("responses", json::obj([("200", json::obj([("description", Value::from("OK"))]))])),
        ("x-kubernetes-group-version-kind", gvk_tag.clone()),
    ])
}

/// The `apiVersion` string for a GVR (`v1`, or `group/version`).
fn api_version_of(gvr: &gvk::GroupVersionResource) -> String {
    if gvr.group.is_empty() {
        gvr.version.clone()
    } else {
        format!("{}/{}", gvr.group, gvr.version)
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

/// Render a `Status` failure object as its canonical JSON string.
fn status_body(status: &Status) -> String {
    json::obj([
        ("apiVersion", Value::from("v1")),
        ("kind", Value::from("Status")),
        ("status", Value::from("Failure")),
        ("reason", Value::from(status.reason.as_str())),
        ("code", Value::from(i64::from(status.code))),
        ("message", Value::from(status.message.as_str())),
    ])
    .to_json_string()
}

/// Render a `Status` failure object as its HTTP response.
fn status_response(status: &Status) -> HttpResponse {
    HttpResponse::json(status.code, status_body(status))
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

    /// Build a request with a method, path and JSON body.
    fn req_body(method: &str, path: &str, ctype: &str, body: &str) -> HttpRequest {
        let raw = format!(
            "{method} {path} HTTP/1.1\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        HttpRequest::parse(raw.as_bytes()).expect("parse")
    }

    const POD_JSON: &str =
        r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx"},"spec":{"containers":[{"name":"web","image":"nginx:1.27"}]}}"#;

    #[test]
    fn post_creates_a_pod_and_returns_201() {
        let mut reg = Registry::new();
        let resp = handle(
            &mut reg,
            &req_body("POST", "/api/v1/namespaces/default/pods", "application/json", POD_JSON),
        );
        assert_eq!(resp.status, 201, "{}", String::from_utf8_lossy(&resp.body));
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"name\":\"nginx\""), "{body}");
        // namespace from the path was injected, server stamped uid + rv.
        assert!(body.contains("\"namespace\":\"default\""), "{body}");
        assert!(body.contains("\"uid\":\"uid-"), "{body}");
        // and it is now retrievable.
        let got = handle(&mut reg, &get("/api/v1/namespaces/default/pods/nginx"));
        assert_eq!(got.status, 200);
    }

    #[test]
    fn post_duplicate_is_409() {
        let mut reg = Registry::new();
        let make = || req_body("POST", "/api/v1/namespaces/default/pods", "application/json", POD_JSON);
        assert_eq!(handle(&mut reg, &make()).status, 201);
        assert_eq!(handle(&mut reg, &make()).status, 409);
    }

    #[test]
    fn post_malformed_json_is_400() {
        let mut reg = Registry::new();
        let resp = handle(
            &mut reg,
            &req_body("POST", "/api/v1/namespaces/default/pods", "application/json", "{not json"),
        );
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn delete_removes_the_pod() {
        let mut reg = Registry::new();
        let _ = handle(&mut reg, &req_body("POST", "/api/v1/namespaces/default/pods", "application/json", POD_JSON));
        let del = HttpRequest::parse(b"DELETE /api/v1/namespaces/default/pods/nginx HTTP/1.1\r\n\r\n").unwrap();
        let resp = handle(&mut reg, &del);
        assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
        assert_eq!(handle(&mut reg, &get("/api/v1/namespaces/default/pods/nginx")).status, 404);
    }

    #[test]
    fn patch_merge_updates_a_field() {
        let mut reg = Registry::new();
        let _ = handle(&mut reg, &req_body("POST", "/api/v1/namespaces/default/pods", "application/json", POD_JSON));
        let patch = r#"{"metadata":{"labels":{"tier":"frontend"}}}"#;
        let resp = handle(
            &mut reg,
            &req_body(
                "PATCH",
                "/api/v1/namespaces/default/pods/nginx",
                "application/merge-patch+json",
                patch,
            ),
        );
        assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"tier\":\"frontend\""), "{body}");
    }

    #[test]
    fn discovery_api_lists_v1() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/api"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"APIVersions\""), "{body}");
        assert!(body.contains("\"v1\""), "{body}");
    }

    #[test]
    fn discovery_apis_lists_groups() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/apis"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"APIGroupList\""), "{body}");
        assert!(body.contains("apps/v1"), "{body}");
        assert!(body.contains("batch/v1"), "{body}");
    }

    #[test]
    fn discovery_core_resourcelist_maps_pods() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/api/v1"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"kind\":\"APIResourceList\""), "{body}");
        assert!(body.contains("\"groupVersion\":\"v1\""), "{body}");
        assert!(body.contains("\"name\":\"pods\""), "{body}");
        assert!(body.contains("\"kind\":\"Pod\""), "{body}");
        assert!(body.contains("\"namespaced\":true"), "{body}");
    }

    #[test]
    fn discovery_grouped_resourcelist() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/apis/apps/v1"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"groupVersion\":\"apps/v1\""), "{body}");
        assert!(body.contains("\"name\":\"deployments\""), "{body}");
    }

    #[test]
    fn openapi_v3_root_lists_every_group_version() {
        // kubectl downloads the OpenAPI v3 root before `apply`; each served
        // group/version must point at its schema document.
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/openapi/v3"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"api/v1\""), "{body}");
        assert!(body.contains("\"apis/apps/v1\""), "{body}");
        assert!(body.contains("serverRelativeURL"), "{body}");
        assert!(body.contains("/openapi/v3/api/v1"), "{body}");
    }

    #[test]
    fn openapi_v3_gv_doc_tags_each_kind_with_its_gvk() {
        // The per-GV document resolves a kind by its x-kubernetes-group-version-kind
        // extension; the schema itself is permissive (we model no field rules),
        // so client-side validation passes for any well-formed object.
        let mut reg = Registry::new();
        // kubectl appends a ?hash= query — the codec strips it to the bare path.
        let resp = handle(&mut reg, &get("/openapi/v3/api/v1"));
        assert_eq!(resp.status, 200);
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("\"openapi\":\"3.0.0\""), "{body}");
        assert!(body.contains("x-kubernetes-group-version-kind"), "{body}");
        assert!(body.contains("\"kind\":\"Pod\""), "{body}");
        // paths + fieldValidation make the doc valid to kubectl's verifier.
        assert!(body.contains("/api/v1/namespaces/{namespace}/pods"), "{body}");
        assert!(body.contains("fieldValidation"), "{body}");
        // grouped GV resolves too
        let g = handle(&mut reg, &get("/openapi/v3/apis/apps/v1"));
        assert_eq!(g.status, 200);
        assert!(String::from_utf8(g.body).unwrap().contains("\"kind\":\"Deployment\""));
    }

    #[test]
    fn openapi_v2_swagger_stays_absent() {
        let mut reg = Registry::new();
        assert_eq!(handle(&mut reg, &get("/openapi/v2")).status, 404);
    }

    #[test]
    fn pod_log_returns_text_for_an_existing_pod() {
        let mut reg = Registry::new();
        let _ = handle(&mut reg, &req_body("POST", "/api/v1/namespaces/default/pods", "application/json", POD_JSON));
        let resp = handle(&mut reg, &get("/api/v1/namespaces/default/pods/nginx/log"));
        assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
        assert!(resp.headers.iter().any(|(k, v)| k == "Content-Type" && v.starts_with("text/plain")));
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("web"), "{body}");
        assert!(body.contains("mock-cri"), "{body}");
    }

    #[test]
    fn pod_log_for_missing_pod_is_404() {
        let mut reg = Registry::new();
        let resp = handle(&mut reg, &get("/api/v1/namespaces/default/pods/ghost/log"));
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn post_on_named_path_is_405() {
        let mut reg = Registry::new();
        let req = req_body("POST", "/api/v1/namespaces/default/pods/nginx", "application/json", POD_JSON);
        assert_eq!(handle(&mut reg, &req).status, 405);
    }

    #[test]
    fn unsupported_method_on_resource_is_405() {
        let mut reg = Registry::new();
        let req = HttpRequest::parse(b"TRACE /api/v1/nodes HTTP/1.1\r\n\r\n").unwrap();
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
