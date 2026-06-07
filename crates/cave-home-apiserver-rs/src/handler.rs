// SPDX-License-Identifier: Apache-2.0
//! The apiserver request handler: the chain that turns an [`crate::http::Request`]
//! into an [`crate::http::Response`] by routing it through
//! authentication → authorization → admission → storage, then encoding the
//! result (object, list, or `metav1.Status`) as JSON.
//!
//! Behavioural reference: the Kubernetes API request pipeline
//! (`k8s.io/apiserver` handler chain: `WithAuthentication` →
//! `WithAuthorization` → `WithAdmission` → the REST storage handler) and the API
//! conventions for the `List` and `Status` response kinds. This is a clean-room
//! reimplementation over the in-crate decision core; the socket loop is in
//! [`crate::server`].

use crate::admission::{AdmissionChain, AdmissionRequest, Operation};
use crate::authn::{AnonymousAuthenticator, AuthenticatorChain};
use crate::discovery::{self, ApiGroup, ApiResource, GroupVersionEntry};
use crate::gvk::{self, GroupVersionResource};
use crate::http::{Method, Request, Response};
use crate::json::{self, obj, Value};
use crate::meta;
use crate::path::{self, ResourcePath};
use crate::rbac::{Attributes, Decision, RbacAuthorizer, UserInfo};
use crate::registry::{ListOptions, ListResult, Registry};
use crate::selector::{FieldSelector, LabelSelector};
use crate::status::{Result, Status};

/// The authorization mode applied after authentication. Mirrors the upstream
/// `--authorization-mode` flag values used by k3s.
pub enum Authorization {
    /// Allow every authenticated request (k3s default for the embedded server in
    /// some bootstrap modes).
    AlwaysAllow,
    /// Deny every request.
    AlwaysDeny,
    /// Consult an additive RBAC authorizer; deny when it returns no opinion.
    Rbac(RbacAuthorizer),
}

/// The apiserver: the storage registry plus the authn/authz pipeline. A single
/// [`ApiServer::handle`] call runs the full chain for one request and returns
/// the encoded [`Response`]. Mutation requires `&mut self` because the in-memory
/// registry is the storage backend.
pub struct ApiServer {
    /// The backing storage registry.
    pub registry: Registry,
    /// The authentication chain.
    pub authn: AuthenticatorChain,
    /// The authorization mode.
    pub authz: Authorization,
    /// The admission pipeline (mutating → validating), run before storage writes.
    pub admission: AdmissionChain,
}

