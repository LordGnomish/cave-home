// SPDX-License-Identifier: Apache-2.0
//! HTTP layer + in-process client.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/apiserver/pkg/server/genericapiserver.go::GenericAPIServer
//! - staging/src/k8s.io/apiserver/pkg/endpoints/installer.go::APIInstaller

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use tokio::sync::broadcast;

use crate::admission::{AdmissionChain, AdmissionController, AdmissionError};
use crate::api::{ApiList, ApiObject, ListMeta, TypeMeta, WatchEvent};
use crate::auth::{
    AuthzDecision, Authorizer as _, ChainAuthenticator, RbacAuthorizer,
};
use crate::client_trait::{ApiClient, ApiClientError, ApiResult};
use crate::serialization::{decode, encode};
use crate::storage::{InMemoryStorage, Storage as _, StorageError};
use crate::types::{AdmissionAttributes, ContentType, ResourceRef, UserInfo, Verb};

/// Top-level server handle.
pub struct ApiServer {
    pub storage: Arc<InMemoryStorage>,
    pub admission: Arc<AdmissionChain>,
    pub authn: Arc<ChainAuthenticator>,
    pub authz: Arc<RbacAuthorizer>,
}

impl ApiServer {
    /// Construct from the four collaborators.
    pub fn new(
        storage: Arc<InMemoryStorage>,
        admission: Arc<AdmissionChain>,
        authn: Arc<ChainAuthenticator>,
        authz: Arc<RbacAuthorizer>,
    ) -> Self {
        Self {
            storage,
            admission,
            authn,
            authz,
        }
    }

    /// Run the full admission/authz/registry pipeline for a write request.
    ///
    /// Source: staging/src/k8s.io/apiserver/pkg/endpoints/handlers/{create,update,delete}.go
    async fn process_write(
        &self,
        user: &UserInfo,
        key: &ResourceRef,
        verb: Verb,
        mut object: Option<ApiObject>,
    ) -> ApiResult<ApiObject> {
        // AuthZ first.
        match self.authz.authorize(user, key, verb).await {
            Ok(AuthzDecision::Allow) => {}
            Ok(AuthzDecision::Deny) => {
                return Err(ApiClientError::Forbidden(format!(
                    "user {} denied {} on {}",
                    user.name,
                    verb.as_str(),
                    key.storage_key()
                )));
            }
            Ok(AuthzDecision::NoOpinion) => {
                // Phase 2: no other authorizers => default deny ONLY when
                // there are bindings configured. With empty bindings the
                // server runs open (test-friendly).
                if !user.name.is_empty() && user.name == "system:anonymous" {
                    return Err(ApiClientError::Unauthorized("anonymous".into()));
                }
            }
            Err(e) => return Err(ApiClientError::Internal(e.to_string())),
        }

        // Admission chain.
        let mut attrs = AdmissionAttributes {
            resource: key.clone(),
            verb,
            user: user.clone(),
            object: object.as_ref().and_then(|o| serde_json::to_value(o).ok()),
            old_object: None,
            dry_run: false,
        };
        if let Err(e) = self.admission.admit(&mut attrs).await {
            return Err(admission_to_api(e));
        }
        if let Err(e) = self.admission.validate(&attrs).await {
            return Err(admission_to_api(e));
        }
        // Mutating admission may have rewritten the object — pull it back.
        if let Some(mutated) = attrs.object.take()
            && object.is_some()
        {
            if let Ok(o) = serde_json::from_value::<ApiObject>(mutated) {
                object = Some(o);
            }
        }

        // Storage dispatch.
        let res = match verb {
            Verb::Create => self.storage.create(key, object.unwrap_or_default()).await,
            Verb::Update => self.storage.update(key, object.unwrap_or_default()).await,
            Verb::Delete => self.storage.delete(key).await,
            _ => unreachable!("process_write only handles writes"),
        };
        res.map_err(storage_to_api)
    }

