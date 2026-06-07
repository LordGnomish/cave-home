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
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .map_err(|e| AcmeError::Crypto(e.to_string()))?;
        Self::from_pkcs8(pkcs8.as_ref())
    }

    /// Reconstruct an account key from its PKCS#8 bytes.
    ///
    /// # Errors
    /// [`AcmeError::Crypto`] if the bytes are not a valid P-256 PKCS#8 key.
    pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, AcmeError> {
        let rng = SystemRandom::new();
        let key_pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8, &rng)
                .map_err(|e| AcmeError::Crypto(e.to_string()))?;
        Ok(Self { key_pair, pkcs8: pkcs8.to_vec(), rng })
    }

    /// The PKCS#8 serialization (for persistence).
    #[must_use]
    pub fn pkcs8_bytes(&self) -> &[u8] {
        &self.pkcs8
    }

    /// The base64url EC coordinates `(x, y)` of the public key.
    fn coordinates(&self) -> (String, String) {
        use ring::signature::KeyPair as _;
        // Uncompressed point: 0x04 ‖ X(32) ‖ Y(32).
        let pk = self.key_pair.public_key().as_ref();
        let x = URL_SAFE_NO_PAD.encode(&pk[1..33]);
        let y = URL_SAFE_NO_PAD.encode(&pk[33..65]);
        (x, y)
    }

    /// The public JWK (`kty=EC, crv=P-256, x, y`) as a JSON value.
    #[must_use]
    pub fn jwk(&self) -> serde_json::Value {
        let (x, y) = self.coordinates();
        serde_json::json!({ "crv": "P-256", "kty": "EC", "x": x, "y": y })
    }

    /// The RFC 7638 JWK thumbprint: base64url(SHA-256(canonical JWK)).
    #[must_use]
    pub fn thumbprint(&self) -> String {
        let (x, y) = self.coordinates();
        // RFC 7638 canonical form: members sorted lexicographically, no space.
        let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
        let digest = Sha256::digest(canonical.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }

    /// ES256-sign `message`, returning the raw 64-byte `r‖s` signature.
    ///
    /// # Errors
    /// [`AcmeError::Crypto`] if signing fails.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, AcmeError> {
        self.key_pair
            .sign(&self.rng, message)
            .map(|s| s.as_ref().to_vec())
            .map_err(|e| AcmeError::Crypto(e.to_string()))
    }
}

/// The HTTP-01 key authorization for `token`: `token.thumbprint`.
#[must_use]
pub fn key_authorization(token: &str, account_key: &AcmeAccountKey) -> String {
    format!("{token}.{}", account_key.thumbprint())
}

