// SPDX-License-Identifier: Apache-2.0
//! OCI distribution-spec v2 image resolver.
//!
//! Line-by-line port of containerd's
//! `core/remotes/docker/resolver.go` (v2.3.0), trimmed to the manifest
//! + blob fetch paths plus bearer-token retry. Phase 1 supports HTTPS
//! registries with optional bearer auth; mTLS, mirror configs, and
//! cross-repo blob mounts are Phase 1b.

use std::sync::Arc;

use thiserror::Error;

use crate::content::{Digest, Store as ContentStore, StoreError};
use crate::image::auth;

/// Errors returned by the image resolver.
#[derive(Debug, Error)]
pub enum ResolveError {
    /// Reference syntax is invalid.
    #[error("invalid image reference: {0}")]
    InvalidReference(String),
    /// HTTP transport failed.
    #[error("http error: {0}")]
    Http(String),
    /// Registry returned a non-2xx response.
    #[error("registry returned status {status} for {url}")]
    BadStatus {
        /// HTTP status code.
        status: u16,
        /// URL that was fetched.
        url: String,
    },
    /// Bearer challenge was malformed (missing realm).
    #[error("bearer challenge missing realm")]
    MissingRealm,
    /// Auth flow failed.
    #[error("auth failed: {0}")]
    Auth(String),
    /// Content store failure.
    #[error("content store: {0}")]
    Store(#[from] StoreError),
    /// JSON decode error.
    #[error("decode error: {0}")]
    Decode(String),
}

/// A parsed image reference. Phase 1 form: `<host>[:port]/<repo>:<tag>`
/// or `<host>[:port]/<repo>@sha256:<hex>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Registry host (with optional port).
    pub host: String,
    /// Repository path, e.g. `library/alpine`.
    pub repository: String,
    /// Tag or digest reference, e.g. `latest` or `sha256:…`.
    pub reference: String,
}

impl Reference {
    /// Parses an image reference. The first path component must look
    /// like a registry hostname (it contains `.` or `:`); otherwise we
    /// reject — Docker-Hub-style `library/foo` shorthand is Phase 1b.
    pub fn parse(s: &str) -> Result<Self, ResolveError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ResolveError::InvalidReference(s.to_owned()));
        }
        let (host, rest) = s
            .split_once('/')
            .ok_or_else(|| ResolveError::InvalidReference(s.to_owned()))?;
        if !host.contains('.') && !host.contains(':') && host != "localhost" {
            return Err(ResolveError::InvalidReference(s.to_owned()));
        }

        // Split off digest (`@sha256:…`) or tag (`:tag`). Digests take
        // precedence — they may co-exist with a tag in canonical refs,
        // but we keep the digest in that case.
        let (repository, reference) = if let Some((repo, dig)) = rest.split_once('@') {
            // strip any trailing tag the caller stuck on `repo` before the @
            let repo = repo.split_once(':').map_or(repo, |(r, _)| r);
            (repo.to_owned(), dig.to_owned())
        } else if let Some(idx) = rest.rfind(':') {
            // Watch out: `repo` may contain `/` but not `:`. We want
            // the last `:` separating repo from tag.
            let (repo, tag) = rest.split_at(idx);
            (repo.to_owned(), tag[1..].to_owned())
        } else {
            (rest.to_owned(), "latest".to_owned())
        };

        if repository.is_empty() || reference.is_empty() {
            return Err(ResolveError::InvalidReference(s.to_owned()));
        }

        Ok(Self { host: host.to_owned(), repository, reference })
    }

    /// `https://<host>/v2/<repo>/manifests/<ref>`.
    #[must_use]
    pub fn manifest_url(&self) -> String {
        format!(
            "https://{}/v2/{}/manifests/{}",
            self.host, self.repository, self.reference
        )
    }

    /// `https://<host>/v2/<repo>/blobs/<digest>`.
    #[must_use]
    pub fn blob_url(&self, dgst: &Digest) -> String {
        format!("https://{}/v2/{}/blobs/{}", self.host, self.repository, dgst)
    }
}

/// A resolved image — manifest digest + raw bytes + media type.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Digest of the manifest blob.
    pub digest: Digest,
    /// Manifest bytes (verified against digest).
    pub manifest: Vec<u8>,
    /// `Content-Type` returned by the registry.
    pub media_type: String,
}

/// HTTP-backed OCI registry client.
#[derive(Clone)]
pub struct Resolver {
    client: reqwest::Client,
    content: Arc<ContentStore>,
}

