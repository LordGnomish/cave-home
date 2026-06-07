// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Authentication for the SysAP local API.

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
