// SPDX-License-Identifier: Apache-2.0
//! ACME (RFC 8555) client for automatic Let's Encrypt certificates.
//!
//! Spec basis: Traefik's ACME certificate resolver obtains and renews
//! certificates over the ACME protocol using the HTTP-01 challenge. This
//! implements the protocol's crypto and order state machine — ES256 account
//! keys, JWS request signing, the RFC 7638 JWK thumbprint, the HTTP-01 key
//! authorization, and the new-order → authorize → finalize → download flow —
//! over an injectable [`AcmeTransport`] seam so the whole issuance is testable
//! against a mock ACME server without a network.
//!
//! The production transport (a hyper client over `application/jose+json`) is the
//! one remaining I/O adapter; everything it drives lives here and is exercised
//! by the mock-issuance test.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// An ACME error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcmeError {
    /// The transport (network) layer failed.
    Transport(String),
    /// The ACME server returned an unexpected / error response.
    Protocol(String),
    /// A JSON body could not be parsed.
    Json(String),
    /// A cryptographic operation failed.
    Crypto(String),
}

impl std::fmt::Display for AcmeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "acme transport error: {e}"),
            Self::Protocol(e) => write!(f, "acme protocol error: {e}"),
            Self::Json(e) => write!(f, "acme json error: {e}"),
            Self::Crypto(e) => write!(f, "acme crypto error: {e}"),
        }
    }
}

impl std::error::Error for AcmeError {}

/// A minimal HTTP response as seen by the ACME client.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers (name lower-cased by convention).
    pub headers: Vec<(String, String)>,
    /// Raw body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Look up a header (case-insensitively).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// The injectable ACME transport seam. Production wires this to a hyper client
/// posting `application/jose+json`; tests wire it to a mock ACME server.
pub trait AcmeTransport {
    /// Perform an HTTP request. `body` is `None` for GET/HEAD.
    ///
    /// # Errors
    /// [`AcmeError::Transport`] on a transport-level failure.
    fn request(&self, method: &str, url: &str, body: Option<&[u8]>)
        -> Result<HttpResponse, AcmeError>;
}

/// The ACME directory (entry-point URLs).
#[derive(Debug, Clone, Deserialize)]
pub struct Directory {
    /// `newNonce` endpoint.
    #[serde(rename = "newNonce")]
    pub new_nonce: String,
    /// `newAccount` endpoint.
    #[serde(rename = "newAccount")]
    pub new_account: String,
    /// `newOrder` endpoint.
    #[serde(rename = "newOrder")]
    pub new_order: String,
}

/// An ACME order resource.
#[derive(Debug, Clone, Deserialize)]
pub struct Order {
    /// Order status (`pending` / `ready` / `processing` / `valid` / `invalid`).
    pub status: String,
    /// Authorization URLs for the order's identifiers.
    #[serde(default)]
    pub authorizations: Vec<String>,
    /// Finalize URL (POST the CSR here).
    #[serde(default)]
    pub finalize: String,
    /// Certificate URL, present once the order is `valid`.
    #[serde(default)]
    pub certificate: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Authorization {
    status: String,
    challenges: Vec<Challenge>,
}

#[derive(Debug, Clone, Deserialize)]
struct Challenge {
    #[serde(rename = "type")]
    kind: String,
    url: String,
    token: String,
}

/// An ECDSA P-256 ACME account key.
pub struct AcmeAccountKey {
    key_pair: EcdsaKeyPair,
    pkcs8: Vec<u8>,
    rng: SystemRandom,
}

impl AcmeAccountKey {
    /// Generate a fresh account key.
    ///
    /// # Errors
    /// [`AcmeError::Crypto`] if key generation fails.
    pub fn generate() -> Result<Self, AcmeError> {
        unimplemented!()
    }

    /// Reconstruct an account key from its PKCS#8 bytes.
    ///
    /// # Errors
    /// [`AcmeError::Crypto`] if the bytes are not a valid P-256 PKCS#8 key.
    pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, AcmeError> {
        unimplemented!()
    }

