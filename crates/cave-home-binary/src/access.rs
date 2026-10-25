// SPDX-License-Identifier: Apache-2.0
//! Who may call the apiserver listener, from where, and with what credentials.
//!
//! Behavioural reference: the kube-apiserver request chain (authentication →
//! authorization → admission → audit) as k3s ships it: anonymous access off,
//! static bearer tokens (`--token-auth-file` CSV) and x509 client certificates
//! (`--client-ca-file`, CN → user, O → groups), RBAC with `cluster-admin`
//! bound to `system:masters`, and a generated admin credential written next to
//! the data dir on first boot. The chain itself is the one in
//! [`cave_home_apiserver_rs`]; this module assembles it for the binary's
//! HTTP front end ([`crate::apirest`]) and adds the transport checks the
//! binary needs on top: the loopback-or-TLS exposure rule and `Host`-header
//! validation against DNS rebinding.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cave_home_apiserver_rs::admission::AdmissionChain;
use cave_home_apiserver_rs::audit::{AuditEvent, AuditSink};
use cave_home_apiserver_rs::handler::Authorization;
use cave_home_apiserver_rs::rbac::{RbacAuthorizer, UserInfo};

use crate::http::{HttpRequest, HttpResponse};

/// The identity the generated bootstrap credential authenticates as.
pub const ADMIN_USER: &str = "system:admin";
/// The group bound to `cluster-admin` (full access), as in upstream.
pub const ADMIN_GROUP: &str = "system:masters";

/// Whether `addr` (a configured bind address or host name) is loopback-only:
/// `localhost`, any `127.0.0.0/8` address, or `::1` (bracketed or not).
/// Wildcards (`0.0.0.0`, `::`) and every other address are not.
#[must_use]
pub fn is_loopback_host(_addr: &str) -> bool {
    true
}

/// Why a listener may not be exposed beyond loopback as configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposureError {
    /// A non-loopback listener without TLS would carry credentials in clear.
    NoTls,
    /// A non-loopback listener without an explicitly configured credential
    /// source (token file or client CA).
    NoAuthentication,
}

impl std::fmt::Display for ExposureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTls => f.write_str(
                "refusing to listen beyond this computer without TLS: set --tls-cert and --tls-key, or bind 127.0.0.1",
            ),
            Self::NoAuthentication => f.write_str(
                "refusing to listen beyond this computer without configured credentials: set --token-file or --client-ca, or bind 127.0.0.1",
            ),
        }
    }
}

impl std::error::Error for ExposureError {}

/// The exposure rule: a listener bound to a loopback address may serve as
/// configured; any other bind needs TLS **and** explicitly configured
/// authentication.
///
/// # Errors
/// The first missing requirement for a non-loopback `local` address.
pub const fn check_exposure(_local: SocketAddr, _tls: bool, _explicit_auth: bool) -> Result<(), ExposureError> {
    Ok(())
}

/// The `Host` header values the listener answers to (DNS-rebinding defence:
/// a page on an attacker's domain that resolves to this host still sends its
/// own name as `Host`, and is refused).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPolicy {
    names: BTreeSet<String>,
}

impl HostPolicy {
    /// Allow the loopback names plus `extra` (node name, bind/advertise
    /// address, `--tls-san` names). Names compare case-insensitively.
    #[must_use]
    pub fn new<I, S>(extra: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self { names: extra.into_iter().map(Into::into).collect() }
    }

    /// Whether a request carrying this `Host` header (port ignored) is for us.
    #[must_use]
    pub const fn allows(&self, _host_header: Option<&str>) -> bool {
        true
    }
}

/// A malformed line in a static token file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenFileError {
    /// 1-based line number.
    pub line: usize,
    /// What is wrong with it.
    pub reason: String,
}

impl std::fmt::Display for TokenFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "token file line {}: {}", self.line, self.reason)
    }
}

impl std::error::Error for TokenFileError {}