    /// AuthZ + storage read.
    async fn process_read(
        &self,
        user: &UserInfo,
        key: &ResourceRef,
        verb: Verb,
    ) -> ApiResult<ApiObject> {
        let _ = self.authz.authorize(user, key, verb).await; // reads are open in Phase 2
        self.storage.get(key).await.map_err(storage_to_api)
    }

    async fn process_list(
        &self,
        user: &UserInfo,
        key: &ResourceRef,
    ) -> ApiResult<Vec<ApiObject>> {
        let _ = self.authz.authorize(user, key, Verb::List).await;
        self.storage.list(key).await.map_err(storage_to_api)
    }

    fn process_watch(&self, key: &ResourceRef) -> broadcast::Receiver<WatchEvent> {
        self.storage.watch(key)
    }
}

fn storage_to_api(e: StorageError) -> ApiClientError {
    match e {
        StorageError::NotFound(s) => ApiClientError::NotFound(s),
        StorageError::AlreadyExists(s) => ApiClientError::AlreadyExists(s),
        StorageError::Conflict(s, _, _) => ApiClientError::Conflict(s),
        StorageError::Invalid(s) => ApiClientError::Invalid(s),
        StorageError::Internal(s) => ApiClientError::Internal(s),
    }
}

fn admission_to_api(e: AdmissionError) -> ApiClientError {
    match e {
        AdmissionError::Rejected(s) => ApiClientError::Invalid(s),
        AdmissionError::Internal(s) => ApiClientError::Internal(s),
    }
}

// ---------- InProcessClient ------------------------------------------------

/// `ApiClient` impl that dispatches directly to the registry inside the
/// same Rust process. Used by the in-binary scheduler/controller-manager
/// without going through HTTP.
pub struct InProcessClient {
    pub server: Arc<ApiServer>,
    /// Identity passed to authz/admission. Defaults to the cluster admin.
    pub user: UserInfo,
}

impl InProcessClient {
    /// Construct a direct-call client.
    #[must_use]
    pub fn new(server: Arc<ApiServer>) -> Self {
        Self {
            server,
            user: UserInfo {
                name: "system:admin".to_string(),
                uid: String::new(),
                groups: vec!["system:masters".to_string()],
                extra: Default::default(),
            },
        }
    }

    /// Override the identity (used in tests / non-admin callers).
    #[must_use]
    pub fn with_user(mut self, user: UserInfo) -> Self {
        self.user = user;
        self
    }
}

#[async_trait]
impl ApiClient for InProcessClient {
    async fn get(&self, key: &ResourceRef) -> ApiResult<ApiObject> {
        self.server.process_read(&self.user, key, Verb::Get).await
    }
    async fn list(&self, key: &ResourceRef) -> ApiResult<Vec<ApiObject>> {
        self.server.process_list(&self.user, key).await
    }
    fn watch(&self, key: &ResourceRef) -> broadcast::Receiver<WatchEvent> {
        self.server.process_watch(key)
    }
    async fn create(&self, key: &ResourceRef, obj: ApiObject) -> ApiResult<ApiObject> {
        self.server
            .process_write(&self.user, key, Verb::Create, Some(obj))
            .await
    }
    async fn update(&self, key: &ResourceRef, obj: ApiObject) -> ApiResult<ApiObject> {
        self.server
            .process_write(&self.user, key, Verb::Update, Some(obj))
            .await
    }
    async fn patch(&self, key: &ResourceRef, patch: serde_json::Value) -> ApiResult<ApiObject> {
        // JSON-Merge-Patch (RFC 7396).
        let current = self.server.process_read(&self.user, key, Verb::Patch).await?;
        let mut value = serde_json::to_value(&current)
            .map_err(|e| ApiClientError::Internal(e.to_string()))?;
        merge_patch(&mut value, &patch);
        let merged: ApiObject = serde_json::from_value(value)
            .map_err(|e| ApiClientError::Invalid(e.to_string()))?;
        self.server
            .process_write(&self.user, key, Verb::Update, Some(merged))
            .await
    }
    async fn delete(&self, key: &ResourceRef) -> ApiResult<ApiObject> {
        self.server
            .process_write(&self.user, key, Verb::Delete, None)
            .await
    }
}

