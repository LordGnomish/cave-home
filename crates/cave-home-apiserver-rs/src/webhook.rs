// SPDX-License-Identifier: Apache-2.0
//! Dynamic admission webhooks: the out-of-process mutating/validating plugins
//! that the static [`crate::admission`] chain deferred.
//!
//! Behavioural reference: the `admission.k8s.io/v1` `AdmissionReview`
//! request/response contract and the documented webhook semantics
//! (`MutatingWebhookConfiguration` / `ValidatingWebhookConfiguration`): the
//! apiserver POSTs an `AdmissionReview` carrying the operation + object to the
//! webhook endpoint; the webhook answers with `response.allowed` (+ an optional
//! `status` on denial) and, for mutating webhooks, a base64-encoded JSONPatch in
//! `response.patch`. The `failurePolicy` (`Fail` / `Ignore`) decides what a
//! transport error means.
//!
//! The HTTP call itself is a **seam** ([`WebhookClient`]) so the admission
//! plugins stay testable without a network: [`MockWebhookClient`] drives the
//! unit tests, and [`HttpWebhookClient`] is a std-only `http://` POST client
//! for real endpoints. `https://` webhooks plug into the same trait via a
//! TLS-backed client (the `tls` feature provides the stream type).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::admission::{AdmissionRequest, MutatingPlugin, Operation, ValidatingPlugin};
use crate::json::{self, obj, Value};
use crate::patch::{self, PatchOp};
use crate::status::{Result, Status, StatusReason};

/// The `admission.k8s.io/v1` API version string both directions carry.
const ADMISSION_API_VERSION: &str = "admission.k8s.io/v1";

// ---------------------------------------------------------------------------
// Transport seam.
// ---------------------------------------------------------------------------

/// A webhook transport failure (connect/IO/non-2xx). Distinct from a [`Status`]
/// rejection so the caller can apply the `failurePolicy`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookError {
    /// Human-readable cause.
    pub message: String,
}

impl WebhookError {
    /// Wrap a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for WebhookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "webhook transport error: {}", self.message)
    }
}

impl std::error::Error for WebhookError {}

/// The HTTP transport a webhook plugin uses to reach its endpoint. POST `body`
/// (a serialized `AdmissionReview`) to `url` and return the raw response body.
pub trait WebhookClient: Send + Sync {
    /// POST `body` to `url`, returning the response body bytes.
    ///
    /// # Errors
    /// A [`WebhookError`] for any connect/transport/non-2xx failure.
    fn post(&self, url: &str, body: &[u8]) -> std::result::Result<Vec<u8>, WebhookError>;
}

/// How a transport failure is treated, mirroring the webhook `failurePolicy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailurePolicy {
    /// A transport failure rejects the request (`500`). The safe default.
    Fail,
    /// A transport failure is ignored — the request proceeds as if allowed.
    Ignore,
}

// ---------------------------------------------------------------------------
// Webhook rule matching (which requests a webhook is invoked for).
// ---------------------------------------------------------------------------

/// One `RuleWithOperations` entry from a `*WebhookConfiguration`.
///
/// The set of `(operations, apiGroups, apiVersions, resources)` a webhook is
/// interested in. Each axis matches `"*"` (any) or an exact value; `operations`
/// additionally understands the wire tokens `CREATE`/`UPDATE`/`DELETE`.
/// Resource entries support the `resource/subresource` and `*/subresource`
/// forms, matching the documented webhook rule semantics (subresource matching
/// is folded into the `resources` axis as it is upstream).
#[derive(Clone, Debug)]
pub struct RuleWithOperations {
    /// Operations matched (`CREATE`/`UPDATE`/`DELETE`, or `*`).
    pub operations: Vec<String>,
    /// API groups matched (`*`, or e.g. `""`/`apps`).
    pub api_groups: Vec<String>,
    /// API versions matched (`*`, or e.g. `v1`).
    pub api_versions: Vec<String>,
    /// Resources matched (`*`, `pods`, `pods/status`, `*/status`).
    pub resources: Vec<String>,
}

impl RuleWithOperations {
    /// Build a rule from its four axes.
    #[must_use]
    pub fn new(operations: &[&str], api_groups: &[&str], api_versions: &[&str], resources: &[&str]) -> Self {
        let to_vec = |s: &[&str]| s.iter().map(|x| (*x).to_string()).collect();
        Self {
            operations: to_vec(operations),
            api_groups: to_vec(api_groups),
            api_versions: to_vec(api_versions),
            resources: to_vec(resources),
        }
    }