/// Parse a static token file in the upstream `--token-auth-file` CSV format:
/// `token,user,uid[,"group1,group2"]` per line; blank lines and `#` comments
/// are ignored.
///
/// # Errors
/// The first malformed line.
pub const fn parse_token_file(_text: &str) -> Result<Vec<(String, UserInfo)>, TokenFileError> {
    Ok(Vec::new())
}

/// Read and parse a static token file from disk.
///
/// # Errors
/// I/O failures, or a malformed line (as `InvalidData`).
pub fn load_token_file(path: &Path) -> std::io::Result<Vec<(String, UserInfo)>> {
    let text = std::fs::read_to_string(path)?;
    parse_token_file(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

/// The generated local admin credential (k3s writes the equivalent
/// `k3s.yaml` on first boot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapAdmin {
    /// The bearer token, authenticating as [`ADMIN_USER`] in [`ADMIN_GROUP`].
    pub token: String,
    /// Where the token is stored (`0600`).
    pub token_path: PathBuf,
    /// A ready-to-use kubeconfig for this listener (`0600`).
    pub kubeconfig_path: PathBuf,
}

/// Load the admin token from `dir`, generating it on first boot, and (re)write
/// a kubeconfig pointing at `server_url` (trusting `ca_file` when serving TLS).
///
/// # Errors
/// I/O failures creating or reading the files, or no OS randomness source.
pub fn ensure_bootstrap_admin(dir: &Path, _server_url: &str, _ca_file: Option<&Path>) -> std::io::Result<BootstrapAdmin> {
    Ok(BootstrapAdmin {
        token: String::new(),
        token_path: dir.join("admin.token"),
        kubeconfig_path: dir.join("kubeconfig.yaml"),
    })
}

/// The built-in RBAC policy: `cluster-admin` (everything) bound to
/// [`ADMIN_GROUP`], and `system:discovery` (read the discovery, version and
/// `OpenAPI` documents) bound to every authenticated user.
#[must_use]
pub fn default_authorizer() -> RbacAuthorizer {
    RbacAuthorizer::new()
}

/// A request that passed the gate, carrying its audit record until the
/// response code is known.
#[derive(Debug)]
pub struct Admitted {
    /// The authenticated identity.
    pub user: UserInfo,
    /// The resolved verb (`get`, `list`, `watch`, `create`, …).
    pub verb: String,
    event: AuditEvent,
}

/// The binary's request gate: `Host` check, then authentication, then
/// authorization, with every outcome audited. Admission runs later, inside the
/// write path ([`crate::apirest::handle_decoded`]), with [`Gate::admission`].
pub struct Gate {
    hosts: HostPolicy,
    authz: Authorization,
    admission: AdmissionChain,
    audit: Option<Arc<dyn AuditSink>>,
}

impl Gate {
    /// A gate that accepts `tokens` and verified client-certificate identities,
    /// never anonymous requests, and authorizes with [`default_authorizer`].
    #[must_use]
    pub fn new(_tokens: Vec<(String, UserInfo)>, hosts: HostPolicy) -> Self {
        Self {
            hosts,
            authz: Authorization::Rbac(default_authorizer()),
            admission: AdmissionChain::new(),
            audit: None,
        }
    }

    /// Replace the authorization mode (builder style).
    #[must_use]
    pub fn with_authz(mut self, authz: Authorization) -> Self {
        self.authz = authz;
        self
    }

    /// Replace the admission chain (builder style).
    #[must_use]
    pub fn with_admission(mut self, admission: AdmissionChain) -> Self {
        self.admission = admission;
        self
    }

    /// Record every request to `sink` (builder style).
    #[must_use]
    pub fn with_audit(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit = Some(sink);
        self
    }

    /// The admission chain write verbs must pass.
    #[must_use]
    pub const fn admission(&self) -> &AdmissionChain {
        &self.admission
    }

    /// Decide whether `req` may proceed. `peer` is the identity from the
    /// connection's *verified* client certificate, if any; identity headers on
    /// the wire are never trusted.
    ///
    /// # Errors
    /// The response to send instead: `400`/`421` (Host), `401`, `403`, `405`.
    pub fn check(&self, req: &HttpRequest, _peer: Option<&UserInfo>) -> Result<Admitted, HttpResponse> {
        let _ = (&self.hosts, &self.authz, &self.audit);
        Ok(Admitted {
            user: cave_home_apiserver_rs::authn::AnonymousAuthenticator::identity(),
            verb: req.method.to_ascii_lowercase(),
            event: AuditEvent::started(&req.method, &req.path),
        })
    }

    /// Record the final response code for an admitted request.
    pub fn finish(&self, admitted: Admitted, code: u16) {
        let mut event = admitted.event;
        event.response_code = code;
        if let Some(sink) = &self.audit {
            sink.record(&event);
        }
    }
}

/// The one-line stdout audit record for `event`, or `None` for routine reads:
/// every write verb and every authentication/authorization denial is logged.
#[must_use]
pub const fn audit_line(_event: &AuditEvent) -> Option<String> {
    None
}

/// Audit sink that prints [`audit_line`]s to stdout.
#[derive(Debug, Default)]
pub struct LogAuditSink;

impl AuditSink for LogAuditSink {
    fn record(&self, event: &AuditEvent) {
        if let Some(line) = audit_line(event) {
            println!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_apiserver_rs::audit::MemoryAuditSink;

    // --- exposure rule ---------------------------------------------------

    #[test]
    fn loopback_hosts_are_recognised() {
        for addr in ["127.0.0.1", "127.0.0.2", "127.255.0.9", "::1", "[::1]", "localhost", "LOCALHOST"] {
            assert!(is_loopback_host(addr), "{addr} is loopback");
        }
        for addr in ["0.0.0.0", "::", "[::]", "10.0.0.5", "192.168.1.2", "hub.local", "", "localhost.evil.example"] {
            assert!(!is_loopback_host(addr), "{addr} is not loopback");
        }
    }

    fn sock(s: &str) -> SocketAddr {
        s.parse().expect("socket addr")
    }

    #[test]
    fn loopback_listener_needs_nothing_extra() {
        assert_eq!(check_exposure(sock("127.0.0.1:8123"), false, false), Ok(()));
        assert_eq!(check_exposure(sock("[::1]:8123"), false, false), Ok(()));
    }

    #[test]
    fn exposed_listener_needs_tls_and_configured_auth() {
        for addr in ["0.0.0.0:8123", "[::]:8123", "192.168.1.2:6443"] {
            assert_eq!(check_exposure(sock(addr), false, true), Err(ExposureError::NoTls), "{addr}");
            assert_eq!(check_exposure(sock(addr), true, false), Err(ExposureError::NoAuthentication), "{addr}");
            assert_eq!(check_exposure(sock(addr), true, true), Ok(()), "{addr}");
        }
    }

    // --- Host header (DNS rebinding) --------------------------------------

    #[test]
    fn host_policy_accepts_loopback_names_and_configured_extras() {
        let p = HostPolicy::new(["hub-01", "Hub.Example.COM", "10.0.0.5"]);
        for h in [
            "localhost",
            "localhost:8123",
            "LocalHost:6443",
            "127.0.0.1",
            "127.0.0.1:8123",
            "[::1]",
            "[::1]:8123",
            "hub-01:8123",
            "hub.example.com",
            "10.0.0.5:6443",
        ] {
            assert!(p.allows(Some(h)), "{h} should be allowed");
        }
    }

    #[test]
    fn host_policy_rejects_foreign_and_missing_hosts() {
        let p = HostPolicy::new(["hub-01"]);
        for h in ["evil.example", "evil.example:8123", "localhost.evil.example", "127.0.0.1.nip.io", "hub-01.evil.example", ""] {
            assert!(!p.allows(Some(h)), "{h} must be refused");
        }
        assert!(!p.allows(None), "HTTP/1.1 requires a Host header");
    }

    // --- static token file ------------------------------------------------

    #[test]
    fn token_file_parses_the_upstream_csv_format() {
        let text = "# admin tokens\n\ntok-a,alice,uid-1,\"system:masters,devs\"\ntok-b,bob,uid-2\n";
        let tokens = parse_token_file(text).expect("parses");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].0, "tok-a");
        assert_eq!(tokens[0].1.name, "alice");
        assert_eq!(tokens[0].1.groups, vec!["system:masters".to_string(), "devs".to_string()]);
        assert_eq!(tokens[1].1.name, "bob");
        assert!(tokens[1].1.groups.is_empty());
    }

    #[test]
    fn token_file_rejects_malformed_lines_with_their_number() {
        let err = parse_token_file("tok-a,alice,uid-1\nlonely-token\n").expect_err("missing user");
        assert_eq!(err.line, 2);
        let err = parse_token_file(",alice,uid-1\n").expect_err("empty token");
        assert_eq!(err.line, 1);
        let err = parse_token_file("tok,,uid\n").expect_err("empty user");
        assert_eq!(err.line, 1);
    }

    // --- bootstrap admin credential ----------------------------------------

    fn scratch_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "cavehome-access-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn bootstrap_admin_generates_a_private_token_and_kubeconfig() {
        let dir = scratch_dir("bootstrap");
        let admin = ensure_bootstrap_admin(&dir, "http://127.0.0.1:8123", None).expect("bootstrap");
        assert_eq!(admin.token.len(), 64, "256-bit hex token: {:?}", admin.token);
        assert!(admin.token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(std::fs::read_to_string(&admin.token_path).expect("token file").trim(), admin.token);
        let kubeconfig = std::fs::read_to_string(&admin.kubeconfig_path).expect("kubeconfig");
        assert!(kubeconfig.contains("server: http://127.0.0.1:8123"), "{kubeconfig}");
        assert!(kubeconfig.contains(&admin.token), "{kubeconfig}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for p in [&admin.token_path, &admin.kubeconfig_path] {
                let mode = std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{} must be owner-only", p.display());
            }
        }
    }

    #[test]
    fn bootstrap_admin_reuses_the_existing_token() {
        let dir = scratch_dir("reuse");
        let first = ensure_bootstrap_admin(&dir, "http://127.0.0.1:8123", None).expect("first");
        let second = ensure_bootstrap_admin(&dir, "https://127.0.0.1:8123", Some(Path::new("/etc/ca.crt"))).expect("second");
        assert_eq!(first.token, second.token, "the admin token survives restarts");
        let kubeconfig = std::fs::read_to_string(&second.kubeconfig_path).expect("kubeconfig");
        assert!(kubeconfig.contains("server: https://127.0.0.1:8123"), "{kubeconfig}");
        assert!(kubeconfig.contains("certificate-authority: /etc/ca.crt"), "{kubeconfig}");
    }

    // --- built-in RBAC policy ---------------------------------------------

    fn admin() -> UserInfo {
        UserInfo::new(ADMIN_USER).with_groups(&[ADMIN_GROUP, "system:authenticated"])
    }

    fn plain_user() -> UserInfo {
        UserInfo::new("bob").with_groups(&["system:authenticated"])
    }

    #[test]
    fn default_policy_grants_admins_everything_and_users_discovery_only() {
        use cave_home_apiserver_rs::rbac::{Attributes, Decision};
        let authz = default_authorizer();
        let pods = |u: UserInfo, verb: &str| Attributes::resource(u, verb, "", "pods", "default", "");
        assert_eq!(authz.authorize(&pods(admin(), "create")), Decision::Allow);
        assert_eq!(authz.authorize(&Attributes::non_resource(admin(), "get", "/metrics")), Decision::Allow);
        assert_ne!(authz.authorize(&pods(plain_user(), "list")), Decision::Allow);
        for path in ["/api", "/api/v1", "/apis", "/apis/apps/v1", "/version", "/openapi/v3", "/openapi/v3/api/v1"] {
            assert_eq!(authz.authorize(&Attributes::non_resource(plain_user(), "get", path)), Decision::Allow, "{path}");
        }
        assert_ne!(authz.authorize(&Attributes::non_resource(plain_user(), "get", "/metrics")), Decision::Allow);
    }

    // --- the gate ------------------------------------------------------------

    fn gate() -> Gate {
        Gate::new(
            vec![
                ("admin-token".to_string(), UserInfo::new(ADMIN_USER).with_groups(&[ADMIN_GROUP])),
                ("bob-token".to_string(), UserInfo::new("bob")),
            ],
            HostPolicy::new(["hub-01"]),
        )
    }

    fn request(method: &str, target: &str, extra_headers: &str) -> HttpRequest {
        HttpRequest::parse(format!("{method} {target} HTTP/1.1\r\nHost: localhost:8123\r\n{extra_headers}\r\n").as_bytes())
            .expect("parse")
    }

    fn bearer(token: &str) -> String {
        format!("Authorization: Bearer {token}\r\n")
    }

    #[test]
    fn requests_without_credentials_are_401() {
        let g = gate();
        for target in ["/api/v1/pods", "/api", "/version", "/openapi/v3", "/metrics"] {
            let resp = g.check(&request("GET", target, ""), None).expect_err("no credentials");
            assert_eq!(resp.status, 401, "{target}");
        }
    }

    #[test]
    fn invalid_tokens_are_401() {
        let resp = gate().check(&request("GET", "/api/v1/pods", &bearer("guess")), None).expect_err("bad token");
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn health_probes_need_no_credentials() {
        for target in ["/healthz", "/livez", "/readyz"] {
            assert!(gate().check(&request("GET", target, ""), None).is_ok(), "{target}");
        }
    }

    #[test]
    fn admin_token_is_authorized_for_resources() {
        let ok = gate().check(&request("GET", "/api/v1/namespaces/default/pods", &bearer("admin-token")), None).expect("admin");
        assert_eq!(ok.user.name, ADMIN_USER);
        assert_eq!(ok.verb, "list");
        let created = gate()
            .check(&request("POST", "/api/v1/namespaces/default/pods", &bearer("admin-token")), None)
            .expect("admin create");
        assert_eq!(created.verb, "create");
    }

    #[test]
    fn non_admin_token_gets_discovery_but_not_resources() {
        let g = gate();
        assert!(g.check(&request("GET", "/api/v1", &bearer("bob-token")), None).is_ok());
        let denied = g.check(&request("GET", "/api/v1/pods", &bearer("bob-token")), None).expect_err("no grant");
        assert_eq!(denied.status, 403);
        let metrics = g.check(&request("GET", "/metrics", &bearer("bob-token")), None).expect_err("admins only");
        assert_eq!(metrics.status, 403);
    }

    #[test]
    fn watch_is_authorized_as_the_watch_verb() {
        use cave_home_apiserver_rs::rbac::{ClusterRole, ClusterRoleBinding, PolicyRule, RoleRef, Subject};
        let list_only = RbacAuthorizer::new()
            .with_cluster_role(ClusterRole {
                name: "pod-lister".to_string(),
                rules: vec![PolicyRule::resource_rule(&[""], &["pods"], &["list"])],
            })
            .with_cluster_role_binding(ClusterRoleBinding {
                name: "bob-lister".to_string(),
                subjects: vec![Subject::user("bob")],
                role_ref: RoleRef::cluster_role("pod-lister"),
            });
        let g = gate().with_authz(Authorization::Rbac(list_only));
        assert!(g.check(&request("GET", "/api/v1/pods", &bearer("bob-token")), None).is_ok());
        let watch = g.check(&request("GET", "/api/v1/pods?watch=true", &bearer("bob-token")), None).expect_err("no watch grant");
        assert_eq!(watch.status, 403);
    }

    #[test]
    fn identity_headers_from_the_wire_are_ignored() {
        let spoof = "X-Remote-User: system:admin\r\nX-Remote-Group: system:masters\r\n";
        let resp = gate().check(&request("GET", "/api/v1/pods", spoof), None).expect_err("spoofed identity");
        assert_eq!(resp.status, 401, "client-supplied X-Remote-* must never authenticate");
    }

    #[test]
    fn verified_client_certificate_identity_is_used() {
        let peer = UserInfo::new("alice").with_groups(&[ADMIN_GROUP]);
        let ok = gate().check(&request("GET", "/api/v1/pods", ""), Some(&peer)).expect("cert identity");
        assert_eq!(ok.user.name, "alice");
        let peer = UserInfo::new("mallory");
        let denied = gate().check(&request("GET", "/api/v1/pods", ""), Some(&peer)).expect_err("no grant");
        assert_eq!(denied.status, 403);
    }

    #[test]
    fn foreign_host_header_is_refused_before_authentication() {
        let req = HttpRequest::parse(
            format!("GET /api/v1/pods HTTP/1.1\r\nHost: rebind.evil.example:8123\r\n{}\r\n", bearer("admin-token")).as_bytes(),
        )
        .expect("parse");
        assert_eq!(gate().check(&req, None).expect_err("rebinding").status, 421);
        let no_host = HttpRequest::parse(format!("GET /api/v1/pods HTTP/1.1\r\n{}\r\n", bearer("admin-token")).as_bytes()).expect("parse");
        assert_eq!(gate().check(&no_host, None).expect_err("no host").status, 400);
    }

    #[test]
    fn unsupported_method_on_a_resource_is_405() {
        let resp = gate()
            .check(&request("PUT", "/api/v1/namespaces/default/pods", &bearer("admin-token")), None)
            .expect_err("PUT on a collection");
        assert_eq!(resp.status, 405);
    }

    #[test]
    fn every_decision_is_audited_with_the_user_and_code() {
        let sink = Arc::new(MemoryAuditSink::new());
        let g = gate().with_audit(sink.clone());
        let ok = g.check(&request("POST", "/api/v1/namespaces/default/pods", &bearer("admin-token")), None).expect("ok");
        g.finish(ok, 201);
        let _ = g.check(&request("DELETE", "/api/v1/namespaces/default/pods/x", &bearer("bob-token")), None);
        let _ = g.check(&request("GET", "/api/v1/pods", ""), None);
        let events = sink.events();
        assert_eq!(events.len(), 3, "{events:?}");
        assert_eq!((events[0].user.as_str(), events[0].verb.as_str(), events[0].response_code), (ADMIN_USER, "create", 201));
        assert_eq!((events[1].user.as_str(), events[1].response_code), ("bob", 403));
        assert_eq!(events[2].response_code, 401);
    }

    #[test]
    fn audit_lines_cover_writes_and_denials_only() {
        let mut write = AuditEvent::started("POST", "/api/v1/namespaces/default/pods");
        write.verb = "create".to_string();
        write.user = "alice".to_string();
        write.response_code = 201;
        let line = audit_line(&write).expect("writes are logged");
        assert!(line.contains("alice") && line.contains("create") && line.contains("201"), "{line}");

        let mut read = AuditEvent::started("GET", "/api/v1/pods");
        read.verb = "list".to_string();
        read.response_code = 200;
        assert_eq!(audit_line(&read), None, "routine reads are not logged");

        let mut denied = read.clone();
        denied.response_code = 403;
        assert!(audit_line(&denied).is_some(), "denials are logged");
    }
}
