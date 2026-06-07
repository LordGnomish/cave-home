// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! End-to-end OAuth2-PKCE flow against the public crate surface.

use cave_home_tesla::fleet_api::auth::{
    authorize_url, parse_token_response, refresh_body, token_exchange_body, AuthConfig, PkcePair,
    TokenExchange, TokenSet,
};

// RFC 7636 Appendix B worked example.
const RFC_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const RFC_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

#[test]
fn full_pkce_authorize_then_exchange() {
    let pkce = PkcePair::from_verifier(RFC_VERIFIER).unwrap();
    assert_eq!(pkce.challenge(), RFC_CHALLENGE);

    let cfg = AuthConfig::new("cave-home-energy", "https://localhost:8443/callback");
    let url = authorize_url(&cfg, "state-123", &pkce);
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains(&format!("code_challenge={RFC_CHALLENGE}")));

    // The browser redirects back with ?code=...; exchange it.
    let body = token_exchange_body(&TokenExchange {
        client_id: "cave-home-energy",
        client_secret: Some("secret"),
        code: "captured-code",
        redirect_uri: "https://localhost:8443/callback",
        code_verifier: pkce.verifier(),
        audience: Some("https://fleet-api.prd.eu.vn.cloud.tesla.com"),
    });
    assert!(body.contains("grant_type=authorization_code"));
    assert!(body.contains(&format!("code_verifier={RFC_VERIFIER}")));

    let resp = parse_token_response(
        r#"{"access_token":"AT","refresh_token":"RT","expires_in":28800,"token_type":"Bearer"}"#,
    )
    .unwrap();
    let set = TokenSet::from_response(&resp, 1_000);
    assert_eq!(set.expires_at_unix(), 29_800);
    assert!(!set.is_expired(2_000, 60));
}

#[test]
fn refresh_round_uses_stored_refresh_token() {
    let body = refresh_body("cave-home-energy", Some("secret"), "RT-stored");
    assert!(body.contains("grant_type=refresh_token"));
    assert!(body.contains("refresh_token=RT-stored"));
}

#[test]
fn generated_pkce_is_rfc_compliant() {
    let pair = PkcePair::generate(&[7u8; 48]).unwrap();
    assert!(pair.verifier().len() >= 43);
    assert!(!pair.challenge().is_empty());
}