impl Default for ApiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiServer {
    /// A server with an empty registry, anonymous-allowed authentication, and
    /// `AlwaysAllow` authorization — the most permissive default, suitable for a
    /// single-node bootstrap. Harden via [`ApiServer::with_authn`] /
    /// [`ApiServer::with_authz`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            authn: AuthenticatorChain::new()
                .with(Box::new(AnonymousAuthenticator))
                .allow_anonymous(true),
            authz: Authorization::AlwaysAllow,
            admission: AdmissionChain::new(),
        }
    }

    /// Replace the authentication chain (builder style).
    #[must_use]
    pub fn with_authn(mut self, authn: AuthenticatorChain) -> Self {
        self.authn = authn;
        self
    }

    /// Replace the authorization mode (builder style).
    #[must_use]
    pub fn with_authz(mut self, authz: Authorization) -> Self {
        self.authz = authz;
        self
    }

    /// Replace the admission pipeline (builder style).
    #[must_use]
    pub fn with_admission(mut self, admission: AdmissionChain) -> Self {
        self.admission = admission;
        self
    }

    /// Run the admission pipeline for one operation, returning the (possibly
    /// mutated) request or the rejecting [`Status`].
    fn admit(
        &self,
        operation: Operation,
        gvr: &GroupVersionResource,
        namespace: &str,
        object: Option<Value>,
        old_object: Option<Value>,
    ) -> Result<AdmissionRequest> {
        self.admission.run(AdmissionRequest {
            gvr: gvr.clone(),
            operation,
            namespace: namespace.to_string(),
            object,
            old_object,
        })
    }

    /// Run the full request pipeline and return the encoded response. Errors at
    /// any stage are rendered as a `metav1.Status` body with the matching HTTP
    /// code, so this method never fails.
    pub fn handle(&mut self, req: &Request) -> Response {
        match self.route(req) {
            Ok(resp) => resp,
            Err(s) => status_response(&s),
        }
    }

    fn route(&mut self, req: &Request) -> Result<Response> {
        // 0. Unauthenticated liveness/health probes (kubelet + load balancers hit
        //    these without credentials).
        if let Some(resp) = health_endpoint(req.path()) {
            return Ok(resp);
        }

        // 1. Authentication.
        let user = self.authn.authenticate(req)?;

        // 1b. Non-resource API-surface endpoints (discovery + version) sit behind
        //     authn but are not RBAC resource requests.
        if let Some(resp) = discovery_endpoint(req.path()) {
            return Ok(resp);
        }

        // 2. Resolve the resource path + verb.
        let rp = path::parse(req.path())?;
        let gvr = rp.gvr();
        let watch = matches!(req.query_get("watch").as_deref(), Some("true" | "1"));
        let verb = resolve_verb(&req.method, &rp, watch)?;

        // 3. Authorization.
        self.authorize(&user, &rp, verb)?;

        // 4. Dispatch to storage.
        self.dispatch(verb, &gvr, &rp, req)
    }

    fn authorize(&self, user: &UserInfo, rp: &ResourcePath, verb: &str) -> Result<()> {
        match &self.authz {
            Authorization::AlwaysAllow => Ok(()),
            Authorization::AlwaysDeny => {
                Err(Status::forbidden(format!("{} is not allowed", user.name)))
            }
            Authorization::Rbac(authorizer) => {
                let mut attrs = Attributes::resource(
                    user.clone(),
                    verb,
                    rp.group.clone(),
                    rp.resource.clone(),
                    rp.namespace.clone(),
                    rp.name.clone(),
                );
                if !rp.subresource.is_empty() {
                    attrs = attrs.with_subresource(rp.subresource.clone());
                }
                if authorizer.authorize(&attrs) == Decision::Allow {
                    Ok(())
                } else {
                    Err(Status::forbidden(format!(
                        "{} cannot {verb} resource \"{}\" in API group \"{}\"",
                        user.name, rp.resource, rp.group
                    )))
                }
            }
        }
    }

    fn dispatch(
        &mut self,
        verb: &str,
        gvr: &GroupVersionResource,
        rp: &ResourcePath,
        req: &Request,
    ) -> Result<Response> {
        match verb {
            "get" => {
                let o = self.registry.get(gvr, &rp.namespace, &rp.name)?;
                Ok(object_response(200, &o))
            }
            "list" => {
                let opts = list_options(rp, req)?;
                let result = self.registry.list(gvr, &opts)?;
                Ok(object_response(200, &list_object(gvr, &result)))
            }
            "create" => {
                let mut body = parse_body(req)?;
                inject_namespace(&mut body, &rp.namespace);
                let admitted = self.admit(Operation::Create, gvr, &rp.namespace, Some(body), None)?;
                let object = admitted
                    .object
                    .ok_or_else(|| Status::invalid("admission removed the object"))?;
                let created = self.registry.create(gvr, object)?;
                Ok(object_response(201, &created))
            }
            "update" => {
                let mut body = parse_body(req)?;
                inject_namespace(&mut body, &rp.namespace);
                if rp.subresource == "status" {
                    let updated = self.registry.update_status(gvr, body)?;
                    return Ok(object_response(200, &updated));
                }
                let old = self.registry.get(gvr, &rp.namespace, &rp.name).ok();
                let admitted = self.admit(Operation::Update, gvr, &rp.namespace, Some(body), old)?;
                let object = admitted
                    .object
                    .ok_or_else(|| Status::invalid("admission removed the object"))?;
                let updated = self.registry.update(gvr, object)?;
                Ok(object_response(200, &updated))
            }
            "patch" => {
                // get → apply patch → admit(Update) → update, so admission and
                // generation/concurrency semantics match a plain update.
                let existing = self.registry.get(gvr, &rp.namespace, &rp.name)?;
                let ctype = req.headers.get("content-type").unwrap_or_default();
                let body = parse_body(req)?;
                let patched = if ctype.starts_with("application/json-patch+json") {
                    let ops = crate::patch::ops_from_json(&body)?;
                    crate::patch::apply_json_patch(&existing, &ops)?
                } else {
                    // merge-patch+json and strategic-merge-patch+json (the latter
                    // approximated by merge) and the default.
                    crate::patch::apply_merge_patch(&existing, &body)
                };
                let admitted =
                    self.admit(Operation::Update, gvr, &rp.namespace, Some(patched), Some(existing))?;
                let object = admitted
                    .object
                    .ok_or_else(|| Status::invalid("admission removed the object"))?;
                let updated = self.registry.update(gvr, object)?;
                Ok(object_response(200, &updated))
            }
            "delete" => {
                let existing = self.registry.get(gvr, &rp.namespace, &rp.name)?;
                self.admit(Operation::Delete, gvr, &rp.namespace, None, Some(existing))?;
                let (object, _removed) = self.registry.delete(gvr, &rp.namespace, &rp.name)?;
                Ok(object_response(200, &object))
            }
            other => Err(Status::new(
                crate::status::StatusReason::MethodNotAllowed,
                format!("verb {other} is not supported"),
            )),
        }
    }
}

