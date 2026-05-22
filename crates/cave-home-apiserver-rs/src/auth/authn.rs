// SPDX-License-Identifier: Apache-2.0
//! Authentication implementations.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! - staging/src/k8s.io/apiserver/pkg/authentication/request/x509/x509.go
//! - staging/src/k8s.io/apiserver/pkg/authentication/serviceaccount/jwt.go

use async_trait::async_trait;
use jsonwebtoken::{
    DecodingKey, EncodingKey, Header, Validation, decode, encode,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use x509_parser::pem::Pem;

use crate::types::UserInfo;

/// Authentication-layer errors.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AuthnError {
    /// No credential was presented (or every authenticator declined it).
    #[error("anonymous")]
    Anonymous,
    /// Credential was malformed (header could not be parsed).
    #[error("malformed credential: {0}")]
    Malformed(String),
    /// Credential was well-formed but invalid.
    #[error("invalid credential: {0}")]
    Invalid(String),
}

/// Convenience alias.
pub type AuthnResult = Result<UserInfo, AuthnError>;

/// Authenticator trait.
///
/// Source: staging/src/k8s.io/apiserver/pkg/authentication/authenticator/interfaces.go::Request
#[async_trait]
pub trait Authenticator: Send + Sync {
    async fn authenticate(&self, headers: &[(String, String)]) -> AuthnResult;
}

/// Run several authenticators in order. The first non-`Anonymous` result
/// (success or invalid) wins.
pub struct ChainAuthenticator {
    inner: Vec<Box<dyn Authenticator>>,
}