    /// Whether this rule selects `request`.
    #[must_use]
    pub fn matches(&self, request: &AdmissionRequest) -> bool {
        let op = operation_token(request.operation);
        let op_ok = self.operations.iter().any(|o| o == "*" || o == op);
        let group_ok = self.api_groups.iter().any(|g| g == "*" || g == &request.gvr.group);
        let version_ok = self.api_versions.iter().any(|v| v == "*" || v == &request.gvr.version);
        let resource_ok = self
            .resources
            .iter()
            .any(|r| resource_axis_matches(r, &request.gvr.resource));
        op_ok && group_ok && version_ok && resource_ok
    }
}

/// Match one `resources`-axis entry against a request's bare resource. Handles
/// `*`, an exact resource, and the `resource/subresource` / `*/subresource`
/// forms. The decision core models the bare resource only, so a `*/sub` or
/// `res/sub` entry matches the resource part (the subresource gate is applied
/// when subresource attributes are present; absent them, the resource part is
/// authoritative).
fn resource_axis_matches(rule: &str, resource: &str) -> bool {
    if rule == "*" || rule == resource {
        return true;
    }
    match rule.split_once('/') {
        Some(("*", _sub)) => true,
        Some((res, _sub)) => res == resource,
        None => false,
    }
}

/// The ordered set of [`RuleWithOperations`] attached to a webhook. A request is
/// selected iff **any** rule matches (the union, as upstream). An empty set
/// selects nothing.
#[derive(Clone, Debug, Default)]
pub struct WebhookRules {
    /// The rules; a request matches if any one matches.
    pub rules: Vec<RuleWithOperations>,
}

impl WebhookRules {
    /// Build a rule set.
    #[must_use]
    pub const fn new(rules: Vec<RuleWithOperations>) -> Self {
        Self { rules }
    }

    /// Whether any rule selects `request`.
    #[must_use]
    pub fn matches(&self, request: &AdmissionRequest) -> bool {
        self.rules.iter().any(|r| r.matches(request))
    }
}

// ---------------------------------------------------------------------------
// AdmissionReview codec.
// ---------------------------------------------------------------------------

/// The wire token for an [`Operation`].
fn operation_token(op: Operation) -> &'static str {
    match op {
        Operation::Create => "CREATE",
        Operation::Update => "UPDATE",
        Operation::Delete => "DELETE",
    }
}

/// Split an `apiVersion` (`group/version` or bare `version`) into `(group, version)`.
fn split_api_version(api_version: &str) -> (String, String) {
    match api_version.split_once('/') {
        Some((g, v)) => (g.to_string(), v.to_string()),
        None => (String::new(), api_version.to_string()),
    }
}