/// Resolve the REST verb from the HTTP method + path scope (named vs collection)
/// + the `watch` flag. Returns `MethodNotAllowed` for unsupported combinations.
fn resolve_verb(method: &Method, rp: &ResourcePath, watch: bool) -> Result<&'static str> {
    let named = rp.is_named();
    let verb = match (method, named) {
        (Method::Get, false) if watch => "watch",
        (Method::Get, false) => "list",
        (Method::Get | Method::Head, true) => "get",
        (Method::Post, false) => "create",
        (Method::Put, true) => "update",
        (Method::Patch, true) => "patch",
        (Method::Delete, true) => "delete",
        _ => {
            return Err(Status::new(
                crate::status::StatusReason::MethodNotAllowed,
                format!("{} not allowed on this resource path", method.as_str()),
            ))
        }
    };
    Ok(verb)
}

/// Build [`ListOptions`] from the path namespace + query parameters.
fn list_options(rp: &ResourcePath, req: &Request) -> Result<ListOptions> {
    let mut opts = ListOptions::default();
    if !rp.namespace.is_empty() {
        opts.namespace = Some(rp.namespace.clone());
    }
    if let Some(ls) = req.query_get("labelSelector") {
        opts.label_selector = LabelSelector::parse(&ls)?;
    }
    if let Some(fs) = req.query_get("fieldSelector") {
        opts.field_selector = FieldSelector::parse(&fs)?;
    }
    if let Some(limit) = req.query_get("limit") {
        opts.limit = limit
            .parse()
            .map_err(|_| Status::bad_request("invalid limit parameter"))?;
    }
    if let Some(token) = req.query_get("continue") {
        opts.continue_token = Some(token);
    }
    Ok(opts)
}

/// Parse a request body as JSON, mapping a parse failure to `BadRequest`.
fn parse_body(req: &Request) -> Result<Value> {
    let text = std::str::from_utf8(&req.body)
        .map_err(|_| Status::bad_request("request body is not valid UTF-8"))?;
    json::parse(text).map_err(|e| Status::bad_request(format!("invalid JSON body: {e}")))
}

/// The URL namespace is authoritative: stamp it onto the object's metadata when
/// the path is namespaced (matching upstream, where the URL namespace overrides
/// any body value).
fn inject_namespace(object: &mut Value, namespace: &str) {
    if namespace.is_empty() {
        return;
    }
    let mut m = meta::read_meta(object);
    m.namespace = namespace.to_string();
    meta::write_meta(object, &m);
}

/// Render a [`Status`] failure as a `metav1.Status` object (the body the
/// apiserver returns for every error).
#[must_use]
pub fn status_object(s: &Status) -> Value {
    obj([
        ("kind", Value::from("Status")),
        ("apiVersion", Value::from("v1")),
        ("metadata", Value::object()),
        ("status", Value::from("Failure")),
        ("message", Value::from(s.message.clone())),
        ("reason", Value::from(s.reason.as_str())),
        ("code", Value::from(i64::from(s.code))),
    ])
}

/// A `metav1.Status` success object (returned by `delete`, and by collection
/// operations that produce no object).
#[must_use]
pub fn success_status(message: impl Into<String>) -> Value {
    obj([
        ("kind", Value::from("Status")),
        ("apiVersion", Value::from("v1")),
        ("metadata", Value::object()),
        ("status", Value::from("Success")),
        ("message", Value::from(message.into())),
        ("code", Value::from(200_i64)),
    ])
}

/// Wrap a [`ListResult`] in the `<Kind>List` envelope per the API conventions:
/// `apiVersion`, `kind`, `metadata.resourceVersion` (+ `continue` when more
/// pages remain), and `items`.
#[must_use]
pub fn list_object(gvr: &GroupVersionResource, result: &ListResult) -> Value {
    let kind = gvk::kind_for(gvr).map_or_else(|| "List".to_string(), |k| format!("{}List", k.kind));
    let mut metadata = obj([(
        "resourceVersion",
        Value::from(result.resource_version.to_string()),
    )]);
    if let Some(token) = &result.continue_token {
        metadata.insert("continue", Value::from(token.clone()));
    }
    obj([
        ("kind", Value::from(kind)),
        ("apiVersion", Value::from(gvr.group_version())),
        ("metadata", metadata),
        ("items", Value::Array(result.items.clone())),
    ])
}

/// Encode a JSON body `Value` into an `application/json` [`Response`] with the
/// given status code.
#[must_use]
pub fn object_response(code: u16, body: &Value) -> Response {
    Response::new(code).with_body("application/json", body.to_json_string().into_bytes())
}

/// Encode a [`Status`] failure into a [`Response`] (its HTTP code + JSON body).
#[must_use]
pub fn status_response(s: &Status) -> Response {
    object_response(s.code, &status_object(s))
}

/// Handle the unauthenticated health probes. `Some` iff `path` is a probe.
fn health_endpoint(path: &str) -> Option<Response> {
    match path {
        "/healthz" | "/livez" | "/readyz" => {
            Some(Response::new(200).with_body("text/plain; charset=utf-8", b"ok".to_vec()))
        }
        _ => None,
    }
}

