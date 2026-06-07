// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `OAuth2` Authorization-Code flow with PKCE (RFC 7636, S256) for the Tesla
//! Fleet API.
//!
//! Everything here is pure: the PKCE pair is derived from caller-supplied
//! entropy (no RNG baked in), the authorize-URL and token-request bodies are
//! built as strings, and token expiry is computed against a caller-supplied
//! Unix time. The loopback redirect capture and the actual HTTPS token POST are
//! the I/O the operational layer wires up (see `parity.manifest.toml`).

use serde::Deserialize;

use crate::crypto::{base64url_nopad, percent_encode, sha256};
use crate::error::{Result, TeslaError};

/// The documented Tesla `OAuth2` authorize endpoint.
pub const AUTHORIZE_URL: &str = "https://auth.tesla.com/oauth2/v3/authorize";
/// The documented Tesla `OAuth2` token endpoint.
pub const TOKEN_URL: &str = "https://auth.tesla.com/oauth2/v3/token";

/// The default scopes cave-home requests: identity, offline refresh, energy
/// read and energy command. (`openid` is required for an id-token; the rest are
/// the Fleet API energy product scopes.)
pub const DEFAULT_SCOPES: [&str; 4] =
    ["openid", "offline_access", "energy_device_data", "energy_cmds"];

/// A PKCE verifier/challenge pair (RFC 7636, S256).
#[derive(Debug, Clone)]
pub struct PkcePair {
    verifier: String,
    challenge: String,
}

impl PkcePair {
    /// Build a pair from an existing code verifier, validating it against the
    /// RFC 7636 §4.1 grammar (43–128 chars from the unreserved set) and
    /// deriving the S256 challenge.
    ///
    /// # Errors
    /// [`TeslaError::Validation`] if the verifier is the wrong length or uses
    /// characters outside `ALPHA / DIGIT / - . _ ~`.
    pub fn from_verifier(verifier: &str) -> Result<Self> {
        let len = verifier.len();
        if !(43..=128).contains(&len) {
            return Err(TeslaError::Validation(format!(
                "PKCE verifier must be 43..=128 chars, got {len}"
            )));
        }
        if !verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
        {
            return Err(TeslaError::Validation(
                "PKCE verifier contains non-unreserved characters".into(),
            ));
        }
        let challenge = base64url_nopad(&sha256(verifier.as_bytes()));
        Ok(Self {
            verifier: verifier.to_string(),
            challenge,
        })
    }

    /// Derive a pair from caller-supplied entropy. The verifier is the
    /// URL-safe base64 (no padding) of the entropy, which is by construction in
    /// the unreserved set; at least 32 bytes are required to reach the 43-char
    /// RFC minimum.
    ///
    /// # Errors
    /// [`TeslaError::Validation`] if fewer than 32 entropy bytes are supplied.
    pub fn generate(entropy: &[u8]) -> Result<Self> {
        if entropy.len() < 32 {
            return Err(TeslaError::Validation(format!(
                "PKCE entropy must be >= 32 bytes, got {}",
                entropy.len()
            )));
        }
        Self::from_verifier(&base64url_nopad(entropy))
    }

    /// The code verifier (kept by the client, sent on token exchange).
    #[must_use]
    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    /// The S256 code challenge (sent on the authorize redirect).
    #[must_use]
    pub fn challenge(&self) -> &str {
        &self.challenge
    }

    /// The PKCE method identifier — always `S256`.
    #[must_use]
    pub const fn method(&self) -> &'static str {
        "S256"
    }
}

/// The static parameters of an authorize request.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// The registered `OAuth2` client id.
    pub client_id: String,
    /// The registered redirect URI (loopback for the desktop/CLI flow).
    pub redirect_uri: String,
    /// The requested scopes (defaults to [`DEFAULT_SCOPES`]).
    pub scopes: Vec<String>,
}

impl AuthConfig {
    /// A config with the default energy scopes.
    #[must_use]
    pub fn new(client_id: impl Into<String>, redirect_uri: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            redirect_uri: redirect_uri.into(),
            scopes: DEFAULT_SCOPES.iter().map(|s| (*s).to_string()).collect(),
        }
    }
}