/// Build the `kind` GVK block from the object (or old object) under admission,
/// falling back to the request's GVR for the version/group when the object
/// carries no `apiVersion`/`kind`.
fn kind_value(request: &AdmissionRequest) -> Value {
    let source = request.object.as_ref().or(request.old_object.as_ref());
    let kind = source
        .and_then(|o| o.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let (group, version) = source
        .and_then(|o| o.get("apiVersion"))
        .and_then(Value::as_str)
        .map(split_api_version)
        .unwrap_or((request.gvr.group.clone(), request.gvr.version.clone()));
    obj([
        ("group", Value::from(group)),
        ("version", Value::from(version)),
        ("kind", Value::from(kind)),
    ])
}

/// Serialize an [`AdmissionRequest`] as an `admission.k8s.io/v1` `AdmissionReview`
/// request carrying the supplied correlation `uid`.
#[must_use]
pub fn build_admission_review(uid: &str, request: &AdmissionRequest) -> Value {
    let resource = obj([
        ("group", Value::from(request.gvr.group.clone())),
        ("version", Value::from(request.gvr.version.clone())),
        ("resource", Value::from(request.gvr.resource.clone())),
    ]);
    let review_request = obj([
        ("uid", Value::from(uid)),
        ("kind", kind_value(request)),
        ("resource", resource),
        ("namespace", Value::from(request.namespace.clone())),
        ("operation", Value::from(operation_token(request.operation))),
        ("object", request.object.clone().unwrap_or(Value::Null)),
        ("oldObject", request.old_object.clone().unwrap_or(Value::Null)),
    ]);
    obj([
        ("apiVersion", Value::from(ADMISSION_API_VERSION)),
        ("kind", Value::from("AdmissionReview")),
        ("request", review_request),
    ])
}

/// The parsed `response` half of an `AdmissionReview`.
#[derive(Clone, Debug)]
pub struct AdmissionResponse {
    /// Whether the webhook allowed the request.
    pub allowed: bool,
    /// The denial `status.code`, if the webhook supplied one.
    pub code: Option<u16>,
    /// The denial `status.message`, if any.
    pub message: Option<String>,
    /// Decoded JSONPatch operations from a mutating webhook (`patchType:
    /// JSONPatch`), if any.
    pub json_patch: Option<Vec<PatchOp>>,
}

impl AdmissionResponse {
    /// A bare "allowed, no mutation" response (used by `Ignore` failure policy).
    #[must_use]
    pub fn allow() -> Self {
        Self { allowed: true, code: None, message: None, json_patch: None }
    }

    /// Render a denial into the [`Status`] the admission chain returns. The
    /// webhook's `status.code` selects the reason; absent that, an admission
    /// denial defaults to `Forbidden` (403).
    #[must_use]
    pub fn into_status(self) -> Status {
        let reason = self.code.map(reason_from_code).unwrap_or(StatusReason::Forbidden);
        let message = self
            .message
            .unwrap_or_else(|| "admission webhook denied the request".to_string());
        Status::new(reason, message)
    }
}

/// Map an HTTP status code back to the closest [`StatusReason`].
fn reason_from_code(code: u16) -> StatusReason {
    match code {
        400 => StatusReason::BadRequest,
        401 => StatusReason::Unauthorized,
        403 => StatusReason::Forbidden,
        404 => StatusReason::NotFound,
        405 => StatusReason::MethodNotAllowed,
        409 => StatusReason::Conflict,
        422 => StatusReason::Invalid,
        _ => StatusReason::InternalError,
    }
}

/// Parse an `AdmissionReview` response body, validating the echoed `uid`.
///
/// # Errors
/// [`StatusReason::InternalError`] if the body is not parseable JSON, lacks a
/// `response`, or echoes a `uid` that does not match `expected_uid` (a webhook
/// protocol violation), or if a returned JSONPatch is malformed.
pub fn parse_admission_response(body: &[u8], expected_uid: &str) -> Result<AdmissionResponse> {
    let text = std::str::from_utf8(body)
        .map_err(|_| Status::new(StatusReason::InternalError, "webhook response was not UTF-8"))?;
    let root = json::parse(text).map_err(|e| {
        Status::new(StatusReason::InternalError, format!("webhook response is not JSON: {e}"))
    })?;
    let response = root.get("response").ok_or_else(|| {
        Status::new(StatusReason::InternalError, "webhook response has no 'response' field")
    })?;

    if let Some(uid) = response.get("uid").and_then(Value::as_str) {
        if uid != expected_uid {
            return Err(Status::new(
                StatusReason::InternalError,
                format!("webhook response uid {uid:?} does not match request {expected_uid:?}"),
            ));
        }
    }

    let allowed = response.get("allowed").and_then(Value::as_bool).unwrap_or(false);
    let status = response.get("status");
    let code = status
        .and_then(|s| s.get("code"))
        .and_then(|c| match c {
            Value::Number(n) => Some(*n as u16),
            _ => None,
        });
    let message = status
        .and_then(|s| s.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let json_patch = decode_patch(response)?;

    Ok(AdmissionResponse { allowed, code, message, json_patch })
}

/// Decode a mutating webhook's `patch` (base64 JSONPatch) when `patchType` is
/// `JSONPatch`. Other patch types are ignored (no mutation applied).
fn decode_patch(response: &Value) -> Result<Option<Vec<PatchOp>>> {
    let patch_type = response.get("patchType").and_then(Value::as_str);
    if patch_type != Some("JSONPatch") {
        return Ok(None);
    }
    let Some(encoded) = response.get("patch").and_then(Value::as_str) else {
        return Ok(None);
    };
    let raw = b64_decode(encoded).ok_or_else(|| {
        Status::new(StatusReason::InternalError, "webhook patch is not valid base64")
    })?;
    let text = std::str::from_utf8(&raw).map_err(|_| {
        Status::new(StatusReason::InternalError, "webhook patch is not UTF-8")
    })?;
    let value = json::parse(text).map_err(|e| {
        Status::new(StatusReason::InternalError, format!("webhook patch is not JSON: {e}"))
    })?;
    Ok(Some(patch::ops_from_json(&value)?))
}

// ---------------------------------------------------------------------------
// The plugins.
// ---------------------------------------------------------------------------

/// Shared webhook invocation state: endpoint, transport, failure policy, and a
/// monotonic uid counter.
struct WebhookConfig {
    name: String,
    url: String,
    client: Arc<dyn WebhookClient>,
    failure_policy: FailurePolicy,
    /// Which requests the webhook is invoked for. `None` means "every request"
    /// (the unconfigured default); `Some(rules)` gates invocation on a match.
    rules: Option<WebhookRules>,
    counter: AtomicU64,
}

impl WebhookConfig {
    fn new(name: impl Into<String>, url: impl Into<String>, client: Arc<dyn WebhookClient>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            client,
            failure_policy: FailurePolicy::Fail,
            rules: None,
            counter: AtomicU64::new(0),
        }
    }

    fn next_uid(&self) -> String {
        format!("{}-{}", self.name, self.counter.fetch_add(1, Ordering::Relaxed))
    }

    /// Whether the webhook's rules select `request` (always true when no rules
    /// are configured).
    fn selects(&self, request: &AdmissionRequest) -> bool {
        self.rules.as_ref().is_none_or(|r| r.matches(request))
    }

    /// POST the review and parse the response, applying the failure policy to a
    /// transport error.
    fn invoke(&self, request: &AdmissionRequest) -> Result<AdmissionResponse> {
        let uid = self.next_uid();
        let body = build_admission_review(&uid, request).to_json_string().into_bytes();
        match self.client.post(&self.url, &body) {
            Ok(bytes) => parse_admission_response(&bytes, &uid),
            Err(e) => match self.failure_policy {
                FailurePolicy::Ignore => Ok(AdmissionResponse::allow()),
                FailurePolicy::Fail => Err(Status::new(
                    StatusReason::InternalError,
                    format!("failed calling webhook {:?}: {e}", self.name),
                )),
            },
        }
    }
}

/// A validating admission webhook: it may only accept or reject.
pub struct WebhookValidatingPlugin {
    config: WebhookConfig,
}

impl WebhookValidatingPlugin {
    /// Build a validating webhook named `name` posting to `url` over `client`
    /// (failure policy defaults to [`FailurePolicy::Fail`]).
    #[must_use]
    pub fn new(name: impl Into<String>, url: impl Into<String>, client: Arc<dyn WebhookClient>) -> Self {
        Self { config: WebhookConfig::new(name, url, client) }
    }

    /// Set the failure policy (builder style).
    #[must_use]
    pub fn with_failure_policy(mut self, policy: FailurePolicy) -> Self {
        self.config.failure_policy = policy;
        self
    }

    /// Restrict the webhook to the requests its [`WebhookRules`] select
    /// (builder style). Without this, the webhook fires on every request.
    #[must_use]
    pub fn with_rules(mut self, rules: WebhookRules) -> Self {
        self.config.rules = Some(rules);
        self
    }
}

impl ValidatingPlugin for WebhookValidatingPlugin {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn validate(&self, request: &AdmissionRequest) -> Result<()> {
        // A request the webhook's rules do not select is allowed without a call.
        if !self.config.selects(request) {
            return Ok(());
        }
        let response = self.config.invoke(request)?;
        if response.allowed {
            Ok(())
        } else {
            Err(response.into_status())
        }
    }
}

/// A mutating admission webhook: it may reject, or accept and return a JSONPatch
/// that is applied to the object in place.
pub struct WebhookMutatingPlugin {
    config: WebhookConfig,
}

impl WebhookMutatingPlugin {
    /// Build a mutating webhook named `name` posting to `url` over `client`
    /// (failure policy defaults to [`FailurePolicy::Fail`]).
    #[must_use]
    pub fn new(name: impl Into<String>, url: impl Into<String>, client: Arc<dyn WebhookClient>) -> Self {
        Self { config: WebhookConfig::new(name, url, client) }
    }

    /// Set the failure policy (builder style).
    #[must_use]
    pub fn with_failure_policy(mut self, policy: FailurePolicy) -> Self {
        self.config.failure_policy = policy;
        self
    }

    /// Restrict the webhook to the requests its [`WebhookRules`] select
    /// (builder style). Without this, the webhook fires on every request.
    #[must_use]
    pub fn with_rules(mut self, rules: WebhookRules) -> Self {
        self.config.rules = Some(rules);
        self
    }
}

impl MutatingPlugin for WebhookMutatingPlugin {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn admit(&self, request: &mut AdmissionRequest) -> Result<()> {
        // A request the webhook's rules do not select is left untouched.
        if !self.config.selects(request) {
            return Ok(());
        }
        let response = self.config.invoke(request)?;
        if !response.allowed {
            return Err(response.into_status());
        }
        if let Some(ops) = &response.json_patch {
            if let Some(object) = request.object.as_mut() {
                *object = patch::apply_json_patch(object, ops)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// base64 (standard alphabet, RFC 4648) — the patch field's framing.
// ---------------------------------------------------------------------------

/// Standard base64 alphabet.
const B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as standard base64 with `=` padding.
#[must_use]
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(B64_ALPHABET[b0 >> 2] as char);
        out.push(B64_ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decode standard base64 (padding optional; ASCII whitespace ignored). Returns
/// `None` on an invalid symbol or a malformed length.
#[must_use]
pub fn b64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut acc = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for &c in input.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = val(c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Test/in-process webhook client.
// ---------------------------------------------------------------------------

/// An in-process [`WebhookClient`] for tests and embedded webhooks: it records
/// every request body and answers via a caller-supplied responder.
pub struct MockWebhookClient {
    #[allow(clippy::type_complexity)]
    responder: Box<dyn Fn(&[u8]) -> std::result::Result<Vec<u8>, WebhookError> + Send + Sync>,
    requests: Mutex<Vec<Vec<u8>>>,
}

impl MockWebhookClient {
    /// Build a client whose response is computed from each request body.
    #[must_use]
    pub fn new(
        responder: impl Fn(&[u8]) -> std::result::Result<Vec<u8>, WebhookError> + Send + Sync + 'static,
    ) -> Self {
        Self { responder: Box::new(responder), requests: Mutex::new(Vec::new()) }
    }

    /// Build a client that always returns the same response body.
    #[must_use]
    pub fn always(response: Vec<u8>) -> Self {
        Self::new(move |_| Ok(response.clone()))
    }

    /// Build a client that always fails the transport (to exercise failure policy).
    #[must_use]
    pub fn failing(message: impl Into<String>) -> Self {
        let message = message.into();
        Self::new(move |_| Err(WebhookError::new(message.clone())))
    }

    /// The recorded request bodies, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<Vec<u8>> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl WebhookClient for MockWebhookClient {
    fn post(&self, _url: &str, body: &[u8]) -> std::result::Result<Vec<u8>, WebhookError> {
        self.requests.lock().expect("requests lock").push(body.to_vec());
        (self.responder)(body)
    }
}

// ---------------------------------------------------------------------------
// Real std-only `http://` client.
// ---------------------------------------------------------------------------

/// A std-only `http://` webhook client (one request per connection,
/// `Connection: close`). HTTPS endpoints use a TLS-backed client behind the same
/// [`WebhookClient`] trait.
#[derive(Clone, Debug, Default)]
pub struct HttpWebhookClient;

impl HttpWebhookClient {
    /// A new client.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Parse `http://host[:port]/path` into `(host, port, path)`.
fn parse_http_url(url: &str) -> std::result::Result<(String, u16, String), WebhookError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| WebhookError::new(format!("unsupported webhook URL scheme: {url:?}")))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| WebhookError::new(format!("invalid port in {url:?}")))?,
        ),
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return Err(WebhookError::new(format!("empty host in {url:?}")));
    }
    Ok((host, port, path.to_string()))
}

impl WebhookClient for HttpWebhookClient {
    fn post(&self, url: &str, body: &[u8]) -> std::result::Result<Vec<u8>, WebhookError> {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let (host, port, path) = parse_http_url(url)?;
        let mut stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| WebhookError::new(format!("connect {host}:{port}: {e}")))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body))
            .and_then(|()| stream.flush())
            .map_err(|e| WebhookError::new(format!("write: {e}")))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| WebhookError::new(format!("read: {e}")))?;

        let header_end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| WebhookError::new("webhook response had no header terminator"))?;
        let status_line = std::str::from_utf8(&raw[..raw.iter().position(|&b| b == b'\r').unwrap_or(0)])
            .unwrap_or("");
        let code: u16 = status_line.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        if !(200..300).contains(&code) {
            return Err(WebhookError::new(format!("webhook returned HTTP {code}")));
        }
        Ok(raw[header_end + 4..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admission::AdmissionChain;
    use crate::gvk::GroupVersionResource;
    use crate::json::obj;
    use std::io::Write;
    use std::net::TcpListener;

    fn create_request(name: &str) -> AdmissionRequest {
        AdmissionRequest {
            gvr: GroupVersionResource::new("", "v1", "pods"),
            operation: Operation::Create,
            namespace: "default".to_string(),
            object: Some(obj([
                ("apiVersion", Value::from("v1")),
                ("kind", Value::from("Pod")),
                ("metadata", obj([("name", Value::from(name))])),
            ])),
            old_object: None,
        }
    }

    /// Build an `AdmissionReview` response body echoing `uid`.
    fn review_response(uid: &str, allowed: bool, extra: Vec<(&str, Value)>) -> Vec<u8> {
        let mut fields = vec![
            ("uid", Value::from(uid)),
            ("allowed", Value::Bool(allowed)),
        ];
        fields.extend(extra);
        let resp = Value::Object(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        obj([
            ("apiVersion", Value::from(ADMISSION_API_VERSION)),
            ("kind", Value::from("AdmissionReview")),
            ("response", resp),
        ])
        .to_json_string()
        .into_bytes()
    }

    #[test]
    fn build_review_has_v1_envelope_and_request_fields() {
        let review = build_admission_review("uid-1", &create_request("nginx"));
        assert_eq!(review.pointer("apiVersion").and_then(Value::as_str), Some("admission.k8s.io/v1"));
        assert_eq!(review.pointer("kind").and_then(Value::as_str), Some("AdmissionReview"));
        assert_eq!(review.pointer("request.uid").and_then(Value::as_str), Some("uid-1"));
        assert_eq!(review.pointer("request.operation").and_then(Value::as_str), Some("CREATE"));
        assert_eq!(review.pointer("request.namespace").and_then(Value::as_str), Some("default"));
        assert_eq!(review.pointer("request.resource.resource").and_then(Value::as_str), Some("pods"));
        assert_eq!(review.pointer("request.kind.kind").and_then(Value::as_str), Some("Pod"));
        assert_eq!(
            review.pointer("request.object.metadata.name").and_then(Value::as_str),
            Some("nginx")
        );
    }

    #[test]
    fn kind_block_derives_group_version_from_api_version() {
        let mut req = create_request("x");
        req.object.as_mut().unwrap().insert("apiVersion", Value::from("apps/v1"));
        req.object.as_mut().unwrap().insert("kind", Value::from("Deployment"));
        req.gvr = GroupVersionResource::new("apps", "v1", "deployments");
        let review = build_admission_review("u", &req);
        assert_eq!(review.pointer("request.kind.group").and_then(Value::as_str), Some("apps"));
        assert_eq!(review.pointer("request.kind.version").and_then(Value::as_str), Some("v1"));
    }

    #[test]
    fn base64_round_trips() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", b"{\"op\":\"add\"}"] {
            let encoded = b64_encode(sample);
            assert_eq!(b64_decode(&encoded).as_deref(), Some(sample), "sample {sample:?}");
        }
        // Known vector.
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_decode("Zm9vYmFy").as_deref(), Some(&b"foobar"[..]));
        // Invalid symbol.
        assert!(b64_decode("****").is_none());
    }

    #[test]
    fn parse_allowed_response() {
        let body = review_response("uid-1", true, vec![]);
        let parsed = parse_admission_response(&body, "uid-1").expect("parse");
        assert!(parsed.allowed);
        assert!(parsed.json_patch.is_none());
    }

    #[test]
    fn parse_denied_response_maps_code_to_reason() {
        let status = obj([("code", Value::from(403_i64)), ("message", Value::from("nope"))]);
        let body = review_response("uid-1", false, vec![("status", status)]);
        let parsed = parse_admission_response(&body, "uid-1").expect("parse");
        assert!(!parsed.allowed);
        let status = parsed.into_status();
        assert_eq!(status.reason, StatusReason::Forbidden);
        assert_eq!(status.code, 403);
        assert_eq!(status.message, "nope");
    }

    #[test]
    fn parse_rejects_uid_mismatch() {
        let body = review_response("other-uid", true, vec![]);
        let err = parse_admission_response(&body, "uid-1").expect_err("mismatch");
        assert_eq!(err.reason, StatusReason::InternalError);
    }

    #[test]
    fn validating_plugin_allows() {
        let client = Arc::new(MockWebhookClient::new(|body| {
            // Echo the uid from the request to keep correlation honest.
            let review = json::parse(std::str::from_utf8(body).unwrap()).unwrap();
            let uid = review.pointer("request.uid").and_then(Value::as_str).unwrap();
            Ok(review_response(uid, true, vec![]))
        }));
        let plugin = WebhookValidatingPlugin::new("policy.example.com", "http://unused", client.clone());
        plugin.validate(&create_request("nginx")).expect("allowed");
        // The webhook actually received an AdmissionReview.
        let recorded = client.requests();
        assert_eq!(recorded.len(), 1);
        assert!(String::from_utf8_lossy(&recorded[0]).contains("\"kind\":\"AdmissionReview\""));
    }

    #[test]
    fn validating_plugin_denies_with_message() {
        let client = Arc::new(MockWebhookClient::new(|body| {
            let review = json::parse(std::str::from_utf8(body).unwrap()).unwrap();
            let uid = review.pointer("request.uid").and_then(Value::as_str).unwrap();
            let status = obj([("code", Value::from(422_i64)), ("message", Value::from("bad pod"))]);
            Ok(review_response(uid, false, vec![("status", status)]))
        }));
        let plugin = WebhookValidatingPlugin::new("policy", "http://unused", client);
        let err = plugin.validate(&create_request("nginx")).expect_err("deny");
        assert_eq!(err.reason, StatusReason::Invalid);
        assert_eq!(err.message, "bad pod");
    }

    #[test]
    fn mutating_plugin_applies_json_patch() {
        // Webhook injects a label via JSONPatch.
        let client = Arc::new(MockWebhookClient::new(|body| {
            let review = json::parse(std::str::from_utf8(body).unwrap()).unwrap();
            let uid = review.pointer("request.uid").and_then(Value::as_str).unwrap();
            let ops = Value::Array(vec![obj([
                ("op", Value::from("add")),
                ("path", Value::from("/metadata/labels")),
                ("value", obj([("injected", Value::from("true"))])),
            ])]);
            let encoded = b64_encode(ops.to_json_string().as_bytes());
            Ok(review_response(
                uid,
                true,
                vec![("patchType", Value::from("JSONPatch")), ("patch", Value::from(encoded))],
            ))
        }));
        let plugin = WebhookMutatingPlugin::new("mutate", "http://unused", client);
        let mut req = create_request("nginx");
        plugin.admit(&mut req).expect("admit");
        let label = req
            .object
            .as_ref()
            .and_then(|o| o.pointer("metadata.labels.injected"))
            .and_then(Value::as_str);
        assert_eq!(label, Some("true"));
    }

    #[test]
    fn failure_policy_ignore_allows_on_transport_error() {
        let client = Arc::new(MockWebhookClient::failing("connection refused"));
        let plugin = WebhookValidatingPlugin::new("flaky", "http://unused", client)
            .with_failure_policy(FailurePolicy::Ignore);
        plugin.validate(&create_request("nginx")).expect("ignored");
    }

    #[test]
    fn failure_policy_fail_rejects_on_transport_error() {
        let client = Arc::new(MockWebhookClient::failing("connection refused"));
        let plugin = WebhookValidatingPlugin::new("strict", "http://unused", client);
        let err = plugin.validate(&create_request("nginx")).expect_err("fail");
        assert_eq!(err.reason, StatusReason::InternalError);
    }

    #[test]
    fn webhook_plugs_into_admission_chain() {
        let client = Arc::new(MockWebhookClient::new(|body| {
            let review = json::parse(std::str::from_utf8(body).unwrap()).unwrap();
            let uid = review.pointer("request.uid").and_then(Value::as_str).unwrap();
            // Deny anything named "forbidden".
            let name = review.pointer("request.object.metadata.name").and_then(Value::as_str).unwrap_or("");
            let allowed = name != "forbidden";
            let extra = if allowed {
                vec![]
            } else {
                vec![("status", obj([("code", Value::from(403_i64)), ("message", Value::from("denied by policy"))]))]
            };
            Ok(review_response(uid, allowed, extra))
        }));
        let chain = AdmissionChain::new()
            .with_validating(Box::new(WebhookValidatingPlugin::new("policy", "http://unused", client)));
        assert!(chain.run(create_request("ok")).is_ok());
        let err = chain.run(create_request("forbidden")).expect_err("deny");
        assert_eq!(err.reason, StatusReason::Forbidden);
    }

    #[test]
    fn http_client_posts_to_a_real_listener() {
        // A one-shot HTTP server that returns a canned allowed AdmissionReview.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            // Drain the full request (head + Content-Length body) before
            // answering, so the client never sees a reset mid-write.
            let req = crate::server::read_request(&mut stream).expect("io").expect("request");
            let parsed = json::parse(std::str::from_utf8(&req.body).expect("utf8")).expect("json");
            let uid = parsed.pointer("request.uid").and_then(Value::as_str).unwrap_or("").to_string();
            let body = review_response(&uid, true, vec![]);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
            stream.flush().unwrap();
        });

        let url = format!("http://127.0.0.1:{}/validate", addr.port());
        let plugin = WebhookValidatingPlugin::new("real", url, Arc::new(HttpWebhookClient::new()));
        plugin.validate(&create_request("nginx")).expect("allowed over real HTTP");
        worker.join().expect("worker");
    }

    #[test]
    fn parse_http_url_splits_host_port_path() {
        assert_eq!(parse_http_url("http://h:8443/x").unwrap(), ("h".into(), 8443, "/x".into()));
        assert_eq!(parse_http_url("http://h/x").unwrap(), ("h".into(), 80, "/x".into()));
        assert_eq!(parse_http_url("http://h").unwrap(), ("h".into(), 80, "/".into()));
        assert!(parse_http_url("https://h/x").is_err());
    }

    // --- RuleWithOperations matching ---------------------------------------

    fn update_request() -> AdmissionRequest {
        let mut r = create_request("nginx");
        r.operation = Operation::Update;
        r.old_object = r.object.clone();
        r
    }

    fn deploy_create() -> AdmissionRequest {
        AdmissionRequest {
            gvr: GroupVersionResource::new("apps", "v1", "deployments"),
            operation: Operation::Create,
            namespace: "default".to_string(),
            object: Some(obj([
                ("apiVersion", Value::from("apps/v1")),
                ("kind", Value::from("Deployment")),
                ("metadata", obj([("name", Value::from("web"))])),
            ])),
            old_object: None,
        }
    }

    #[test]
    fn rule_matches_operation_group_and_resource() {
        // CREATE pods (core group) on a rule scoped to CREATE × ""/pods.
        let rule = RuleWithOperations::new(&["CREATE"], &[""], &["v1"], &["pods"]);
        assert!(rule.matches(&create_request("x")));
        // An UPDATE is excluded by the operations list.
        assert!(!rule.matches(&update_request()));
        // A different group/resource is excluded.
        assert!(!rule.matches(&deploy_create()));
    }

    #[test]
    fn rule_wildcards_match_everything() {
        let rule = RuleWithOperations::new(&["*"], &["*"], &["*"], &["*"]);
        assert!(rule.matches(&create_request("x")));
        assert!(rule.matches(&update_request()));
        assert!(rule.matches(&deploy_create()));
    }

    #[test]
    fn rule_set_matches_if_any_rule_matches() {
        let rules = WebhookRules::new(vec![
            RuleWithOperations::new(&["CREATE"], &["apps"], &["v1"], &["deployments"]),
            RuleWithOperations::new(&["DELETE"], &[""], &["v1"], &["pods"]),
        ]);
        assert!(rules.matches(&deploy_create()));
        // A CREATE pods request matches neither rule.
        assert!(!rules.matches(&create_request("x")));
    }

    #[test]
    fn empty_rule_set_matches_nothing() {
        // An explicitly-empty rule set selects no request (upstream: no Rules
        // means the webhook is never called).
        let rules = WebhookRules::new(vec![]);
        assert!(!rules.matches(&create_request("x")));
    }

    #[test]
    fn validating_plugin_skips_call_when_no_rule_matches() {
        // The webhook would DENY everything, but the request does not match its
        // rules (UPDATE-only), so the plugin must not call it and must allow.
        let client = Arc::new(MockWebhookClient::new(|body| {
            let review = json::parse(std::str::from_utf8(body).unwrap()).unwrap();
            let uid = review.pointer("request.uid").and_then(Value::as_str).unwrap();
            Ok(review_response(uid, false, vec![]))
        }));
        let plugin = WebhookValidatingPlugin::new("deny-updates", "http://unused", client.clone())
            .with_rules(WebhookRules::new(vec![RuleWithOperations::new(
                &["UPDATE"],
                &[""],
                &["v1"],
                &["pods"],
            )]));
        // A CREATE is not selected -> allowed without calling the webhook.
        plugin.validate(&create_request("x")).expect("allowed (skipped)");
        assert!(client.requests().is_empty(), "webhook must not be called");
        // An UPDATE *is* selected -> the webhook is called and denies.
        let err = plugin.validate(&update_request()).expect_err("denied");
        assert_eq!(err.reason, StatusReason::Forbidden);
        assert_eq!(client.requests().len(), 1);
    }

    #[test]
    fn mutating_plugin_skips_call_when_no_rule_matches() {
        let client = Arc::new(MockWebhookClient::new(|body| {
            let review = json::parse(std::str::from_utf8(body).unwrap()).unwrap();
            let uid = review.pointer("request.uid").and_then(Value::as_str).unwrap();
            let ops = Value::Array(vec![obj([
                ("op", Value::from("add")),
                ("path", Value::from("/metadata/labels")),
                ("value", obj([("injected", Value::from("true"))])),
            ])]);
            let encoded = b64_encode(ops.to_json_string().as_bytes());
            Ok(review_response(
                uid,
                true,
                vec![("patchType", Value::from("JSONPatch")), ("patch", Value::from(encoded))],
            ))
        }));
        let plugin = WebhookMutatingPlugin::new("label-deployments", "http://unused", client.clone())
            .with_rules(WebhookRules::new(vec![RuleWithOperations::new(
                &["CREATE"],
                &["apps"],
                &["v1"],
                &["deployments"],
            )]));
        // A core/v1 pod create is not selected -> object is left untouched.
        let mut pod = create_request("x");
        plugin.admit(&mut pod).expect("admit");
        assert!(pod.object.as_ref().and_then(|o| o.pointer("metadata.labels.injected")).is_none());
        assert!(client.requests().is_empty());
        // A deployment create IS selected -> label injected.
        let mut deploy = deploy_create();
        plugin.admit(&mut deploy).expect("admit");
        assert_eq!(
            deploy.object.as_ref().and_then(|o| o.pointer("metadata.labels.injected")).and_then(Value::as_str),
            Some("true")
        );
    }

    #[test]
    fn plugin_without_rules_matches_all_requests() {
        // Backwards-compatible default: a plugin with no configured rules fires
        // on every request (the existing behaviour the chain relied on).
        let client = Arc::new(MockWebhookClient::new(|body| {
            let review = json::parse(std::str::from_utf8(body).unwrap()).unwrap();
            let uid = review.pointer("request.uid").and_then(Value::as_str).unwrap();
            Ok(review_response(uid, true, vec![]))
        }));
        let plugin = WebhookValidatingPlugin::new("all", "http://unused", client.clone());
        plugin.validate(&create_request("x")).expect("allowed");
        plugin.validate(&deploy_create()).expect("allowed");
        assert_eq!(client.requests().len(), 2);
    }
}