/// Handle the discovery + version surface. Returns `Some` iff `path` is one of
/// the discovery endpoints (`/api`, `/apis`, `/api/{v}`, `/apis/{g}/{v}`) or
/// `/version`; an unknown group/version inside that surface yields a `404`.
fn discovery_endpoint(path: &str) -> Option<Response> {
    if path == "/version" {
        return Some(object_response(200, &version_object()));
    }
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segs.as_slice() {
        ["api"] => Some(object_response(200, &api_versions_object())),
        ["apis"] => Some(object_response(200, &api_group_list_object())),
        ["api", version] => Some(resource_list_response("", version)),
        ["apis", group, version] => Some(resource_list_response(group, version)),
        _ => None,
    }
}

/// `APIVersions` served at `/api`.
fn api_versions_object() -> Value {
    obj([
        ("kind", Value::from("APIVersions")),
        (
            "versions",
            Value::Array(discovery::api_versions().into_iter().map(Value::from).collect()),
        ),
    ])
}

/// `APIGroupList` served at `/apis`.
fn api_group_list_object() -> Value {
    let groups = discovery::api_groups().iter().map(api_group_object).collect();
    obj([
        ("kind", Value::from("APIGroupList")),
        ("apiVersion", Value::from("v1")),
        ("groups", Value::Array(groups)),
    ])
}

fn group_version_entry_object(e: &GroupVersionEntry) -> Value {
    obj([
        ("groupVersion", Value::from(e.group_version.clone())),
        ("version", Value::from(e.version.clone())),
    ])
}

fn api_group_object(g: &ApiGroup) -> Value {
    let versions = g.versions.iter().map(group_version_entry_object).collect();
    obj([
        ("name", Value::from(g.name.clone())),
        ("versions", Value::Array(versions)),
        ("preferredVersion", group_version_entry_object(&g.preferred_version)),
    ])
}

fn api_resource_object(r: &ApiResource) -> Value {
    obj([
        ("name", Value::from(r.name.clone())),
        ("singularName", Value::from(r.singular_name.clone())),
        ("namespaced", Value::from(r.namespaced)),
        ("kind", Value::from(r.kind.clone())),
        (
            "verbs",
            Value::Array(r.verbs.iter().cloned().map(Value::from).collect()),
        ),
    ])
}

/// `APIResourceList` for one group/version, or a `404` if nothing is served
/// there.
fn resource_list_response(group: &str, version: &str) -> Response {
    let resources = discovery::api_resources(group, version);
    if resources.is_empty() {
        return status_response(&Status::not_found(
            "the server could not find the requested resource",
        ));
    }
    let group_version = if group.is_empty() {
        version.to_string()
    } else {
        format!("{group}/{version}")
    };
    let items = resources.iter().map(api_resource_object).collect();
    object_response(
        200,
        &obj([
            ("kind", Value::from("APIResourceList")),
            ("apiVersion", Value::from("v1")),
            ("groupVersion", Value::from(group_version)),
            ("resources", Value::Array(items)),
        ]),
    )
}