/// Build the `OAuth2` authorize URL the user opens in a browser.
#[must_use]
pub fn authorize_url(cfg: &AuthConfig, state: &str, pkce: &PkcePair) -> String {
    let scope = cfg.scopes.join(" ");
    let params = [
        ("response_type", "code"),
        ("client_id", cfg.client_id.as_str()),
        ("redirect_uri", cfg.redirect_uri.as_str()),
        ("scope", scope.as_str()),
        ("state", state),
        ("code_challenge", pkce.challenge()),
        ("code_challenge_method", pkce.method()),
    ];
    let query = encode_form(&params);
    format!("{AUTHORIZE_URL}?{query}")
}

/// The fields of an `authorization_code` token exchange.
#[derive(Debug, Clone)]
pub struct TokenExchange<'a> {
    /// The `OAuth2` client id.
    pub client_id: &'a str,
    /// The client secret, for confidential clients (Fleet API). `None` for a
    /// pure public PKCE client.
    pub client_secret: Option<&'a str>,
    /// The authorization code captured from the redirect.
    pub code: &'a str,
    /// The redirect URI used in the authorize request (must match).
    pub redirect_uri: &'a str,
    /// The PKCE code verifier.
    pub code_verifier: &'a str,
    /// The regional Fleet API audience, when the provider requires it.
    pub audience: Option<&'a str>,
}

/// Build the `application/x-www-form-urlencoded` body for an authorization-code
/// exchange.
#[must_use]
pub fn token_exchange_body(req: &TokenExchange) -> String {
    let mut params: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("client_id", req.client_id),
        ("code", req.code),
        ("redirect_uri", req.redirect_uri),
        ("code_verifier", req.code_verifier),
    ];
    if let Some(secret) = req.client_secret {
        params.push(("client_secret", secret));
    }
    if let Some(aud) = req.audience {
        params.push(("audience", aud));
    }
    encode_form(&params)
}

/// Build the form body for a `refresh_token` grant.
#[must_use]
pub fn refresh_body(client_id: &str, client_secret: Option<&str>, refresh_token: &str) -> String {
    let mut params: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
    ];
    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }
    encode_form(&params)
}

/// The token endpoint's JSON response.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    /// The bearer access token.
    pub access_token: String,
    /// The refresh token (present when `offline_access` was granted).
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Access-token lifetime in seconds.
    pub expires_in: u64,
    /// The token type (`Bearer`).
    #[serde(default)]
    pub token_type: String,
    /// The `OpenID` id-token, when `openid` was requested.
    #[serde(default)]
    pub id_token: Option<String>,
}

/// Parse a token endpoint response body.
///
/// # Errors
/// [`TeslaError::Decode`] if the body is not the expected JSON shape.
pub fn parse_token_response(body: &str) -> Result<TokenResponse> {
    Ok(serde_json::from_str(body)?)
}

/// A held token with the wall-clock time it was obtained, so expiry can be
/// computed without the token itself carrying an absolute timestamp.
#[derive(Debug, Clone)]
pub struct TokenSet {
    /// The bearer access token.
    pub access_token: String,
    /// The refresh token, if one was issued.
    pub refresh_token: Option<String>,
    /// Unix seconds at which the token was obtained.
    pub obtained_at_unix: u64,
    /// Lifetime in seconds from `obtained_at_unix`.
    pub expires_in: u64,
}

impl TokenSet {
    /// Build from a [`TokenResponse`] and the Unix time it was received.
    #[must_use]
    pub fn from_response(resp: &TokenResponse, now_unix: u64) -> Self {
        Self {
            access_token: resp.access_token.clone(),
            refresh_token: resp.refresh_token.clone(),
            obtained_at_unix: now_unix,
            expires_in: resp.expires_in,
        }
    }

    /// The Unix second at which this token expires.
    #[must_use]
    pub const fn expires_at_unix(&self) -> u64 {
        self.obtained_at_unix.saturating_add(self.expires_in)
    }

    /// Whether the token is expired at `now_unix`, treating anything within
    /// `skew_secs` of expiry as already expired (so a refresh is triggered
    /// before the token actually lapses mid-request).
    #[must_use]
    pub const fn is_expired(&self, now_unix: u64, skew_secs: u64) -> bool {
        now_unix.saturating_add(skew_secs) >= self.expires_at_unix()
    }
}

/// Encode a list of key/value pairs as `application/x-www-form-urlencoded`
/// (also the query-string format), percent-encoding both sides.
fn encode_form(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

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
