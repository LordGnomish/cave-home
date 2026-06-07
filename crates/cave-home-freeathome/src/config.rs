// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Connection configuration and SysAP URL derivation.
//!
//! The System Access Point exposes its local API under a fixed prefix:
//! - REST: `https://<host>/fhapi/v1/api/rest`
//! - WebSocket: `wss://<host>/fhapi/v1/api/ws`
//!
//! The host is normalised: any `http(s)://` scheme prefix and trailing slash are
//! stripped so callers may pass either a bare IP/hostname or a full URL.

use crate::auth::AuthMethod;

/// Everything needed to reach one SysAP.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    host: String,
    auth: AuthMethod,
    insecure_tls: bool,
}

impl ClientConfig {
    /// Build a config for `host` (bare host or full URL) with `auth`.
    pub fn new(host: impl AsRef<str>, auth: AuthMethod) -> Self {
        Self {
            host: normalise_host(host.as_ref()),
            auth,
            insecure_tls: false,
        }
    }

    /// Accept a self-signed SysAP certificate (LAN deployments ship one).
    ///
    /// Off by default; pinning/proper trust is the secure path.
    #[must_use]
    pub const fn with_insecure_tls(mut self, insecure: bool) -> Self {
        self.insecure_tls = insecure;
        self
    }

    /// The normalised host (no scheme, no trailing slash).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The authentication method.
    pub const fn auth(&self) -> &AuthMethod {
        &self.auth
    }

    /// Whether a self-signed certificate is accepted.
    pub const fn insecure_tls(&self) -> bool {
        self.insecure_tls
    }

    /// The REST API base URL, e.g. `https://<host>/fhapi/v1/api/rest`.
    pub fn rest_base_url(&self) -> String {
        format!("https://{}/fhapi/v1/api/rest", self.host)
    }

    /// The WebSocket URL, e.g. `wss://<host>/fhapi/v1/api/ws`.
    pub fn ws_url(&self) -> String {
        format!("wss://{}/fhapi/v1/api/ws", self.host)
    }
}

/// Strip a leading `http(s)://` scheme and any trailing slash.
fn normalise_host(raw: &str) -> String {
    let no_scheme = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);
    no_scheme.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthMethod;

    fn cfg(host: &str) -> ClientConfig {
        ClientConfig::new(host, AuthMethod::basic("u", "p"))
    }

    #[test]
    fn rest_base_url_built() {
        assert_eq!(
            cfg("192.168.1.10").rest_base_url(),
            "https://192.168.1.10/fhapi/v1/api/rest"
        );
    }

    #[test]
    fn ws_url_built() {
        assert_eq!(
            cfg("192.168.1.10").ws_url(),
            "wss://192.168.1.10/fhapi/v1/api/ws"
        );
    }

    #[test]
    fn host_strips_https_scheme() {
        assert_eq!(cfg("https://sysap.local").host(), "sysap.local");
    }

    #[test]
    fn host_strips_trailing_slash() {
        assert_eq!(cfg("sysap.local/").host(), "sysap.local");
    }

    #[test]
    fn insecure_tls_default_false_with_setter() {
        let c = cfg("h");
        assert!(!c.insecure_tls());
        assert!(c.with_insecure_tls(true).insecure_tls());
    }

    #[test]
    fn auth_is_accessible() {
        let c = cfg("h");
        // cfg() uses AuthMethod::basic("u", "p") → base64("u:p") == "dTpw".
        assert_eq!(
            c.auth().basic_auth_header_value(),
            Some("Basic dTpw".to_string())
        );
    }

    #[test]
    fn origin_override_drives_rest_base() {
        let c = cfg("ignored.host").with_origin("http://127.0.0.1:8080");
        assert_eq!(c.rest_base_url(), "http://127.0.0.1:8080/fhapi/v1/api/rest");
    }

    #[test]
    fn origin_override_derives_ws_scheme_from_http() {
        let c = cfg("ignored.host").with_origin("http://127.0.0.1:8080");
        assert_eq!(c.ws_url(), "ws://127.0.0.1:8080/fhapi/v1/api/ws");
    }

    #[test]
    fn origin_override_derives_wss_from_https() {
        let c = cfg("ignored.host").with_origin("https://sysap.lan:443");
        assert_eq!(c.ws_url(), "wss://sysap.lan:443/fhapi/v1/api/ws");
    }

    #[test]
    fn origin_override_trims_trailing_slash() {
        let c = cfg("h").with_origin("http://127.0.0.1:9/");
        assert_eq!(c.rest_base_url(), "http://127.0.0.1:9/fhapi/v1/api/rest");
    }

    #[test]
    fn no_origin_keeps_https_default() {
        assert_eq!(
            cfg("sysap.lan").rest_base_url(),
            "https://sysap.lan/fhapi/v1/api/rest"
        );
        assert_eq!(cfg("sysap.lan").ws_url(), "wss://sysap.lan/fhapi/v1/api/ws");
    }
}
