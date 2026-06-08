// SPDX-License-Identifier: Apache-2.0
//! Authentication: map an inbound request to an authenticated identity.
//!
//! Behavioural reference: the Kubernetes authentication contract
//! (`authentication.md`): a chain of authenticators is tried in order; the first
//! that produces an identity wins; a presented-but-invalid credential is a hard
//! `401 Unauthorized`; a request with no credentials falls through to the
//! anonymous identity (`system:anonymous`, group `system:unauthenticated`) when
//! anonymous access is enabled, otherwise `401`.
//!
//! This std-only implementation ships the bearer-token authenticator (the
//! token-file mechanism k3s uses for its bootstrap/node tokens) and the
//! anonymous authenticator. mTLS client-cert and OIDC authenticators are
//! deferred (see `parity.manifest.toml`); they plug into the same
//! [`Authenticator`] trait.

use std::collections::BTreeMap;

use crate::http::Request;
use crate::rbac::UserInfo;
use crate::status::{Result, Status};

/// Extract the bearer token from an `Authorization: Bearer <token>` header.
/// Returns `None` if the header is absent or uses a different scheme.
fn bearer_token(req: &Request) -> Option<&str> {
    let value = req.headers.get("authorization")?;
    let rest = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer "))?;
    let token = rest.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// An authenticator inspects a request and produces an identity.
///
/// The three outcomes mirror the upstream `authenticator.Request` contract:
/// - `Ok(Some(user))` — credentials recognized.
/// - `Ok(None)` — no credentials this authenticator handles ("no opinion");
///   the chain tries the next authenticator.
/// - `Err(401)` — credentials were presented but are invalid; a hard stop.
pub trait Authenticator: Send + Sync {
    /// Attempt to authenticate `req`.
    ///
    /// # Errors
    /// A `401 Unauthorized` [`Status`] when a presented credential is invalid.
    fn authenticate(&self, req: &Request) -> Result<Option<UserInfo>>;
}

/// Static bearer-token authenticator (the token-file mechanism). Maps a set of
/// known tokens to identities; an unknown *presented* token is a hard `401`.
#[derive(Debug, Default)]
pub struct TokenAuthenticator {
    tokens: BTreeMap<String, UserInfo>,
}

impl TokenAuthenticator {
    /// An empty token set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `token` → `user` (builder style).
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>, user: UserInfo) -> Self {
        self.tokens.insert(token.into(), user);
        self
    }
}

impl Authenticator for TokenAuthenticator {
    fn authenticate(&self, req: &Request) -> Result<Option<UserInfo>> {
        match bearer_token(req) {
            None => Ok(None),
            Some(tok) => match self.tokens.get(tok) {
                Some(user) => Ok(Some(user.clone())),
                None => Err(Status::unauthorized("invalid bearer token")),
            },
        }
    }
}

/// Authenticate from the front-proxy request headers (`X-Remote-User` +
/// repeatable `X-Remote-Group`), the mechanism k8s calls
/// `--requestheader-*` / the authenticating proxy.
///
/// **Security contract:** these headers are trusted *only* because the TLS
/// terminator ([`crate::tls`]) strips any client-supplied `X-Remote-*` and then
/// sets them from the verified client certificate's subject. Never place this
/// authenticator in front of a transport that does not perform that
/// strip-then-inject step, or a client could spoof any identity.
#[derive(Debug, Default)]
pub struct RequestHeaderAuthenticator;

impl RequestHeaderAuthenticator {
    /// A new authenticator.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Authenticator for RequestHeaderAuthenticator {
    fn authenticate(&self, req: &Request) -> Result<Option<UserInfo>> {
        match req.headers.get("x-remote-user") {
            None => Ok(None),
            Some(name) if name.is_empty() => Ok(None),
            Some(name) => {
                let groups = req
                    .headers
                    .get_all("x-remote-group")
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                Ok(Some(UserInfo { name: name.to_string(), groups }))
            }
        }
    }
}

/// Always succeeds with the anonymous identity. Placed last in a chain to allow
/// unauthenticated access; omit it to require credentials.
#[derive(Debug, Default)]
pub struct AnonymousAuthenticator;

impl AnonymousAuthenticator {
    /// The canonical anonymous identity.
    #[must_use]
    pub fn identity() -> UserInfo {
        UserInfo::new("system:anonymous").with_groups(&["system:unauthenticated"])
    }
}

impl Authenticator for AnonymousAuthenticator {
    fn authenticate(&self, _req: &Request) -> Result<Option<UserInfo>> {
        Ok(Some(Self::identity()))
    }
}

/// An ordered chain of authenticators. The first to recognize a credential wins;
/// a presented-but-invalid credential short-circuits with `401`. If no
/// authenticator produces an identity, the request is treated as anonymous when
/// `allow_anonymous` is set, otherwise rejected `401`.
#[derive(Default)]
pub struct AuthenticatorChain {
    delegates: Vec<Box<dyn Authenticator>>,
    allow_anonymous: bool,
}