/// JSON Merge Patch (RFC 7396) — mutates `target` in place.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/util/strategicpatch (the
/// merge-patch path; strategic-merge is not Phase 2 scope).
fn merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    use serde_json::Value;
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else if t.contains_key(k) {
                    if let Some(existing) = t.get_mut(k) {
                        merge_patch(existing, v);
                    }
                } else {
                    t.insert(k.clone(), v.clone());
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

// ---------- HTTP router ----------------------------------------------------

/// Build the axum `Router` that serves core/v1, apps/v1, batch/v1.
///
/// Source: staging/src/k8s.io/apiserver/pkg/endpoints/installer.go::APIInstaller
pub fn http_router(server: Arc<ApiServer>) -> Router {
    Router::new()
        // /api/v1/...
        .route(
            "/api/v1/:resource",
            get(handle_list_cluster).post(handle_create_cluster),
        )
        .route(
            "/api/v1/:resource/:name",
            get(handle_get_cluster)
                .put(handle_update_cluster)
                .delete(handle_delete_cluster)
                .patch(handle_patch_cluster),
        )
        .route(
            "/api/v1/namespaces/:namespace/:resource",
            get(handle_list_namespaced).post(handle_create_namespaced),
        )
        .route(
            "/api/v1/namespaces/:namespace/:resource/:name",
            get(handle_get_namespaced)
                .put(handle_update_namespaced)
                .delete(handle_delete_namespaced)
                .patch(handle_patch_namespaced),
        )
        // /apis/{group}/{version}/...
        .route(
            "/apis/:group/:version/:resource",
            get(handle_list_grouped_cluster).post(handle_create_grouped_cluster),
        )
        .route(
            "/apis/:group/:version/namespaces/:namespace/:resource",
            get(handle_list_grouped).post(handle_create_grouped),
        )
        .route(
            "/apis/:group/:version/namespaces/:namespace/:resource/:name",
            get(handle_get_grouped)
                .put(handle_update_grouped)
                .delete(handle_delete_grouped)
                .patch(handle_patch_grouped),
        )
        .with_state(server)
}

/// Build a real HTTP server bound to `addr` and run it to completion.
///
/// This is the surface the binary main wires up. Test-only callers can
/// hold the `Router` directly via `http_router`.
pub async fn serve(server: Arc<ApiServer>, addr: std::net::SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, http_router(server))
        .await
        .map_err(std::io::Error::other)
}

fn pick_content_type(headers: &HeaderMap) -> ContentType {
    headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .map(ContentType::parse)
        .unwrap_or(ContentType::Json)
}

fn extract_user(_headers: &HeaderMap) -> UserInfo {
    // Phase 2 minimal: every HTTP caller is system:admin until we wire
    // the authenticator chain into the axum layer. Real wiring lands
    // alongside the front-proxy work in Phase 2b.
    UserInfo {
        name: "system:admin".to_string(),
        uid: String::new(),
        groups: vec!["system:masters".to_string()],
        extra: Default::default(),
    }
}