impl ChainAuthenticator {
    /// Construct a new chain.
    #[must_use]
    pub fn new(inner: Vec<Box<dyn Authenticator>>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Authenticator for ChainAuthenticator {
    async fn authenticate(&self, headers: &[(String, String)]) -> AuthnResult {
        for a in &self.inner {
            match a.authenticate(headers).await {
                Ok(u) => return Ok(u),
                Err(AuthnError::Anonymous) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(AuthnError::Anonymous)
    }
}

// ---------- X.509 client certs --------------------------------------------

/// Validates X.509 client certs against a pinned CA bundle.
///
/// Phase 2 path: the upstream HTTPS terminator (TLS handler) extracts the
/// presented client cert chain and forwards the leaf in a synthetic
/// `X-Remote-Client-Cert` PEM header. The authenticator parses that PEM,
/// verifies the certificate's `Subject` matches `CN=<user>,O=<group>...`,
/// and returns the user.
///
/// CA-chain verification is intentionally minimal in Phase 2: we trust any
/// cert that parses as X.509 and whose `Issuer` matches one of the supplied
/// CAs' `Subject` field. Full RFC 5280 path validation arrives in Phase 2b.
///
/// Source: staging/src/k8s.io/apiserver/pkg/authentication/request/x509/x509.go::Authenticator
pub struct ClientCertAuthenticator {
    /// Trusted CA subjects (DER-encoded RDN strings).
    trusted_issuers: Vec<String>,
}

impl ClientCertAuthenticator {
    /// Construct an authenticator that trusts the supplied PEM CA bundle.
    pub fn new(ca_pem: Vec<u8>) -> Self {
        let mut trusted_issuers = Vec::new();
        for pem in Pem::iter_from_buffer(&ca_pem).flatten() {
            if let Ok(cert) = pem.parse_x509() {
                trusted_issuers.push(cert.subject().to_string());
            }
        }
        Self { trusted_issuers }
    }

    /// How many CAs the bundle parsed to.
    #[must_use]
    pub fn ca_count(&self) -> usize {
        self.trusted_issuers.len()
    }
}

#[async_trait]
impl Authenticator for ClientCertAuthenticator {
    async fn authenticate(&self, headers: &[(String, String)]) -> AuthnResult {
        let Some(pem_value) = find_header(headers, "x-remote-client-cert") else {
            return Err(AuthnError::Anonymous);
        };

        // Parse the supplied PEM blob.
        let bytes = pem_value.replace("\\n", "\n").into_bytes();
        let pem = Pem::iter_from_buffer(&bytes)
            .next()
            .ok_or_else(|| AuthnError::Malformed("no PEM block".into()))?
            .map_err(|e| AuthnError::Malformed(e.to_string()))?;
        let cert = pem
            .parse_x509()
            .map_err(|e| AuthnError::Malformed(e.to_string()))?;

        // Extract subject CN / O.
        let subj = cert.subject().to_string();
        let cn = extract_rdn(&subj, "CN").unwrap_or_else(|| "unknown".to_string());
        let groups = extract_rdn_all(&subj, "O");

        // Verify issuer is trusted (subject-only check for Phase 2).
        let issuer = cert.issuer().to_string();
        if !self.trusted_issuers.is_empty()
            && !self.trusted_issuers.iter().any(|t| t == &issuer)
        {
            return Err(AuthnError::Invalid(format!(
                "issuer {issuer} not in trusted bundle"
            )));
        }

        Ok(UserInfo {
            name: cn,
            uid: String::new(),
            groups,
            extra: Default::default(),
        })
    }
}

// ---------- ServiceAccount bearer tokens ----------------------------------

/// JWT claims structure for ServiceAccount tokens.
///
/// Source: staging/src/k8s.io/apiserver/pkg/authentication/serviceaccount/jwt.go
#[derive(Clone, Debug, Deserialize, Serialize)]
struct SaClaims {
    iss: String,
    sub: String,
    #[serde(rename = "kubernetes.io/serviceaccount/namespace")]
    namespace: String,
    #[serde(rename = "kubernetes.io/serviceaccount/service-account.name")]
    sa_name: String,
    #[serde(default)]
    exp: i64,
}

/// Validates Kubernetes ServiceAccount bearer tokens (JWT, HS256).
///
/// Source: staging/src/k8s.io/apiserver/pkg/authentication/serviceaccount/jwt.go
pub struct ServiceAccountTokenAuthenticator {
    issuer: String,
    hmac_secret: Vec<u8>,
}

impl ServiceAccountTokenAuthenticator {
    /// Construct an authenticator.
    #[must_use]
    pub fn new(issuer: impl Into<String>, hmac_secret: Vec<u8>) -> Self {
        Self {
            issuer: issuer.into(),
            hmac_secret,
        }
    }

    /// Mint a token for the given ServiceAccount.
    pub fn mint(&self, namespace: &str, sa_name: &str) -> Result<String, AuthnError> {
        let claims = SaClaims {
            iss: self.issuer.clone(),
            sub: format!("system:serviceaccount:{namespace}:{sa_name}"),
            namespace: namespace.to_string(),
            sa_name: sa_name.to_string(),
            exp: 32_503_680_000, // year 3000 — Phase 2 tokens don't expire on the wire
        };
        encode(
            &Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.hmac_secret),
        )
        .map_err(|e| AuthnError::Invalid(e.to_string()))
    }

    /// Issuer the validator expects (`iss` claim).
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// HMAC secret length (debug only).
    #[must_use]
    pub fn secret_len(&self) -> usize {
        self.hmac_secret.len()
    }
}

#[async_trait]
impl Authenticator for ServiceAccountTokenAuthenticator {
    async fn authenticate(&self, headers: &[(String, String)]) -> AuthnResult {
        let Some(auth_value) = find_header(headers, "authorization") else {
            return Err(AuthnError::Anonymous);
        };
        let token = match auth_value.strip_prefix("Bearer ") {
            Some(t) => t,
            None => return Err(AuthnError::Anonymous),
        };
        let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.set_issuer(&[&self.issuer]);
        validation.validate_exp = false; // Phase 2: no exp enforcement
        let token_data = decode::<SaClaims>(
            token,
            &DecodingKey::from_secret(&self.hmac_secret),
            &validation,
        )
        .map_err(|e| AuthnError::Invalid(e.to_string()))?;
        let c = token_data.claims;
        Ok(UserInfo {
            name: format!(
                "system:serviceaccount:{}:{}",
                c.namespace, c.sa_name
            ),
            uid: String::new(),
            groups: vec![
                "system:serviceaccounts".to_string(),
                format!("system:serviceaccounts:{}", c.namespace),
            ],
            extra: Default::default(),
        })
    }
}

// ---------- helpers --------------------------------------------------------

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Extract the first `<attr>=value` from a `CN=foo, O=bar, ...` string.
fn extract_rdn(s: &str, attr: &str) -> Option<String> {
    s.split(',')
        .filter_map(|part| {
            let kv: Vec<&str> = part.trim().splitn(2, '=').collect();
            if kv.len() == 2 && kv[0].eq_ignore_ascii_case(attr) {
                Some(kv[1].to_string())
            } else {
                None
            }
        })
        .next()
}

/// All `<attr>=value` entries, in order.
fn extract_rdn_all(s: &str, attr: &str) -> Vec<String> {
    s.split(',')
        .filter_map(|part| {
            let kv: Vec<&str> = part.trim().splitn(2, '=').collect();
            if kv.len() == 2 && kv[0].eq_ignore_ascii_case(attr) {
                Some(kv[1].to_string())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn anonymous_when_no_header() {
        let a = ClientCertAuthenticator::new(vec![]);
        let res = a.authenticate(&[]).await;
        assert!(matches!(res, Err(AuthnError::Anonymous)));
    }

    #[tokio::test]
    async fn sa_token_round_trips_through_mint_and_authenticate() {
        let a = ServiceAccountTokenAuthenticator::new(
            "kubernetes/serviceaccount",
            b"unit-test-secret".to_vec(),
        );
        let token = a.mint("default", "my-sa").expect("mint");
        let res = a
            .authenticate(&[("Authorization".to_string(), format!("Bearer {token}"))])
            .await
            .expect("authenticate");
        assert_eq!(res.name, "system:serviceaccount:default:my-sa");
        assert!(res.groups.contains(&"system:serviceaccounts".to_string()));
        assert!(res
            .groups
            .contains(&"system:serviceaccounts:default".to_string()));
    }

    #[tokio::test]
    async fn sa_token_rejects_bad_signature() {
        let a = ServiceAccountTokenAuthenticator::new("k/sa", b"secret".to_vec());
        let res = a
            .authenticate(&[(
                "Authorization".to_string(),
                "Bearer not-a-real-jwt".to_string(),
            )])
            .await;
        assert!(matches!(res, Err(AuthnError::Invalid(_))));
    }

    #[tokio::test]
    async fn sa_token_rejects_wrong_secret() {
        let a = ServiceAccountTokenAuthenticator::new("k/sa", b"secret-A".to_vec());
        let token = a.mint("ns", "sa").expect("mint");
        let b = ServiceAccountTokenAuthenticator::new("k/sa", b"secret-B".to_vec());
        let res = b
            .authenticate(&[("Authorization".to_string(), format!("Bearer {token}"))])
            .await;
        assert!(matches!(res, Err(AuthnError::Invalid(_))));
    }

    #[tokio::test]
    async fn chain_returns_first_non_anonymous() {
        let chain = ChainAuthenticator::new(vec![
            Box::new(ClientCertAuthenticator::new(vec![])),
            Box::new(ServiceAccountTokenAuthenticator::new(
                "k/sa",
                b"secret".to_vec(),
            )),
        ]);
        let res = chain.authenticate(&[]).await;
        assert!(matches!(res, Err(AuthnError::Anonymous)));
    }

    #[test]
    fn extract_cn_from_subject_string() {
        assert_eq!(
            extract_rdn("CN=alice, O=eng, O=ops", "CN"),
            Some("alice".to_string())
        );
        assert_eq!(
            extract_rdn_all("CN=alice, O=eng, O=ops", "O"),
            vec!["eng".to_string(), "ops".to_string()]
        );
    }
}
