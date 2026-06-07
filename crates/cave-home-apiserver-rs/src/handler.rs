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
}
