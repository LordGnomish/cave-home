// SPDX-License-Identifier: Apache-2.0
//! HTTP Basic-auth enforcement (the runtime half of the `BasicAuth` middleware).
//!
//! Spec basis: Traefik's `BasicAuth` middleware checks the `Authorization`
//! header against `htpasswd`-style `user:hash` entries and, on failure, returns
//! 401 with a `WWW-Authenticate: Basic realm="…"` challenge.
//!
//! Supported password schemes: Apache `{SHA}` (base64 of SHA-1) and plaintext.
//! Unknown/again-unsupported schemes (bcrypt, apr1-MD5) safely *fail closed*.
//! Credential comparison is constant-time to avoid leaking the secret through
//! response timing.

use std::collections::HashMap;

use base64::Engine as _;
use sha1::{Digest, Sha1};
use subtle::ConstantTimeEq;

/// Decode a `Basic` `Authorization` header value into `(user, password)`.
#[must_use]
pub fn parse_basic_auth(header_value: &str) -> Option<(String, String)> {
    let token = header_value.strip_prefix("Basic ").or_else(|| header_value.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(token.trim()).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

/// Constant-time byte equality (length is allowed to leak).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Verify `plain` against an `htpasswd` hash field (constant-time).
#[must_use]
pub fn verify_password(plain: &str, htpasswd_hash: &str) -> bool {
    if let Some(b64) = htpasswd_hash.strip_prefix("{SHA}") {
        let Ok(expected) = base64::engine::general_purpose::STANDARD.decode(b64) else {
            return false;
        };
        let mut h = Sha1::new();
        h.update(plain.as_bytes());
        return ct_eq(&h.finalize(), &expected);
    }
    // bcrypt / apr1-MD5 / crypt are not supported offline: fail closed rather
    // than mis-compare a hashed field against the plaintext.
    if htpasswd_hash.starts_with('$') {
        return false;
    }
    ct_eq(plain.as_bytes(), htpasswd_hash.as_bytes())
}

/// The `WWW-Authenticate` challenge value for `realm`.
#[must_use]
pub fn challenge(realm: &str) -> String {
    format!("Basic realm=\"{realm}\"")
}

/// The outcome of a basic-auth check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthResult {
    /// Credentials matched a known user.
    Authorized(String),
    /// Missing or invalid credentials; respond 401 with this challenge value.
    Unauthorized(String),
}

/// A basic-auth checker built from `htpasswd` lines.
#[derive(Debug, Clone)]
pub struct BasicAuthChecker {
    realm: String,
    users: HashMap<String, String>,
}

impl BasicAuthChecker {
    /// Build from a realm and `user:hash` entries (e.g. htpasswd lines).
    /// Malformed entries (no colon) are ignored.
    #[must_use]
    pub fn new(realm: &str, entries: &[String]) -> Self {
        let mut users = HashMap::new();
        for entry in entries {
            if let Some((user, hash)) = entry.split_once(':') {
                users.insert(user.to_string(), hash.to_string());
            }
        }
        Self { realm: realm.to_string(), users }
    }

    /// Check an inbound `Authorization` header value (if any).
    #[must_use]
    pub fn check(&self, authorization: Option<&str>) -> AuthResult {
        let unauthorized = || AuthResult::Unauthorized(challenge(&self.realm));
        let Some((user, pass)) = authorization.and_then(parse_basic_auth) else {
            return unauthorized();
        };
        match self.users.get(&user) {
            Some(hash) if verify_password(&pass, hash) => AuthResult::Authorized(user),
            _ => unauthorized(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha_entry(user: &str, password: &str) -> String {
        let mut h = Sha1::new();
        h.update(password.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(h.finalize());
        format!("{user}:{{SHA}}{b64}")
    }

    fn basic_header(user: &str, password: &str) -> String {
        let token =
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
        format!("Basic {token}")
    }

    #[test]
    fn parses_basic_credentials() {
        let h = basic_header("user", "pass");
        assert_eq!(parse_basic_auth(&h), Some(("user".to_string(), "pass".to_string())));
    }

    #[test]
    fn rejects_non_basic_scheme() {
        assert_eq!(parse_basic_auth("Bearer abc"), None);
        assert_eq!(parse_basic_auth("Basic !!!notbase64"), None);
    }

    #[test]
    fn verify_plaintext_password() {
        assert!(verify_password("hunter2", "hunter2"));
        assert!(!verify_password("hunter2", "wrong"));
    }

    #[test]
    fn verify_sha_password() {
        let entry = sha_entry("bob", "s3cret");
        let hash = entry.split_once(':').unwrap().1;
        assert!(verify_password("s3cret", hash));
        assert!(!verify_password("nope", hash));
    }

    #[test]
    fn unknown_scheme_fails_closed() {
        assert!(!verify_password("x", "$2y$10$somethingbcryptish"));
        assert!(!verify_password("x", "$apr1$abc$def"));
    }

    #[test]
    fn checker_authorizes_valid_user() {
        let checker = BasicAuthChecker::new("cave", &[sha_entry("alice", "open")]);
        let res = checker.check(Some(&basic_header("alice", "open")));
        assert_eq!(res, AuthResult::Authorized("alice".to_string()));
    }

    #[test]
    fn checker_rejects_bad_password_and_missing_header() {
        let checker = BasicAuthChecker::new("cave", &[sha_entry("alice", "open")]);
        assert!(matches!(
            checker.check(Some(&basic_header("alice", "wrong"))),
            AuthResult::Unauthorized(_)
        ));
        match checker.check(None) {
            AuthResult::Unauthorized(ch) => assert!(ch.contains("realm=\"cave\"")),
            AuthResult::Authorized(_) => panic!("should be unauthorized"),
        }
    }
}