impl AuthenticatorChain {
    /// An empty chain (anonymous disabled by default).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an authenticator (builder style).
    #[must_use]
    pub fn with(mut self, a: Box<dyn Authenticator>) -> Self {
        self.delegates.push(a);
        self
    }

    /// Enable/disable the anonymous fallthrough (builder style).
    #[must_use]
    pub fn allow_anonymous(mut self, allow: bool) -> Self {
        self.allow_anonymous = allow;
        self
    }

    /// Resolve the request's identity.
    ///
    /// # Errors
    /// `401 Unauthorized` when a credential is invalid, or when no credential is
    /// supplied and anonymous access is disabled.
    pub fn authenticate(&self, req: &Request) -> Result<UserInfo> {
        for d in &self.delegates {
            if let Some(user) = d.authenticate(req)? {
                return Ok(add_authenticated_group(user));
            }
        }
        if self.allow_anonymous {
            Ok(AnonymousAuthenticator::identity())
        } else {
            Err(Status::unauthorized("no credentials provided"))
        }
    }
}

/// The built-in group every authenticated identity carries.
pub const SYSTEM_AUTHENTICATED: &str = "system:authenticated";
/// The built-in group the anonymous identity carries.
pub const SYSTEM_UNAUTHENTICATED: &str = "system:unauthenticated";

