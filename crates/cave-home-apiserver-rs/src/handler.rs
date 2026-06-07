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

use crate::gvk::{self, GroupVersionResource};
use crate::http::Response;
use crate::json::{obj, Value};
use crate::registry::ListResult;
use crate::status::Status;

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