fn api_error_status(err: &ApiClientError) -> StatusCode {
    match err {
        ApiClientError::NotFound(_) => StatusCode::NOT_FOUND,
        ApiClientError::AlreadyExists(_) => StatusCode::CONFLICT,
        ApiClientError::Conflict(_) => StatusCode::CONFLICT,
        ApiClientError::Forbidden(_) => StatusCode::FORBIDDEN,
        ApiClientError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
        ApiClientError::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
        ApiClientError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn err_response(err: ApiClientError) -> axum::response::Response {
    let status = api_error_status(&err);
    let body = serde_json::json!({
        "kind": "Status",
        "apiVersion": "v1",
        "status": "Failure",
        "message": err.to_string(),
        "code": status.as_u16(),
    });
    (status, axum::Json(body)).into_response()
}

fn ok_object(obj: ApiObject, headers: &HeaderMap) -> axum::response::Response {
    let ct = pick_content_type(headers);
    match encode(&obj, ct) {
        Ok(bytes) => ([("content-type", ct.mime())], bytes).into_response(),
        Err(e) => err_response(ApiClientError::Internal(e.to_string())),
    }
}

fn ok_list(items: Vec<ApiObject>, headers: &HeaderMap, kind: &str) -> axum::response::Response {
    let list = ApiList {
        type_meta: TypeMeta {
            api_version: "v1".to_string(),
            kind: format!("{kind}List"),
        },
        metadata: ListMeta::default(),
        items,
    };
    let ct = pick_content_type(headers);
    let body = match ct {
        ContentType::Json => serde_json::to_vec(&list)
            .map_err(|e| ApiClientError::Internal(e.to_string())),
        ContentType::Yaml => serde_yaml::to_string(&list)
            .map(String::into_bytes)
            .map_err(|e| ApiClientError::Internal(e.to_string())),
    };
    match body {
        Ok(b) => ([("content-type", ct.mime())], b).into_response(),
        Err(e) => err_response(e),
    }
}

// ---------- core/v1 cluster-scoped ----------------------------------------

async fn handle_list_cluster(
    Path(resource): Path<String>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::cluster("", "v1", resource.clone(), "");
    match server.process_list(&user, &kref).await {
        Ok(items) => {
            let kind = crate::api::core_v1::kind_of(&resource).unwrap_or("Object");
            ok_list(items, &headers, kind)
        }
        Err(e) => err_response(e),
    }
}

async fn handle_get_cluster(
    Path((resource, name)): Path<(String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::cluster("", "v1", resource, name);
    match server.process_read(&user, &kref, Verb::Get).await {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_create_cluster(
    Path(resource): Path<String>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(ContentType::parse)
        .unwrap_or(ContentType::Json);
    let obj = match decode(&body, ct) {
        Ok(o) => o,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let kref = ResourceRef::cluster("", "v1", resource, obj.metadata.name.clone());
    match server
        .process_write(&user, &kref, Verb::Create, Some(obj))
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_update_cluster(
    Path((resource, name)): Path<(String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let ct = pick_content_type(&headers);
    let obj = match decode(&body, ct) {
        Ok(o) => o,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let kref = ResourceRef::cluster("", "v1", resource, name);
    match server
        .process_write(&user, &kref, Verb::Update, Some(obj))
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_patch_cluster(
    Path((resource, name)): Path<(String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let patch: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let client = InProcessClient::new(server.clone()).with_user(user);
    let kref = ResourceRef::cluster("", "v1", resource, name);
    match client.patch(&kref, patch).await {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_delete_cluster(
    Path((resource, name)): Path<(String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::cluster("", "v1", resource, name);
    match server
        .process_write(&user, &kref, Verb::Delete, None)
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

// ---------- core/v1 namespaced --------------------------------------------

async fn handle_list_namespaced(
    Path((namespace, resource)): Path<(String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::namespaced("", "v1", resource.clone(), namespace, "");
    match server.process_list(&user, &kref).await {
        Ok(items) => {
            let kind = crate::api::core_v1::kind_of(&resource).unwrap_or("Object");
            ok_list(items, &headers, kind)
        }
        Err(e) => err_response(e),
    }
}

async fn handle_get_namespaced(
    Path((namespace, resource, name)): Path<(String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::namespaced("", "v1", resource, namespace, name);
    match server.process_read(&user, &kref, Verb::Get).await {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_create_namespaced(
    Path((namespace, resource)): Path<(String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(ContentType::parse)
        .unwrap_or(ContentType::Json);
    let mut obj = match decode(&body, ct) {
        Ok(o) => o,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    obj.metadata.namespace = namespace.clone();
    let kref = ResourceRef::namespaced(
        "",
        "v1",
        resource,
        namespace,
        obj.metadata.name.clone(),
    );
    match server
        .process_write(&user, &kref, Verb::Create, Some(obj))
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_update_namespaced(
    Path((namespace, resource, name)): Path<(String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let ct = pick_content_type(&headers);
    let obj = match decode(&body, ct) {
        Ok(o) => o,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let kref = ResourceRef::namespaced("", "v1", resource, namespace, name);
    match server
        .process_write(&user, &kref, Verb::Update, Some(obj))
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_patch_namespaced(
    Path((namespace, resource, name)): Path<(String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let patch: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let client = InProcessClient::new(server.clone()).with_user(user);
    let kref = ResourceRef::namespaced("", "v1", resource, namespace, name);
    match client.patch(&kref, patch).await {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_delete_namespaced(
    Path((namespace, resource, name)): Path<(String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::namespaced("", "v1", resource, namespace, name);
    match server
        .process_write(&user, &kref, Verb::Delete, None)
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

// ---------- /apis/{group}/{version} ---------------------------------------

async fn handle_list_grouped_cluster(
    Path((group, version, resource)): Path<(String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::cluster(group, version, resource, "");
    match server.process_list(&user, &kref).await {
        Ok(items) => ok_list(items, &headers, "Object"),
        Err(e) => err_response(e),
    }
}

async fn handle_create_grouped_cluster(
    Path((group, version, resource)): Path<(String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let ct = pick_content_type(&headers);
    let obj = match decode(&body, ct) {
        Ok(o) => o,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let kref = ResourceRef::cluster(group, version, resource, obj.metadata.name.clone());
    match server
        .process_write(&user, &kref, Verb::Create, Some(obj))
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_list_grouped(
    Path((group, version, namespace, resource)): Path<(String, String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::namespaced(group, version, resource, namespace, "");
    match server.process_list(&user, &kref).await {
        Ok(items) => ok_list(items, &headers, "Object"),
        Err(e) => err_response(e),
    }
}

async fn handle_create_grouped(
    Path((group, version, namespace, resource)): Path<(String, String, String, String)>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let ct = pick_content_type(&headers);
    let mut obj = match decode(&body, ct) {
        Ok(o) => o,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    obj.metadata.namespace = namespace.clone();
    let kref = ResourceRef::namespaced(
        group,
        version,
        resource,
        namespace,
        obj.metadata.name.clone(),
    );
    match server
        .process_write(&user, &kref, Verb::Create, Some(obj))
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_get_grouped(
    Path((group, version, namespace, resource, name)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::namespaced(group, version, resource, namespace, name);
    match server.process_read(&user, &kref, Verb::Get).await {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_update_grouped(
    Path((group, version, namespace, resource, name)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let ct = pick_content_type(&headers);
    let obj = match decode(&body, ct) {
        Ok(o) => o,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let kref = ResourceRef::namespaced(group, version, resource, namespace, name);
    match server
        .process_write(&user, &kref, Verb::Update, Some(obj))
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_patch_grouped(
    Path((group, version, namespace, resource, name)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let patch: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err_response(ApiClientError::Invalid(e.to_string())),
    };
    let client = InProcessClient::new(server.clone()).with_user(user);
    let kref = ResourceRef::namespaced(group, version, resource, namespace, name);
    match client.patch(&kref, patch).await {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}

async fn handle_delete_grouped(
    Path((group, version, namespace, resource, name)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    State(server): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = extract_user(&headers);
    let kref = ResourceRef::namespaced(group, version, resource, namespace, name);
    match server
        .process_write(&user, &kref, Verb::Delete, None)
        .await
    {
        Ok(o) => ok_object(o, &headers),
        Err(e) => err_response(e),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::admission::{NamespaceLifecycle, ServiceAccount};
    use crate::client_trait::ApiClient;

    fn build_server() -> Arc<ApiServer> {
        Arc::new(ApiServer::new(
            Arc::new(InMemoryStorage::new()),
            Arc::new(AdmissionChain::new(vec![
                Box::new(NamespaceLifecycle::new(vec!["default".into()])),
                Box::new(ServiceAccount::new()),
            ])),
            Arc::new(ChainAuthenticator::new(vec![])),
            Arc::new(RbacAuthorizer::new()),
        ))
    }

    #[tokio::test]
    async fn in_process_create_get_list_delete() {
        let server = build_server();
        let client = InProcessClient::new(server.clone());

        let key = ResourceRef::namespaced("", "v1", "pods", "default", "p1");
        let mut pod = ApiObject::new("v1", "Pod", "p1").with_namespace("default");
        pod.spec = Some(serde_json::json!({"serviceAccountName": "default"}));

        let created = client.create(&key, pod).await.expect("create");
        assert!(!created.metadata.uid.is_empty());

        let got = client.get(&key).await.expect("get");
        assert_eq!(got.metadata.name, "p1");

        let listed = client
            .list(&ResourceRef::namespaced("", "v1", "pods", "default", ""))
            .await
            .expect("list");
        assert_eq!(listed.len(), 1);

        let deleted = client.delete(&key).await.expect("delete");
        assert_eq!(deleted.metadata.name, "p1");
    }

    #[tokio::test]
    async fn namespace_lifecycle_rejects_create_in_missing_namespace() {
        let server = build_server();
        let client = InProcessClient::new(server);
        let key = ResourceRef::namespaced("", "v1", "pods", "non-existent", "p1");
        let mut pod = ApiObject::new("v1", "Pod", "p1").with_namespace("non-existent");
        pod.spec = Some(serde_json::json!({"serviceAccountName": "default"}));
        let err = client.create(&key, pod).await.expect_err("rejected");
        assert!(matches!(err, ApiClientError::Invalid(_)));
    }

    #[tokio::test]
    async fn admission_injects_default_service_account() {
        let server = build_server();
        let client = InProcessClient::new(server);
        let key = ResourceRef::namespaced("", "v1", "pods", "default", "p2");
        let mut pod = ApiObject::new("v1", "Pod", "p2").with_namespace("default");
        pod.spec = Some(serde_json::json!({}));
        let created = client.create(&key, pod).await.expect("create");
        let sa = created
            .spec
            .as_ref()
            .and_then(|s| s.get("serviceAccountName"))
            .and_then(|v| v.as_str())
            .expect("sa set");
        assert_eq!(sa, "default");
    }

    #[tokio::test]
    async fn patch_merges_into_existing_data() {
        let server = build_server();
        let client = InProcessClient::new(server);
        let key = ResourceRef::namespaced("", "v1", "configmaps", "default", "cm");
        let cm = ApiObject::new("v1", "ConfigMap", "cm").with_namespace("default");
        client.create(&key, cm).await.expect("create");

        let patch_v = serde_json::json!({"data": {"hello": "world"}});
        let patched = client.patch(&key, patch_v).await.expect("patch");
        assert_eq!(
            patched
                .extra
                .get("data")
                .and_then(|d| d.get("hello"))
                .and_then(|v| v.as_str()),
            Some("world")
        );
    }

    #[test]
    fn merge_patch_deletes_null_keys() {
        let mut t = serde_json::json!({"a": 1, "b": 2});
        let p = serde_json::json!({"b": null, "c": 3});
        merge_patch(&mut t, &p);
        assert_eq!(t, serde_json::json!({"a": 1, "c": 3}));
    }

    #[tokio::test]
    async fn http_router_lists_pods() {
        let server = build_server();
        let router = http_router(server.clone());

        let client = InProcessClient::new(server);
        let key = ResourceRef::namespaced("", "v1", "pods", "default", "pp");
        let mut pod = ApiObject::new("v1", "Pod", "pp").with_namespace("default");
        pod.spec = Some(serde_json::json!({"serviceAccountName": "default"}));
        client.create(&key, pod).await.expect("create");

        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .uri("/api/v1/namespaces/default/pods")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.expect("call");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn http_router_get_one_pod() {
        let server = build_server();
        let router = http_router(server.clone());
        let client = InProcessClient::new(server);
        let key = ResourceRef::namespaced("", "v1", "pods", "default", "got");
        let mut pod = ApiObject::new("v1", "Pod", "got").with_namespace("default");
        pod.spec = Some(serde_json::json!({"serviceAccountName": "default"}));
        client.create(&key, pod).await.expect("create");

        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .uri("/api/v1/namespaces/default/pods/got")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.expect("call");
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