/// Append `system:authenticated` to a successfully-authenticated identity's
/// groups (mirroring upstream's `AuthenticatedGroupAdder`), unless it is the
/// anonymous identity or already carries the group. The anonymous identity
/// (`system:anonymous` / member of `system:unauthenticated`) is never tagged.
fn add_authenticated_group(mut user: UserInfo) -> UserInfo {
    let is_anonymous = user.name == "system:anonymous"
        || user.groups.iter().any(|g| g == SYSTEM_UNAUTHENTICATED);
    if is_anonymous {
        return user;
    }
    if !user.groups.iter().any(|g| g == SYSTEM_AUTHENTICATED) {
        user.groups.push(SYSTEM_AUTHENTICATED.to_string());
    }
    user
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Request;
    use crate::rbac::UserInfo;
    use crate::status::StatusReason;

    fn req_with_auth(value: &str) -> Request {
        let raw = format!("GET /api/v1/pods HTTP/1.1\r\nAuthorization: {value}\r\n\r\n");
        Request::parse(raw.as_bytes()).expect("parse")
    }

    fn req_no_auth() -> Request {
        Request::parse(b"GET /api/v1/pods HTTP/1.1\r\n\r\n").expect("parse")
    }

    #[test]
    fn unauthorized_status_is_401() {
        assert_eq!(StatusReason::Unauthorized.code(), 401);
        assert_eq!(StatusReason::Unauthorized.as_str(), "Unauthorized");
    }

    #[test]
    fn token_authenticator_accepts_valid_bearer() {
        let auth = TokenAuthenticator::new()
            .with_token("s3cret", UserInfo::new("admin").with_groups(&["system:masters"]));
        let user = auth
            .authenticate(&req_with_auth("Bearer s3cret"))
            .expect("ok")
            .expect("some");
        assert_eq!(user.name, "admin");
        assert_eq!(user.groups, vec!["system:masters".to_string()]);
    }

    #[test]
    fn token_authenticator_no_header_is_no_opinion() {
        let auth = TokenAuthenticator::new().with_token("t", UserInfo::new("u"));
        assert!(auth.authenticate(&req_no_auth()).expect("ok").is_none());
    }

    #[test]
    fn token_authenticator_unknown_token_is_401() {
        let auth = TokenAuthenticator::new().with_token("good", UserInfo::new("u"));
        let err = auth.authenticate(&req_with_auth("Bearer bad")).expect_err("401");
        assert_eq!(err.code, 401);
        assert_eq!(err.reason, StatusReason::Unauthorized);
    }

    #[test]
    fn token_authenticator_non_bearer_scheme_is_no_opinion() {
        let auth = TokenAuthenticator::new().with_token("t", UserInfo::new("u"));
        // A Basic credential is simply not handled by the token authenticator.
        assert!(auth.authenticate(&req_with_auth("Basic abc")).expect("ok").is_none());
    }

    #[test]
    fn request_header_authenticator_reads_user_and_groups() {
        let raw = "GET /api/v1/pods HTTP/1.1\r\nX-Remote-User: alice\r\nX-Remote-Group: system:masters\r\nX-Remote-Group: dev\r\n\r\n";
        let req = Request::parse(raw.as_bytes()).expect("parse");
        let user = RequestHeaderAuthenticator.authenticate(&req).expect("ok").expect("some");
        assert_eq!(user.name, "alice");
        assert_eq!(user.groups, vec!["system:masters".to_string(), "dev".to_string()]);
    }

    #[test]
    fn request_header_authenticator_no_header_is_no_opinion() {
        assert!(RequestHeaderAuthenticator.authenticate(&req_no_auth()).expect("ok").is_none());
    }

    #[test]
    fn anonymous_authenticator_yields_system_anonymous() {
        let user = AnonymousAuthenticator.authenticate(&req_no_auth()).expect("ok").expect("some");
        assert_eq!(user.name, "system:anonymous");
        assert_eq!(user.groups, vec!["system:unauthenticated".to_string()]);
    }

    #[test]
    fn chain_valid_token_wins() {
        let chain = AuthenticatorChain::new()
            .with(Box::new(
                TokenAuthenticator::new().with_token("k", UserInfo::new("node")),
            ))
            .allow_anonymous(true);
        let user = chain.authenticate(&req_with_auth("Bearer k")).expect("authn");
        assert_eq!(user.name, "node");
    }

    #[test]
    fn chain_no_creds_falls_through_to_anonymous_when_enabled() {
        let chain = AuthenticatorChain::new()
            .with(Box::new(TokenAuthenticator::new().with_token("k", UserInfo::new("node"))))
            .allow_anonymous(true);
        let user = chain.authenticate(&req_no_auth()).expect("anon");
        assert_eq!(user.name, "system:anonymous");
    }

    #[test]
    fn chain_no_creds_is_401_when_anonymous_disabled() {
        let chain = AuthenticatorChain::new()
            .with(Box::new(TokenAuthenticator::new().with_token("k", UserInfo::new("node"))))
            .allow_anonymous(false);
        let err = chain.authenticate(&req_no_auth()).expect_err("401");
        assert_eq!(err.code, 401);
    }

    #[test]
    fn chain_invalid_token_short_circuits_401() {
        let chain = AuthenticatorChain::new()
            .with(Box::new(TokenAuthenticator::new().with_token("k", UserInfo::new("node"))))
            .allow_anonymous(true);
        // Even with anonymous enabled, a *presented* bad token is a hard 401.
        let err = chain.authenticate(&req_with_auth("Bearer wrong")).expect_err("401");
        assert_eq!(err.code, 401);
    }

    // --- implicit system:authenticated group --------------------------------
    // Upstream's AuthenticatedGroupAdder appends `system:authenticated` to the
    // groups of every successfully-authenticated identity, so bindings to that
    // built-in group grant all logged-in users.

    #[test]
    fn chain_adds_system_authenticated_group_to_authenticated_user() {
        let chain = AuthenticatorChain::new()
            .with(Box::new(TokenAuthenticator::new().with_token("k", UserInfo::new("alice"))));
        let user = chain.authenticate(&req_with_auth("Bearer k")).expect("authn");
        assert_eq!(user.name, "alice");
        assert!(
            user.groups.contains(&"system:authenticated".to_string()),
            "groups: {:?}",
            user.groups
        );
    }

    #[test]
    fn chain_preserves_authenticator_groups_and_appends_authenticated() {
        let chain = AuthenticatorChain::new().with(Box::new(
            TokenAuthenticator::new()
                .with_token("k", UserInfo::new("alice").with_groups(&["dev"])),
        ));
        let user = chain.authenticate(&req_with_auth("Bearer k")).expect("authn");
        assert!(user.groups.contains(&"dev".to_string()));
        assert!(user.groups.contains(&"system:authenticated".to_string()));
    }

    #[test]
    fn chain_does_not_duplicate_system_authenticated() {
        // An authenticator that already lists the group must not get a duplicate.
        let chain = AuthenticatorChain::new().with(Box::new(
            TokenAuthenticator::new().with_token(
                "k",
                UserInfo::new("alice").with_groups(&["system:authenticated"]),
            ),
        ));
        let user = chain.authenticate(&req_with_auth("Bearer k")).expect("authn");
        let count = user.groups.iter().filter(|g| *g == "system:authenticated").count();
        assert_eq!(count, 1, "groups: {:?}", user.groups);
    }

    #[test]
    fn chain_does_not_add_authenticated_to_anonymous() {
        // The anonymous fallthrough stays `system:unauthenticated` only.
        let chain = AuthenticatorChain::new()
            .with(Box::new(TokenAuthenticator::new().with_token("k", UserInfo::new("node"))))
            .allow_anonymous(true);
        let user = chain.authenticate(&req_no_auth()).expect("anon");
        assert_eq!(user.name, "system:anonymous");
        assert!(!user.groups.contains(&"system:authenticated".to_string()));
        assert!(user.groups.contains(&"system:unauthenticated".to_string()));
    }

    #[test]
    fn anonymous_authenticator_delegate_is_not_marked_authenticated() {
        // If AnonymousAuthenticator is wired as a delegate (it always returns an
        // identity), the system:anonymous user it yields must NOT be tagged
        // system:authenticated — it is recognised as the anonymous identity.
        let chain = AuthenticatorChain::new().with(Box::new(AnonymousAuthenticator));
        let user = chain.authenticate(&req_no_auth()).expect("anon");
        assert_eq!(user.name, "system:anonymous");
        assert!(!user.groups.contains(&"system:authenticated".to_string()));
    }
}