    /// The PKCS#8 serialization (for persistence).
    #[must_use]
    pub fn pkcs8_bytes(&self) -> &[u8] {
        &self.pkcs8
    }

    /// The public JWK (`kty=EC, crv=P-256, x, y`) as a JSON value.
    #[must_use]
    pub fn jwk(&self) -> serde_json::Value {
        unimplemented!()
    }

    /// The RFC 7638 JWK thumbprint: base64url(SHA-256(canonical JWK)).
    #[must_use]
    pub fn thumbprint(&self) -> String {
        unimplemented!()
    }

    /// ES256-sign `message`, returning the raw 64-byte `r‖s` signature.
    ///
    /// # Errors
    /// [`AcmeError::Crypto`] if signing fails.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, AcmeError> {
        unimplemented!()
    }
}

/// The HTTP-01 key authorization for `token`: `token.thumbprint`.
#[must_use]
pub fn key_authorization(token: &str, account_key: &AcmeAccountKey) -> String {
    unimplemented!()
}

/// Whether a certificate expiring at `not_after_unix` should be renewed now,
/// given the current time and how long before expiry to renew (Traefik default:
/// 30 days).
#[must_use]
pub fn needs_renewal(not_after_unix: u64, now_unix: u64, renew_before_secs: u64) -> bool {
    unimplemented!()
}

/// An ACME client driving issuance over a transport.
pub struct AcmeClient<T: AcmeTransport> {
    transport: T,
    directory: Directory,
    account_key: AcmeAccountKey,
    account_url: Option<String>,
    nonce: Option<String>,
}

impl<T: AcmeTransport> AcmeClient<T> {
    /// Create a client for a known directory and account key.
    #[must_use]
    pub fn new(transport: T, directory: Directory, account_key: AcmeAccountKey) -> Self {
        Self { transport, directory, account_key, account_url: None, nonce: None }
    }

    /// Run a full HTTP-01 issuance for `domains`, posting `csr_der` at finalize,
    /// and return the issued certificate chain PEM.
    ///
    /// The HTTP-01 challenge tokens must be served by the caller (the proxy's
    /// `/.well-known/acme-challenge/` handler) before validation; see
    /// [`key_authorization`].
    ///
    /// # Errors
    /// [`AcmeError`] on any transport or protocol failure.
    pub fn obtain_certificate(
        &mut self,
        domains: &[String],
        csr_der: &[u8],
    ) -> Result<String, AcmeError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn account_thumbprint_is_stable_across_reload() {
        let key = AcmeAccountKey::generate().unwrap();
        let t1 = key.thumbprint();
        let t2 = key.thumbprint();
        assert_eq!(t1, t2);
        assert!(!t1.is_empty());
        let reloaded = AcmeAccountKey::from_pkcs8(key.pkcs8_bytes()).unwrap();
        assert_eq!(reloaded.thumbprint(), t1);
    }

    #[test]
    fn jwk_is_ec_p256() {
        let key = AcmeAccountKey::generate().unwrap();
        let jwk = key.jwk();
        assert_eq!(jwk["kty"], "EC");
        assert_eq!(jwk["crv"], "P-256");
        assert!(jwk["x"].as_str().is_some());
        assert!(jwk["y"].as_str().is_some());
    }

