// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Authentication: credentials, the login request body, and the live session.
//!
//! UniFi consoles authenticate one of two ways:
//!
//! - **Username + password** ([`Credentials::Password`]). A POST to the
//!   console's login path returns a session **cookie** (`TOKEN` on UniFi OS,
//!   `unifises` on a legacy controller) and a **CSRF token**. The CSRF token
//!   arrives either in the `x-csrf-token` *response header*, in a `csrf_token`
//!   *cookie*, or — on UniFi OS — encoded inside the `TOKEN` cookie's JWT
//!   payload (`csrfToken` claim). Every subsequent mutating request must echo
//!   the cookie **and** the CSRF token, or the console answers 403.
//! - **API key** ([`Credentials::ApiKey`]). Newer UniFi OS / Network builds
//!   issue a long-lived key sent as the `X-API-KEY` header on every request,
//!   with no login round-trip and no CSRF dance.
//!
//! [`Session`] is the mutable auth state: it captures cookies + CSRF from a
//! login (or any) response and applies them to outgoing requests. It is the
//! only place the cookie/CSRF/api-key wire details live.

use std::collections::BTreeMap;

use base64::Engine as _;
use serde_json::json;

use crate::console::Console;
use crate::transport::{HttpMethod, HttpRequest};

/// How to authenticate to a console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credentials {
    /// Username + password (the classic local-admin login). `remember` asks the
    /// console for a longer-lived session cookie.
    Password {
        /// The local account username.
        username: String,
        /// The account password.
        password: String,
        /// Whether to request a remembered (longer-lived) session.
        remember: bool,
    },
    /// A pre-issued API key sent as `X-API-KEY` (no login round-trip).
    ApiKey(String),
}

impl Credentials {
    /// Username + password, remembered by default.
    #[must_use]
    pub fn password(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Password {
            username: username.into(),
            password: password.into(),
            remember: true,
        }
    }

    /// An API key.
    #[must_use]
    pub fn api_key(key: impl Into<String>) -> Self {
        Self::ApiKey(key.into())
    }

    /// Whether this credential authenticates by API key (and so needs no login
    /// POST).
    #[must_use]
    pub fn is_api_key(&self) -> bool {
        matches!(self, Self::ApiKey(_))
    }

    /// Build the login request for a password credential against `console`.
    ///
    /// Returns `None` for an API-key credential (there is nothing to POST).
    ///
    /// # Errors
    /// Fails if the JSON body cannot be serialized (it always can in practice).
    pub fn login_request(&self, console: &Console) -> crate::Result<Option<HttpRequest>> {
        let Self::Password {
            username,
            password,
            remember,
        } = self
        else {
            return Ok(None);
        };
        let body = json!({
            "username": username,
            "password": password,
            "remember": remember,
            // Legacy controllers historically also accept `strict`; UniFi OS
            // ignores the extra field, so sending it is safe for both.
            "strict": true,
        });
        let req = HttpRequest::new(HttpMethod::Post, console.login_url())
            .header("Accept", "application/json")
            .json(&body)?;
        Ok(Some(req))
    }
}

/// Parse a single `Set-Cookie` header value into its `(name, value)` pair,
/// discarding the attributes (`Path`, `HttpOnly`, `Max-Age`, …).
#[must_use]
pub fn parse_set_cookie(header: &str) -> Option<(String, String)> {
    let first = header.split(';').next()?.trim();
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), value.trim().to_string()))
}

