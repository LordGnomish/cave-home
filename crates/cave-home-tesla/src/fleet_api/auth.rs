// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! OAuth2 Authorization-Code flow with PKCE (RFC 7636, S256) for the Tesla
//! Fleet API.
//!
//! Everything here is pure: the PKCE pair is derived from caller-supplied
//! entropy (no RNG baked in), the authorize-URL and token-request bodies are
//! built as strings, and token expiry is computed against a caller-supplied
//! Unix time. The loopback redirect capture and the actual HTTPS token POST are
//! the I/O the operational layer wires up (see `parity.manifest.toml`).

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 7636 Appendix B worked example.
    const RFC_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const RFC_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    #[test]
    fn pkce_matches_rfc7636_vector() {
        let pair = PkcePair::from_verifier(RFC_VERIFIER).unwrap();
        assert_eq!(pair.challenge(), RFC_CHALLENGE);
        assert_eq!(pair.method(), "S256");
    }

    #[test]
    fn pkce_generate_from_entropy_is_valid_and_deterministic() {
        let a = PkcePair::generate(&[0u8; 32]).unwrap();
        let b = PkcePair::generate(&[0u8; 32]).unwrap();
        assert_eq!(a.verifier(), b.verifier(), "same entropy -> same verifier");
        // 32 bytes of base64url(no pad) is 43 chars — the RFC minimum.
        assert_eq!(a.verifier().len(), 43);
        assert!(!a.challenge().is_empty());
    }

    #[test]
    fn pkce_rejects_short_verifier() {
        assert!(PkcePair::from_verifier("tooshort").is_err());
        assert!(PkcePair::generate(&[0u8; 8]).is_err());
    }

    #[test]
    fn pkce_rejects_non_unreserved_verifier() {
        let bad = "a".repeat(40) + "+/="; // 43 chars but illegal alphabet
        assert!(PkcePair::from_verifier(&bad).is_err());
    }

    #[test]
    fn authorize_url_carries_all_required_params() {
        let cfg = AuthConfig::new("cave-home-client", "https://localhost:8443/callback");
        let pkce = PkcePair::from_verifier(RFC_VERIFIER).unwrap();
        let url = authorize_url(&cfg, "xyz-state", &pkce);
        assert!(url.starts_with("https://auth.tesla.com/oauth2/v3/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=cave-home-client"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={RFC_CHALLENGE}")));
        assert!(url.contains("state=xyz-state"));
        // redirect_uri must be percent-encoded.
        assert!(url.contains("redirect_uri=https%3A%2F%2Flocalhost%3A8443%2Fcallback"));
        // scope must include the energy scopes (space -> %20).
        assert!(url.contains("scope="));
        assert!(url.contains("energy_device_data"));
        assert!(url.contains("offline_access"));
    }

    #[test]
    fn token_exchange_body_is_form_encoded() {
        let req = TokenExchange {
            client_id: "cid",
            client_secret: Some("sec"),
            code: "auth-code",
            redirect_uri: "https://localhost/cb",
            code_verifier: RFC_VERIFIER,
            audience: Some("https://fleet-api.prd.na.vn.cloud.tesla.com"),
        };
        let body = token_exchange_body(&req);
        assert!(body.contains("grant_type=authorization_code"));
        assert!(body.contains("client_id=cid"));
        assert!(body.contains("client_secret=sec"));
        assert!(body.contains("code=auth-code"));
        assert!(body.contains(&format!("code_verifier={RFC_VERIFIER}")));
        assert!(body.contains("redirect_uri=https%3A%2F%2Flocalhost%2Fcb"));
        assert!(body.contains("audience=https%3A%2F%2Ffleet-api"));
    }

    #[test]
    fn token_exchange_body_omits_absent_optionals() {
        let req = TokenExchange {
            client_id: "cid",
            client_secret: None,
            code: "c",
            redirect_uri: "https://x/cb",
            code_verifier: RFC_VERIFIER,
            audience: None,
        };
        let body = token_exchange_body(&req);
        assert!(!body.contains("client_secret="));
        assert!(!body.contains("audience="));
    }

    #[test]
    fn refresh_body_is_form_encoded() {
        let body = refresh_body("cid", Some("sec"), "refresh-123");
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains("client_id=cid"));
        assert!(body.contains("client_secret=sec"));
        assert!(body.contains("refresh_token=refresh-123"));
    }

    #[test]
    fn parse_token_response_reads_fields() {
        let json = r#"{
            "access_token": "AT-abc",
            "refresh_token": "RT-xyz",
            "expires_in": 28800,
            "token_type": "Bearer"
        }"#;
        let resp = parse_token_response(json).unwrap();
        assert_eq!(resp.access_token, "AT-abc");
        assert_eq!(resp.refresh_token.as_deref(), Some("RT-xyz"));
        assert_eq!(resp.expires_in, 28800);
    }

    #[test]
    fn token_set_tracks_expiry_against_supplied_clock() {
        let resp = parse_token_response(
            r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600,"token_type":"Bearer"}"#,
        )
        .unwrap();
        let set = TokenSet::from_response(&resp, 1_000);
        assert_eq!(set.expires_at_unix(), 4_600);
        assert!(!set.is_expired(2_000, 60));
        // Within the skew window it is treated as expired.
        assert!(set.is_expired(4_560, 60));
        assert!(set.is_expired(5_000, 0));
    }

    #[test]
    fn default_scopes_cover_energy_read_and_command() {
        let cfg = AuthConfig::new("c", "https://x/cb");
        assert!(cfg.scopes.iter().any(|s| s == "energy_device_data"));
        assert!(cfg.scopes.iter().any(|s| s == "energy_cmds"));
        assert!(cfg.scopes.iter().any(|s| s == "offline_access"));
    }
}