    #[test]
    fn es256_signature_is_64_bytes() {
        let key = AcmeAccountKey::generate().unwrap();
        let sig = key.sign(b"hello").unwrap();
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn key_authorization_is_token_dot_thumbprint() {
        let key = AcmeAccountKey::generate().unwrap();
        let ka = key_authorization("tok123", &key);
        assert_eq!(ka, format!("tok123.{}", key.thumbprint()));
    }

    #[test]
    fn renewal_threshold() {
        // expires in 60 days, renew within 30 -> not yet
        assert!(!needs_renewal(60 * 86400, 0, 30 * 86400));
        // expires in 20 days, renew within 30 -> renew now
        assert!(needs_renewal(20 * 86400, 0, 30 * 86400));
        // already expired -> renew
        assert!(needs_renewal(100, 200, 30 * 86400));
    }

    /// A mock ACME server walking the full HTTP-01 issuance.
    struct MockAcme {
        authz_polls: RefCell<u32>,
        order_polls: RefCell<u32>,
        seen_finalize_csr: RefCell<bool>,
    }

    impl MockAcme {
        fn new() -> Self {
            Self {
                authz_polls: RefCell::new(0),
                order_polls: RefCell::new(0),
                seen_finalize_csr: RefCell::new(false),
            }
        }
        fn resp(status: u16, headers: &[(&str, &str)], body: &str) -> HttpResponse {
            HttpResponse {
                status,
                headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                body: body.as_bytes().to_vec(),
            }
        }
    }

    impl AcmeTransport for MockAcme {
        fn request(
            &self,
            _method: &str,
            url: &str,
            body: Option<&[u8]>,
        ) -> Result<HttpResponse, AcmeError> {
            let nonce = [("replay-nonce", "nonce-xyz")];
            // POST bodies must be a flattened JWS we can parse and that signs the url.
            if let Some(b) = body {
                let v: serde_json::Value =
                    serde_json::from_slice(b).map_err(|e| AcmeError::Json(e.to_string()))?;
                assert!(v.get("protected").is_some(), "JWS missing protected header");
                assert!(v.get("signature").is_some(), "JWS missing signature");
                let prot = v["protected"].as_str().unwrap();
                let decoded = URL_SAFE_NO_PAD.decode(prot).unwrap();
                let ph: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
                assert_eq!(ph["url"], url, "JWS url must match request url");
                assert_eq!(ph["alg"], "ES256");
            }
            match url {
                "https://acme/newNonce" => Ok(Self::resp(200, &nonce, "")),
                "https://acme/newAccount" => Ok(Self::resp(
                    201,
                    &[("replay-nonce", "nonce-xyz"), ("location", "https://acme/acct/1")],
                    "{}",
                )),
                "https://acme/newOrder" => Ok(Self::resp(
                    201,
                    &[("replay-nonce", "nonce-xyz"), ("location", "https://acme/order/1")],
                    r#"{"status":"pending","authorizations":["https://acme/authz/1"],"finalize":"https://acme/order/1/finalize"}"#,
                )),
                "https://acme/authz/1" => {
                    let mut p = self.authz_polls.borrow_mut();
                    *p += 1;
                    let status = if *p >= 2 { "valid" } else { "pending" };
                    Ok(Self::resp(
                        200,
                        &nonce,
                        &format!(
                            r#"{{"status":"{status}","challenges":[{{"type":"http-01","url":"https://acme/chal/1","token":"tok-1"}}]}}"#
                        ),
                    ))
                }
                "https://acme/chal/1" => Ok(Self::resp(200, &nonce, "{}")),
                "https://acme/order/1/finalize" => {
                    *self.seen_finalize_csr.borrow_mut() = true;
                    Ok(Self::resp(200, &nonce, r#"{"status":"processing"}"#))
                }
                "https://acme/order/1" => {
                    let mut p = self.order_polls.borrow_mut();
                    *p += 1;
                    Ok(Self::resp(
                        200,
                        &nonce,
                        r#"{"status":"valid","certificate":"https://acme/cert/1"}"#,
                    ))
                }
                "https://acme/cert/1" => Ok(Self::resp(
                    200,
                    &nonce,
                    "-----BEGIN CERTIFICATE-----\nMIIBmock\n-----END CERTIFICATE-----\n",
                )),
                other => Err(AcmeError::Protocol(format!("unexpected url {other}"))),
            }
        }
    }

    #[test]
    fn mock_issuance_full_flow() {
        let directory = Directory {
            new_nonce: "https://acme/newNonce".to_string(),
            new_account: "https://acme/newAccount".to_string(),
            new_order: "https://acme/newOrder".to_string(),
        };
        let key = AcmeAccountKey::generate().unwrap();
        let mock = MockAcme::new();
        let mut client = AcmeClient::new(mock, directory, key);
        let pem = client
            .obtain_certificate(&["app.cave.local".to_string()], b"fake-csr-der")
            .unwrap();
        assert!(pem.contains("BEGIN CERTIFICATE"));
    }
}