/// Extract the `csrfToken` claim from a UniFi OS `TOKEN` JWT, if present.
///
/// A JWT is `header.payload.signature`; the payload is base64url (no padding)
/// JSON. UniFi OS embeds the CSRF token there as `csrfToken`.
#[must_use]
pub fn csrf_from_jwt(jwt: &str) -> Option<String> {
    let mut parts = jwt.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _sig = parts.next()?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("csrfToken")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

/// The live authentication state for a console connection.
#[derive(Debug, Clone, Default)]
pub struct Session {
    cookies: BTreeMap<String, String>,
    csrf: Option<String>,
    api_key: Option<String>,
}

impl Session {
    /// An empty, unauthenticated session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A session that authenticates by API key.
    #[must_use]
    pub fn from_api_key(key: impl Into<String>) -> Self {
        Self {
            api_key: Some(key.into()),
            ..Self::default()
        }
    }

    /// The names of the session cookies we currently hold (for diagnostics).
    #[must_use]
    pub fn cookie_names(&self) -> Vec<&str> {
        self.cookies.keys().map(String::as_str).collect()
    }

    /// The current CSRF token, if any.
    #[must_use]
    pub fn csrf(&self) -> Option<&str> {
        self.csrf.as_deref()
    }

    /// Whether this session can authenticate a request (API key, or a session
    /// cookie has been captured).
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.api_key.is_some()
            || self
                .cookies
                .keys()
                .any(|k| k == "TOKEN" || k == "unifises")
    }

    /// Ingest the auth-bearing headers of a response: every `set-cookie`, the
    /// `x-csrf-token` header, the `csrf_token` cookie, and (UniFi OS) the CSRF
    /// claim inside a `TOKEN` JWT. Returns `self`-mutating; later sources only
    /// overwrite the CSRF when they actually carry one.
    pub fn ingest_response_headers<'a>(
        &mut self,
        headers: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) {
        for (name, value) in headers {
            if name.eq_ignore_ascii_case("set-cookie") {
                if let Some((cname, cvalue)) = parse_set_cookie(value) {
                    if cname.eq_ignore_ascii_case("csrf_token") {
                        self.csrf = Some(cvalue.clone());
                    }
                    if cname == "TOKEN" {
                        if let Some(c) = csrf_from_jwt(&cvalue) {
                            self.csrf = Some(c);
                        }
                    }
                    self.cookies.insert(cname, cvalue);
                }
            } else if name.eq_ignore_ascii_case("x-csrf-token") && !value.is_empty() {
                self.csrf = Some(value.to_string());
            }
        }
    }

    /// The `Cookie` request-header value for the cookies we hold, or `None` if
    /// we hold none.
    #[must_use]
    pub fn cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        let joined = self
            .cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        Some(joined)
    }

    /// Apply this session's auth to an outgoing request: the cookie header, the
    /// `X-CSRF-Token` header (for mutating verbs), and `X-API-KEY` if keyed.
    #[must_use]
    pub fn authorize(&self, mut req: HttpRequest) -> HttpRequest {
        if let Some(key) = &self.api_key {
            req = req.header("X-API-KEY", key);
        }
        if let Some(cookie) = self.cookie_header() {
            req = req.header("Cookie", cookie);
        }
        if let Some(csrf) = &self.csrf {
            // The CSRF token is only meaningful for state-changing verbs, but
            // sending it on a GET is harmless and the console ignores it.
            req = req.header("X-CSRF-Token", csrf);
        }
        req
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_login_request_targets_console_login_path_with_json_body() {
        let console = Console::unifi_os("10.0.0.1");
        let creds = Credentials::password("admin", "s3cret");
        let req = creds.login_request(&console).unwrap().unwrap();
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "https://10.0.0.1:443/api/auth/login");
        assert_eq!(req.header_value("content-type"), Some("application/json"));
        let body: serde_json::Value =
            serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["username"], "admin");
        assert_eq!(body["password"], "s3cret");
        assert_eq!(body["remember"], true);
    }

    #[test]
    fn api_key_credential_has_no_login_request() {
        let console = Console::unifi_os("h");
        let creds = Credentials::api_key("KEY123");
        assert!(creds.is_api_key());
        assert!(creds.login_request(&console).unwrap().is_none());
    }

    #[test]
    fn parse_set_cookie_strips_attributes() {
        assert_eq!(
            parse_set_cookie("unifises=abc123; Path=/; HttpOnly; Secure"),
            Some(("unifises".to_string(), "abc123".to_string()))
        );
        assert_eq!(parse_set_cookie(""), None);
        assert_eq!(parse_set_cookie("=novalue"), None);
    }

    #[test]
    fn csrf_extracted_from_token_jwt_payload() {
        // header.{"csrfToken":"tok-xyz"}.sig  (payload base64url, no padding)
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"csrfToken":"tok-xyz","userId":"u1"}"#);
        let jwt = format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig");
        assert_eq!(csrf_from_jwt(&jwt), Some("tok-xyz".to_string()));
        assert_eq!(csrf_from_jwt("not-a-jwt"), None);
    }

    #[test]
    fn session_ingests_unifi_os_login_headers() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"csrfToken":"jwt-csrf"}"#);
        let token = format!("h.{payload}.s");
        let mut s = Session::new();
        assert!(!s.is_authenticated());
        s.ingest_response_headers([
            ("Set-Cookie", format!("TOKEN={token}; Path=/; HttpOnly").as_str()),
            ("x-csrf-token", "header-csrf"),
        ]);
        assert!(s.is_authenticated());
        // The explicit x-csrf-token header wins as the last source ingested.
        assert_eq!(s.csrf(), Some("header-csrf"));
        assert!(s.cookie_names().contains(&"TOKEN"));
    }

    #[test]
    fn session_csrf_from_jwt_when_no_header() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"csrfToken":"jwt-only"}"#);
        let token = format!("h.{payload}.s");
        let mut s = Session::new();
        s.ingest_response_headers([(
            "set-cookie",
            format!("TOKEN={token}; Path=/").as_str(),
        )]);
        assert_eq!(s.csrf(), Some("jwt-only"));
    }

    #[test]
    fn legacy_csrf_token_cookie_sets_csrf() {
        let mut s = Session::new();
        s.ingest_response_headers([
            ("Set-Cookie", "unifises=sess; Path=/"),
            ("Set-Cookie", "csrf_token=legacy-csrf; Path=/"),
        ]);
        assert!(s.is_authenticated());
        assert_eq!(s.csrf(), Some("legacy-csrf"));
    }

    #[test]
    fn authorize_attaches_cookie_and_csrf_headers() {
        let mut s = Session::new();
        s.ingest_response_headers([
            ("set-cookie", "unifises=sess; Path=/"),
            ("set-cookie", "csrf_token=cx; Path=/"),
        ]);
        let req = s.authorize(HttpRequest::new(HttpMethod::Post, "https://h/api/x"));
        let cookie = req.header_value("cookie").unwrap();
        assert!(cookie.contains("unifises=sess"));
        assert!(cookie.contains("csrf_token=cx"));
        assert_eq!(req.header_value("x-csrf-token"), Some("cx"));
    }

    #[test]
    fn authorize_api_key_sets_header_and_no_cookie() {
        let s = Session::from_api_key("KEY999");
        assert!(s.is_authenticated());
        let req = s.authorize(HttpRequest::new(HttpMethod::Get, "https://h/api/x"));
        assert_eq!(req.header_value("x-api-key"), Some("KEY999"));
        assert!(req.header_value("cookie").is_none());
    }
}