/// Whether a certificate expiring at `not_after_unix` should be renewed now,
/// given the current time and how long before expiry to renew (Traefik default:
/// 30 days).
#[must_use]
pub const fn needs_renewal(not_after_unix: u64, now_unix: u64, renew_before_secs: u64) -> bool {
    now_unix.saturating_add(renew_before_secs) >= not_after_unix
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
    pub const fn new(transport: T, directory: Directory, account_key: AcmeAccountKey) -> Self {
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
        self.ensure_account()?;
        let (order_url, order) = self.new_order(domains)?;
        for authz_url in &order.authorizations {
            self.process_authorization(authz_url)?;
        }
        self.finalize_and_download(&order_url, &order.finalize, csr_der)
    }

    /// Fetch a fresh replay nonce, or reuse the one carried from the last POST.
    fn take_nonce(&mut self) -> Result<String, AcmeError> {
        if let Some(n) = self.nonce.take() {
            return Ok(n);
        }
        let url = self.directory.new_nonce.clone();
        let resp = self.transport.request("GET", &url, None)?;
        resp.header("replay-nonce")
            .map(str::to_owned)
            .ok_or_else(|| AcmeError::Protocol("newNonce returned no Replay-Nonce".into()))
    }

    /// Build, sign and POST a JWS request; track the returned replay nonce.
    fn post(&mut self, url: &str, payload: &str, use_jwk: bool) -> Result<HttpResponse, AcmeError> {
        let nonce = self.take_nonce()?;
        let protected = self.protected_header(url, &nonce, use_jwk)?;
        let jws = self.build_jws(&protected, payload)?;
        let resp = self.transport.request("POST", url, Some(&jws))?;
        if let Some(n) = resp.header("replay-nonce") {
            self.nonce = Some(n.to_string());
        }
        Ok(resp)
    }

    fn protected_header(
        &self,
        url: &str,
        nonce: &str,
        use_jwk: bool,
    ) -> Result<String, AcmeError> {
        let mut h = serde_json::Map::new();
        h.insert("alg".into(), serde_json::json!("ES256"));
        h.insert("nonce".into(), serde_json::json!(nonce));
        h.insert("url".into(), serde_json::json!(url));
        if use_jwk {
            h.insert("jwk".into(), self.account_key.jwk());
        } else {
            let kid = self
                .account_url
                .clone()
                .ok_or_else(|| AcmeError::Protocol("no registered account (kid)".into()))?;
            h.insert("kid".into(), serde_json::json!(kid));
        }
        serde_json::to_string(&serde_json::Value::Object(h))
            .map_err(|e| AcmeError::Json(e.to_string()))
    }

    fn build_jws(&self, protected_json: &str, payload: &str) -> Result<Vec<u8>, AcmeError> {
        let b64_protected = URL_SAFE_NO_PAD.encode(protected_json.as_bytes());
        let b64_payload = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let signing_input = format!("{b64_protected}.{b64_payload}");
        let signature = self.account_key.sign(signing_input.as_bytes())?;
        let flat = serde_json::json!({
            "protected": b64_protected,
            "payload": b64_payload,
            "signature": URL_SAFE_NO_PAD.encode(signature),
        });
        serde_json::to_vec(&flat).map_err(|e| AcmeError::Json(e.to_string()))
    }

    /// Register the account (idempotent) and capture its `kid` URL.
    fn ensure_account(&mut self) -> Result<(), AcmeError> {
        if self.account_url.is_some() {
            return Ok(());
        }
        let url = self.directory.new_account.clone();
        let payload = serde_json::json!({ "termsOfServiceAgreed": true }).to_string();
        let resp = self.post(&url, &payload, true)?;
        if resp.status >= 400 {
            return Err(AcmeError::Protocol(format!("newAccount status {}", resp.status)));
        }
        let kid = resp
            .header("location")
            .ok_or_else(|| AcmeError::Protocol("newAccount missing Location".into()))?;
        self.account_url = Some(kid.to_string());
        Ok(())
    }

    fn new_order(&mut self, domains: &[String]) -> Result<(String, Order), AcmeError> {
        let identifiers: Vec<_> = domains
            .iter()
            .map(|d| serde_json::json!({ "type": "dns", "value": d }))
            .collect();
        let url = self.directory.new_order.clone();
        let payload = serde_json::json!({ "identifiers": identifiers }).to_string();
        let resp = self.post(&url, &payload, false)?;
        if resp.status >= 400 {
            return Err(AcmeError::Protocol(format!("newOrder status {}", resp.status)));
        }
        let order_url = resp
            .header("location")
            .ok_or_else(|| AcmeError::Protocol("newOrder missing Location".into()))?
            .to_string();
        let order = parse_json::<Order>(&resp.body)?;
        Ok((order_url, order))
    }

    /// Drive one authorization through its HTTP-01 challenge to `valid`.
    fn process_authorization(&mut self, authz_url: &str) -> Result<(), AcmeError> {
        let authz = parse_json::<Authorization>(&self.post(authz_url, "", false)?.body)?;
        if authz.status == "valid" {
            return Ok(());
        }
        let challenge = authz
            .challenges
            .iter()
            .find(|c| c.kind == "http-01")
            .ok_or_else(|| AcmeError::Protocol("no http-01 challenge offered".into()))?;
        // The proxy serves `key_authorization` at the well-known path before we
        // tell the server to validate; compute it to assert the token is usable.
        let _key_auth = key_authorization(&challenge.token, &self.account_key);
        let challenge_url = challenge.url.clone();
        self.post(&challenge_url, "{}", false)?;

        for _ in 0..16 {
            let polled = parse_json::<Authorization>(&self.post(authz_url, "", false)?.body)?;
            match polled.status.as_str() {
                "valid" => return Ok(()),
                "invalid" => return Err(AcmeError::Protocol("authorization invalid".into())),
                _ => {}
            }
        }
        Err(AcmeError::Protocol("authorization did not validate".into()))
    }

    fn finalize_and_download(
        &mut self,
        order_url: &str,
        finalize_url: &str,
        csr_der: &[u8],
    ) -> Result<String, AcmeError> {
        let payload =
            serde_json::json!({ "csr": URL_SAFE_NO_PAD.encode(csr_der) }).to_string();
        self.post(finalize_url, &payload, false)?;

        for _ in 0..16 {
            let order = parse_json::<Order>(&self.post(order_url, "", false)?.body)?;
            match order.status.as_str() {
                "valid" => {
                    let cert_url = order.certificate.ok_or_else(|| {
                        AcmeError::Protocol("valid order without certificate URL".into())
                    })?;
                    let cert = self.post(&cert_url, "", false)?;
                    return String::from_utf8(cert.body)
                        .map_err(|e| AcmeError::Protocol(e.to_string()));
                }
                "invalid" => return Err(AcmeError::Protocol("order invalid".into())),
                _ => {}
            }
        }
        Err(AcmeError::Protocol("order did not finalize".into()))
    }
}

/// Parse a JSON body into `T`, mapping failures to [`AcmeError::Json`].
fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, AcmeError> {
    serde_json::from_slice(body).map_err(|e| AcmeError::Json(e.to_string()))
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