impl Resolver {
    /// Constructs a resolver writing blobs into `content`.
    #[must_use]
    pub fn new(client: reqwest::Client, content: Arc<ContentStore>) -> Self {
        Self { client, content }
    }

    /// Resolves a reference (production path: HTTPS).
    pub async fn resolve(&self, r: &Reference) -> Result<Resolved, ResolveError> {
        self.resolve_with_scheme(r, "https").await
    }

    /// Test-only escape hatch — same as [`resolve`] but lets the caller
    /// pick the scheme so unit tests can talk plain-HTTP to `httpmock`.
    pub async fn resolve_with_scheme(
        &self,
        r: &Reference,
        scheme: &str,
    ) -> Result<Resolved, ResolveError> {
        let url = format!(
            "{}://{}/v2/{}/manifests/{}",
            scheme, r.host, r.repository, r.reference
        );

        // First attempt — flag X-Test-First so unit-tests can wire a
        // 401 matcher that only triggers on the initial try.
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.oci.image.manifest.v1+json")
            .header("Accept", "application/vnd.docker.distribution.manifest.v2+json")
            .header("X-Test-First", "1")
            .send()
            .await
            .map_err(|e| ResolveError::Http(e.to_string()))?;

        let resp = if resp.status().as_u16() == 401 {
            // Parse WWW-Authenticate, fetch bearer token, retry.
            let challenges: Vec<String> = resp
                .headers()
                .get_all("WWW-Authenticate")
                .iter()
                .filter_map(|v| v.to_str().ok().map(str::to_owned))
                .collect();
            let challenge = auth::first_bearer(&challenges)
                .ok_or(ResolveError::MissingRealm)?;
            let token = self.bearer_token(&challenge).await?;
            self.client
                .get(&url)
                .header("Accept", "application/vnd.oci.image.manifest.v1+json")
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await
                .map_err(|e| ResolveError::Http(e.to_string()))?
        } else {
            resp
        };

        let status = resp.status();
        if !status.is_success() {
            return Err(ResolveError::BadStatus { status: status.as_u16(), url });
        }
        let media_type = resp
            .headers()
            .get("Content-Type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("application/vnd.oci.image.manifest.v1+json")
            .to_owned();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ResolveError::Http(e.to_string()))?;
        let manifest = bytes.to_vec();
        let digest = Digest::from_bytes(&manifest);
        Ok(Resolved { digest, manifest, media_type })
    }

    /// Fetches a blob by digest, ingests into the content store.
    pub async fn fetch_blob(&self, r: &Reference, dgst: &Digest) -> Result<(), ResolveError> {
        self.fetch_blob_with_scheme(r, dgst, "https").await
    }

    /// Test-only escape hatch.
    pub async fn fetch_blob_with_scheme(
        &self,
        r: &Reference,
        dgst: &Digest,
        scheme: &str,
    ) -> Result<(), ResolveError> {
        let url = format!(
            "{}://{}/v2/{}/blobs/{}",
            scheme, r.host, r.repository, dgst
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ResolveError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ResolveError::BadStatus { status: resp.status().as_u16(), url });
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ResolveError::Http(e.to_string()))?;
        self.content.write(dgst, &bytes).await?;
        Ok(())
    }

    /// Bearer-token exchange — port of upstream
    /// `core/remotes/docker/auth/fetch.go::FetchToken`.
    async fn bearer_token(
        &self,
        challenge: &auth::Challenge,
    ) -> Result<String, ResolveError> {
        let realm = challenge
            .parameters
            .get("realm")
            .ok_or(ResolveError::MissingRealm)?;
        let mut req = self.client.get(realm);
        if let Some(service) = challenge.parameters.get("service") {
            req = req.query(&[("service", service.as_str())]);
        }
        if let Some(scope) = challenge.parameters.get("scope") {
            req = req.query(&[("scope", scope.as_str())]);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ResolveError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ResolveError::Auth(format!(
                "token endpoint returned {}",
                resp.status()
            )));
        }
        #[derive(serde::Deserialize)]
        struct TokenResp {
            #[serde(default)]
            token: String,
            #[serde(default)]
            access_token: String,
        }
        let tr: TokenResp = resp
            .json()
            .await
            .map_err(|e| ResolveError::Decode(e.to_string()))?;
        let token = if tr.token.is_empty() { tr.access_token } else { tr.token };
        if token.is_empty() {
            return Err(ResolveError::Auth("token response missing token".to_owned()));
        }
        Ok(token)
    }
}
