// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Connection configuration and SysAP URL derivation.

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
        assert_eq!(c.auth().basic_auth_header_value(), Some("Basic dXNlcjpwYXNz".to_string()));
    }
}
