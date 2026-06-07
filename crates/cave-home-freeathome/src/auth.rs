// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Authentication for the SysAP local API.
//!
//! The System Access Point's `fhapi` endpoint accepts HTTP Basic credentials
//! (the user/password configured in the SysAP, e.g. the `installer` account).
//! A client-certificate / mTLS path is modelled here so the transport layer can
//! select it later; only the Basic path produces an `Authorization` header.

use std::path::{Path, PathBuf};

use base64::Engine as _;

/// A username + password pair for HTTP Basic authentication.
#[derive(Debug, Clone)]
pub struct Credentials {
    username: String,
    password: String,
}

impl Credentials {
    /// Build credentials from a username and password.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// The configured username.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// The full `Authorization` header value, e.g. `"Basic dXNlcjpwYXNz"`.
    pub fn basic_auth_header_value(&self) -> String {
        let raw = format!("{}:{}", self.username, self.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        format!("Basic {encoded}")
    }
}

/// Paths to a client certificate and its private key (for future mTLS).
#[derive(Debug, Clone)]
pub struct ClientCertConfig {
    cert_path: PathBuf,
    key_path: PathBuf,
}

impl ClientCertConfig {
    /// Build a client-cert config from a certificate and key path.
    pub fn new(cert_path: impl Into<PathBuf>, key_path: impl Into<PathBuf>) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
        }
    }

    /// The certificate path.
    pub fn cert_path(&self) -> &Path {
        &self.cert_path
    }

    /// The private-key path.
    pub fn key_path(&self) -> &Path {
        &self.key_path
    }
}

/// How the client authenticates to the SysAP.
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// HTTP Basic authentication.
    Basic(Credentials),
    /// Client-certificate / mTLS authentication.
    ClientCert(ClientCertConfig),
}

impl AuthMethod {
    /// Convenience constructor for HTTP Basic.
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Basic(Credentials::new(username, password))
    }

    /// The `Authorization` header value for Basic auth, or `None` for cert auth.
    pub fn basic_auth_header_value(&self) -> Option<String> {
        match self {
            Self::Basic(c) => Some(c.basic_auth_header_value()),
            Self::ClientCert(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_header_known_vector() {
        let c = Credentials::new("user", "pass");
        assert_eq!(c.basic_auth_header_value(), "Basic dXNlcjpwYXNz");
    }

    #[test]
    fn basic_header_empty_password() {
        let c = Credentials::new("installer", "");
        assert_eq!(c.basic_auth_header_value(), "Basic aW5zdGFsbGVyOg==");
    }

    #[test]
    fn username_accessor() {
        let c = Credentials::new("admin", "secret");
        assert_eq!(c.username(), "admin");
    }

    #[test]
    fn auth_method_basic_variant() {
        let m = AuthMethod::basic("u", "p");
        assert!(matches!(m, AuthMethod::Basic(_)));
    }

    #[test]
    fn auth_method_exposes_basic_header() {
        let m = AuthMethod::basic("user", "pass");
        assert_eq!(m.basic_auth_header_value(), Some("Basic dXNlcjpwYXNz".to_string()));
    }

    #[test]
    fn client_cert_config_holds_paths() {
        let cc = ClientCertConfig::new("/tmp/c.pem", "/tmp/k.pem");
        assert_eq!(cc.cert_path().to_str(), Some("/tmp/c.pem"));
        assert_eq!(cc.key_path().to_str(), Some("/tmp/k.pem"));
    }

    #[test]
    fn client_cert_method_has_no_basic_header() {
        let m = AuthMethod::ClientCert(ClientCertConfig::new("/c.pem", "/k.pem"));
        assert_eq!(m.basic_auth_header_value(), None);
    }
}