/// The `/version` payload (`version.Info`). The minor/gitVersion track the
/// Kubernetes API surface level this transport targets.
fn version_object() -> Value {
    obj([
        ("major", Value::from("1")),
        ("minor", Value::from("29")),
        ("gitVersion", Value::from("v1.29.0-cavehome")),
        ("compiler", Value::from("rustc")),
        ("platform", Value::from("cave-home/orchestration")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gvk::GroupVersionResource;
    use crate::json::{obj, Value};
    use crate::registry::ListResult;
    use crate::status::Status;

    #[test]
    fn status_object_has_failure_shape() {
        let s = Status::not_found("pods \"x\" not found");
        let v = status_object(&s);
        assert_eq!(v.pointer("kind"), Some(&Value::from("Status")));
        assert_eq!(v.pointer("apiVersion"), Some(&Value::from("v1")));
        assert_eq!(v.pointer("status"), Some(&Value::from("Failure")));
        assert_eq!(v.pointer("reason"), Some(&Value::from("NotFound")));
        assert_eq!(v.pointer("code"), Some(&Value::from(404_i64)));
        assert_eq!(v.pointer("message"), Some(&Value::from("pods \"x\" not found")));
    }

    #[test]
    fn success_status_object_shape() {
        let v = success_status("pods \"x\" deleted");
        assert_eq!(v.pointer("kind"), Some(&Value::from("Status")));
        assert_eq!(v.pointer("status"), Some(&Value::from("Success")));
        assert_eq!(v.pointer("code"), Some(&Value::from(200_i64)));
    }

    #[test]
    fn list_object_wraps_items_with_kind_and_rv() {
        let pods = GroupVersionResource::new("", "v1", "pods");
        let result = ListResult {
            items: vec![
                obj([("metadata", obj([("name", Value::from("a"))]))]),
                obj([("metadata", obj([("name", Value::from("b"))]))]),
            ],
            continue_token: None,
            resource_version: 7,
        };
        let v = list_object(&pods, &result);
        assert_eq!(v.pointer("kind"), Some(&Value::from("PodList")));
        assert_eq!(v.pointer("apiVersion"), Some(&Value::from("v1")));
        assert_eq!(v.pointer("metadata.resourceVersion"), Some(&Value::from("7")));
        assert_eq!(v.pointer("items").and_then(Value::as_array).map(<[_]>::len), Some(2));
    }

    #[test]
    fn list_object_grouped_api_version_and_continue() {
        let deploys = GroupVersionResource::new("apps", "v1", "deployments");
        let result = ListResult {
            items: vec![],
            continue_token: Some("2".to_string()),
            resource_version: 9,
        };
        let v = list_object(&deploys, &result);
        assert_eq!(v.pointer("kind"), Some(&Value::from("DeploymentList")));
        assert_eq!(v.pointer("apiVersion"), Some(&Value::from("apps/v1")));
        assert_eq!(v.pointer("metadata.continue"), Some(&Value::from("2")));
    }

    #[test]
    fn status_response_sets_code_and_json_body() {
        let resp = status_response(&Status::conflict("stale"));
        assert_eq!(resp.status, 409);
        assert_eq!(resp.headers.get("content-type"), Some("application/json"));
        let body = String::from_utf8(resp.body.clone()).expect("utf8");
        assert!(body.contains("\"reason\":\"Conflict\""), "got: {body}");
    }

    #[test]
    fn object_response_uses_given_code() {
        let o = obj([("metadata", obj([("name", Value::from("p"))]))]);
        let resp = object_response(201, &o);
        assert_eq!(resp.status, 201);
        assert_eq!(resp.headers.get("content-type"), Some("application/json"));
        let body = String::from_utf8(resp.body.clone()).expect("utf8");
        assert!(body.contains("\"name\":\"p\""));
    }

    // --- REST dispatch (verb resolution + storage) --------------------------

    use crate::http::Request;

    fn req(method: &str, target: &str, ctype: &str, body: &str) -> Request {
        let raw = format!(
            "{method} {target} HTTP/1.1\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        Request::parse(raw.as_bytes()).expect("parse")
    }

    fn body_str(resp: &crate::http::Response) -> String {
        String::from_utf8(resp.body.clone()).expect("utf8")
    }

    fn pod_json(ns: &str, name: &str) -> String {
        format!(r#"{{"apiVersion":"v1","kind":"Pod","metadata":{{"name":"{name}","namespace":"{ns}"}}}}"#)
    }

    #[test]
    fn create_returns_201_with_resource_version() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "nginx")));
        assert_eq!(resp.status, 201);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("metadata.resourceVersion"), Some(&Value::from("1")));
        assert_eq!(v.pointer("metadata.name"), Some(&Value::from("nginx")));
    }

    #[test]
    fn get_existing_returns_200() {
        let mut s = ApiServer::new();
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "nginx")));
        let resp = s.handle(&req("GET", "/api/v1/namespaces/default/pods/nginx", "application/json", ""));
        assert_eq!(resp.status, 200);
        assert!(body_str(&resp).contains("\"name\":\"nginx\""));
    }

    #[test]
    fn get_missing_returns_404_status() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/api/v1/namespaces/default/pods/ghost", "application/json", ""));
        assert_eq!(resp.status, 404);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("kind"), Some(&Value::from("Status")));
        assert_eq!(v.pointer("reason"), Some(&Value::from("NotFound")));
    }

    #[test]
    fn list_returns_podlist_with_items() {
        let mut s = ApiServer::new();
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "a")));
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "b")));
        let resp = s.handle(&req("GET", "/api/v1/namespaces/default/pods", "application/json", ""));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("kind"), Some(&Value::from("PodList")));
        assert_eq!(v.pointer("items").and_then(Value::as_array).map(<[_]>::len), Some(2));
    }

    #[test]
    fn list_honours_label_selector() {
        let mut s = ApiServer::new();
        let labeled = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"web","namespace":"default","labels":{"app":"web"}}}"#;
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", labeled));
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "db")));
        let resp = s.handle(&req("GET", "/api/v1/namespaces/default/pods?labelSelector=app%3Dweb", "application/json", ""));
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("items").and_then(Value::as_array).map(<[_]>::len), Some(1));
    }

    #[test]
    fn update_bumps_resource_version() {
        let mut s = ApiServer::new();
        let created = s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "nginx")));
        let body = body_str(&created);
        let resp = s.handle(&req("PUT", "/api/v1/namespaces/default/pods/nginx", "application/json", &body));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("metadata.resourceVersion"), Some(&Value::from("2")));
    }

    #[test]
    fn update_with_stale_rv_returns_409() {
        let mut s = ApiServer::new();
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "nginx")));
        // Stale rv on the PUT body.
        let stale = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx","namespace":"default","resourceVersion":"999"}}"#;
        let resp = s.handle(&req("PUT", "/api/v1/namespaces/default/pods/nginx", "application/json", stale));
        assert_eq!(resp.status, 409);
    }

    #[test]
    fn merge_patch_updates_field() {
        let mut s = ApiServer::new();
        let with_spec = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx","namespace":"default"},"spec":{"replicas":1}}"#;
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", with_spec));
        let resp = s.handle(&req(
            "PATCH",
            "/api/v1/namespaces/default/pods/nginx",
            "application/merge-patch+json",
            r#"{"spec":{"replicas":5}}"#,
        ));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("spec.replicas"), Some(&Value::from(5_i64)));
    }

    #[test]
    fn json_patch_replaces_field() {
        let mut s = ApiServer::new();
        let with_spec = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx","namespace":"default"},"spec":{"replicas":1}}"#;
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", with_spec));
        let resp = s.handle(&req(
            "PATCH",
            "/api/v1/namespaces/default/pods/nginx",
            "application/json-patch+json",
            r#"[{"op":"replace","path":"/spec/replicas","value":7}]"#,
        ));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("spec.replicas"), Some(&Value::from(7_i64)));
    }

    #[test]
    fn delete_returns_200_and_removes() {
        let mut s = ApiServer::new();
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "nginx")));
        let del = s.handle(&req("DELETE", "/api/v1/namespaces/default/pods/nginx", "application/json", ""));
        assert_eq!(del.status, 200);
        let get = s.handle(&req("GET", "/api/v1/namespaces/default/pods/nginx", "application/json", ""));
        assert_eq!(get.status, 404);
    }

    #[test]
    fn unknown_resource_returns_404() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/apis/example.com/v1/widgets", "application/json", ""));
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn put_on_collection_is_405() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("PUT", "/api/v1/namespaces/default/pods", "application/json", "{}"));
        assert_eq!(resp.status, 405);
    }

    // --- authentication + authorization enforcement ------------------------

    use crate::authn::{AuthenticatorChain, TokenAuthenticator};
    use crate::rbac::{
        ClusterRole, ClusterRoleBinding, PolicyRule, RbacAuthorizer, RoleRef, Subject, UserInfo,
    };

    fn req_auth(method: &str, target: &str, token: &str, body: &str) -> Request {
        let raw = format!(
            "{method} {target} HTTP/1.1\r\nauthorization: Bearer {token}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        Request::parse(raw.as_bytes()).expect("parse")
    }

    fn rbac_server() -> ApiServer {
        let reader = ClusterRole {
            name: "pod-reader".to_string(),
            rules: vec![PolicyRule::resource_rule(&[""], &["pods"], &["get", "list"])],
        };
        let binding = ClusterRoleBinding {
            name: "alice-reader".to_string(),
            subjects: vec![Subject::user("alice")],
            role_ref: RoleRef::cluster_role("pod-reader"),
        };
        let authz = RbacAuthorizer::new()
            .with_cluster_role(reader)
            .with_cluster_role_binding(binding);
        let authn = AuthenticatorChain::new()
            .with(Box::new(
                TokenAuthenticator::new().with_token("alice-token", UserInfo::new("alice")),
            ))
            .allow_anonymous(false);
        ApiServer::new()
            .with_authn(authn)
            .with_authz(Authorization::Rbac(authz))
    }

    #[test]
    fn rbac_allows_permitted_verb() {
        let mut s = rbac_server();
        // Seed a pod directly so the read is authorized but the write isn't tested here.
        s.registry
            .create(
                &GroupVersionResource::new("", "v1", "pods"),
                crate::json::parse(&pod_json("default", "nginx")).expect("json"),
            )
            .expect("seed");
        let resp = s.handle(&req_auth("GET", "/api/v1/namespaces/default/pods/nginx", "alice-token", ""));
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn rbac_denies_unpermitted_verb() {
        let mut s = rbac_server();
        // alice may get/list but not create.
        let resp = s.handle(&req_auth(
            "POST",
            "/api/v1/namespaces/default/pods",
            "alice-token",
            &pod_json("default", "nginx"),
        ));
        assert_eq!(resp.status, 403);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("reason"), Some(&Value::from("Forbidden")));
    }

    #[test]
    fn missing_credentials_is_401_when_anonymous_disabled() {
        let mut s = rbac_server();
        let resp = s.handle(&req("GET", "/api/v1/namespaces/default/pods", "application/json", ""));
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn invalid_token_is_401() {
        let mut s = rbac_server();
        let resp = s.handle(&req_auth("GET", "/api/v1/namespaces/default/pods", "bogus", ""));
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn always_deny_is_403() {
        let mut s = ApiServer::new().with_authz(Authorization::AlwaysDeny);
        let resp = s.handle(&req("GET", "/api/v1/pods", "application/json", ""));
        assert_eq!(resp.status, 403);
    }

    // --- watch streams ------------------------------------------------------

    #[test]
    fn watch_streams_added_events_as_chunked() {
        let mut s = ApiServer::new();
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "a")));
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "b")));
        let resp = s.handle(&req(
            "GET",
            "/api/v1/namespaces/default/pods?watch=true&resourceVersion=0",
            "application/json",
            "",
        ));
        assert_eq!(resp.status, 200);
        assert!(resp.chunked, "watch must be a chunked stream");
        let body = body_str(&resp);
        assert_eq!(body.matches(r#""type":"ADDED""#).count(), 2, "body: {body}");
        // Chunk framing present + the object payload.
        assert!(body.contains(r#""name":"a""#));
        assert!(body.contains(r#""name":"b""#));
    }

    #[test]
    fn watch_since_resource_version_returns_only_newer() {
        let mut s = ApiServer::new();
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "a"))); // rv1
        s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "b"))); // rv2
        let resp = s.handle(&req(
            "GET",
            "/api/v1/namespaces/default/pods?watch=1&resourceVersion=1",
            "application/json",
            "",
        ));
        let body = body_str(&resp);
        assert_eq!(body.matches(r#""type":"ADDED""#).count(), 1, "body: {body}");
        assert!(body.contains(r#""name":"b""#));
        assert!(!body.contains(r#""name":"a""#));
    }

    #[test]
    fn watch_reports_modified_and_deleted() {
        let mut s = ApiServer::new();
        let created = s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "a")));
        let body = body_str(&created);
        s.handle(&req("PUT", "/api/v1/namespaces/default/pods/a", "application/json", &body));
        s.handle(&req("DELETE", "/api/v1/namespaces/default/pods/a", "application/json", ""));
        let resp = s.handle(&req(
            "GET",
            "/api/v1/namespaces/default/pods?watch=true&resourceVersion=0",
            "application/json",
            "",
        ));
        let body = body_str(&resp);
        assert!(body.contains(r#""type":"ADDED""#));
        assert!(body.contains(r#""type":"MODIFIED""#));
        assert!(body.contains(r#""type":"DELETED""#));
    }

    #[test]
    fn watch_is_namespace_scoped() {
        let mut s = ApiServer::new();
        s.handle(&req("POST", "/api/v1/namespaces/ns1/pods", "application/json", &pod_json("ns1", "a")));
        s.handle(&req("POST", "/api/v1/namespaces/ns2/pods", "application/json", &pod_json("ns2", "b")));
        let resp = s.handle(&req(
            "GET",
            "/api/v1/namespaces/ns1/pods?watch=true&resourceVersion=0",
            "application/json",
            "",
        ));
        let body = body_str(&resp);
        assert_eq!(body.matches(r#""type":"ADDED""#).count(), 1, "body: {body}");
        assert!(body.contains(r#""name":"a""#));
        assert!(!body.contains(r#""name":"b""#));
    }

    // --- admission integration ---------------------------------------------

    use crate::admission::{AdmissionChain, AdmissionRequest, MutatingPlugin, Operation, RequireName};

    struct AnnotateOwner;
    impl MutatingPlugin for AnnotateOwner {
        fn name(&self) -> &str {
            "AnnotateOwner"
        }
        fn admit(&self, request: &mut AdmissionRequest) -> crate::status::Result<()> {
            if let Some(object) = request.object.as_mut() {
                let m = object.as_object_mut().expect("object");
                let metadata = m.entry("metadata".to_string()).or_insert_with(Value::object);
                let mm = metadata.as_object_mut().expect("metadata obj");
                let ann = mm.entry("annotations".to_string()).or_insert_with(Value::object);
                ann.insert("owner", Value::from("cave"));
            }
            Ok(())
        }
    }

    struct RejectUpdates;
    impl crate::admission::ValidatingPlugin for RejectUpdates {
        fn name(&self) -> &str {
            "RejectUpdates"
        }
        fn validate(&self, request: &AdmissionRequest) -> crate::status::Result<()> {
            if request.operation == Operation::Update {
                return Err(crate::status::Status::invalid("updates are not permitted"));
            }
            Ok(())
        }
    }

    #[test]
    fn validating_plugin_rejects_nameless_create_422() {
        let chain = AdmissionChain::new().with_validating(Box::new(RequireName));
        let mut s = ApiServer::new().with_admission(chain);
        let nameless = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default"}}"#;
        let resp = s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", nameless));
        assert_eq!(resp.status, 422);
    }

    #[test]
    fn mutating_plugin_applies_before_storage() {
        let chain = AdmissionChain::new().with_mutating(Box::new(AnnotateOwner));
        let mut s = ApiServer::new().with_admission(chain);
        let resp = s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", &pod_json("default", "nginx")));
        assert_eq!(resp.status, 201);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("metadata.annotations.owner"), Some(&Value::from("cave")));
    }

    #[test]
    fn admission_runs_on_update() {
        let chain = AdmissionChain::new().with_validating(Box::new(RejectUpdates));
        let mut s = ApiServer::new().with_admission(chain);
        // Seed directly (bypass admission), then attempt an update through the handler.
        s.registry
            .create(
                &GroupVersionResource::new("", "v1", "pods"),
                crate::json::parse(&pod_json("default", "nginx")).expect("json"),
            )
            .expect("seed");
        let body = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx","namespace":"default","resourceVersion":"1"}}"#;
        let resp = s.handle(&req("PUT", "/api/v1/namespaces/default/pods/nginx", "application/json", body));
        assert_eq!(resp.status, 422);
    }

    // --- non-resource endpoints: health, discovery, version ----------------

    #[test]
    fn health_probes_return_ok() {
        let mut s = ApiServer::new();
        for path in ["/healthz", "/livez", "/readyz"] {
            let resp = s.handle(&req("GET", path, "text/plain", ""));
            assert_eq!(resp.status, 200, "{path}");
            assert_eq!(body_str(&resp), "ok", "{path}");
        }
    }

    #[test]
    fn health_probes_need_no_auth() {
        // Even with anonymous disabled, health is reachable (probes are unauthenticated).
        let authn = AuthenticatorChain::new()
            .with(Box::new(TokenAuthenticator::new().with_token("t", UserInfo::new("u"))))
            .allow_anonymous(false);
        let mut s = ApiServer::new().with_authn(authn);
        let resp = s.handle(&req("GET", "/healthz", "text/plain", ""));
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn api_discovery_lists_core_version() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/api", "application/json", ""));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("kind"), Some(&Value::from("APIVersions")));
        let versions = v.pointer("versions").and_then(Value::as_array).expect("versions");
        assert!(versions.contains(&Value::from("v1")));
    }

    #[test]
    fn apis_discovery_lists_groups() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/apis", "application/json", ""));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("kind"), Some(&Value::from("APIGroupList")));
        let groups = v.pointer("groups").and_then(Value::as_array).expect("groups");
        let names: Vec<&str> = groups.iter().filter_map(|g| g.pointer("name").and_then(Value::as_str)).collect();
        assert!(names.contains(&"apps"));
        assert!(names.contains(&"batch"));
    }

    #[test]
    fn core_resource_list_includes_pods() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/api/v1", "application/json", ""));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("kind"), Some(&Value::from("APIResourceList")));
        assert_eq!(v.pointer("groupVersion"), Some(&Value::from("v1")));
        let resources = v.pointer("resources").and_then(Value::as_array).expect("resources");
        let names: Vec<&str> = resources.iter().filter_map(|r| r.pointer("name").and_then(Value::as_str)).collect();
        assert!(names.contains(&"pods"));
    }

    #[test]
    fn grouped_resource_list_includes_deployments() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/apis/apps/v1", "application/json", ""));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("groupVersion"), Some(&Value::from("apps/v1")));
        let resources = v.pointer("resources").and_then(Value::as_array).expect("resources");
        let names: Vec<&str> = resources.iter().filter_map(|r| r.pointer("name").and_then(Value::as_str)).collect();
        assert!(names.contains(&"deployments"));
    }

    #[test]
    fn unknown_group_version_discovery_is_404() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/apis/nope.example.com/v9", "application/json", ""));
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn version_endpoint_reports_major_minor() {
        let mut s = ApiServer::new();
        let resp = s.handle(&req("GET", "/version", "application/json", ""));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert!(v.pointer("major").is_some());
        assert!(v.pointer("minor").is_some());
        assert!(v.pointer("gitVersion").and_then(Value::as_str).is_some());
    }

    #[test]
    fn update_status_subresource_persists_only_status() {
        let mut s = ApiServer::new();
        let with_spec = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx","namespace":"default"},"spec":{"replicas":1}}"#;
        let created = s.handle(&req("POST", "/api/v1/namespaces/default/pods", "application/json", with_spec));
        let cv = crate::json::parse(&body_str(&created)).expect("json");
        let rv = cv.pointer("metadata.resourceVersion").and_then(Value::as_str).expect("rv");
        // Status write also tries to change spec; spec must be ignored.
        let submit = format!(
            r#"{{"apiVersion":"v1","kind":"Pod","metadata":{{"name":"nginx","namespace":"default","resourceVersion":"{rv}"}},"spec":{{"replicas":99}},"status":{{"phase":"Running"}}}}"#
        );
        let resp = s.handle(&req("PUT", "/api/v1/namespaces/default/pods/nginx/status", "application/json", &submit));
        assert_eq!(resp.status, 200);
        let v = crate::json::parse(&body_str(&resp)).expect("json");
        assert_eq!(v.pointer("spec.replicas"), Some(&Value::from(1_i64)));
        assert_eq!(v.pointer("status.phase"), Some(&Value::from("Running")));
    }
}
