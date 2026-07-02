// SPDX-License-Identifier: Apache-2.0
//! OCI / Docker image-reference parsing and normalisation.
//!
//! Behavioural reimplementation of the documented `distribution/reference`
//! grammar and Docker's `normalizeName` defaulting rules:
//!
//!   * A reference is `[registry/]repository[:tag][@digest]`.
//!   * The first slash-separated component is the registry **only** if it
//!     contains a `.` or `:` or equals `localhost`; otherwise the whole
//!     name is a repository on the default registry.
//!   * Default registry is `docker.io` (normalised from the legacy
//!     `index.docker.io` / `registry-1.docker.io`).
//!   * On `docker.io`, a single-component repository is namespaced under
//!     `library/` (so `nginx` -> `docker.io/library/nginx`).
//!   * Default tag is `latest` when neither tag nor digest is present.
//!
//! Spec sources:
//!   * `distribution/reference` package grammar (reference.md / regexp.go
//!     documented forms).
//!   * Docker `reference.ParseNormalizedNamed` / `splitDockerDomain`
//!     defaulting behaviour (public docs + the documented `docker.io` /
//!     `library/` / `latest` rules).
//!   * OCI image-spec tag grammar `[A-Za-z0-9_][A-Za-z0-9._-]{0,127}`.

use std::fmt;

use crate::digest::{Digest, DigestError};

/// The canonical default registry host.
pub const DEFAULT_REGISTRY: &str = "docker.io";
/// The legacy Docker Hub host that normalises to [`DEFAULT_REGISTRY`].
pub const LEGACY_REGISTRY: &str = "index.docker.io";
/// The default namespace applied to single-component docker.io repositories.
pub const DEFAULT_NAMESPACE: &str = "library";
/// The default tag applied when a reference carries neither tag nor digest.
pub const DEFAULT_TAG: &str = "latest";

/// Errors raised while parsing an image reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceError {
    /// The reference was empty.
    Empty,
    /// The repository path was empty or otherwise malformed.
    InvalidRepository(String),
    /// A tag violated the OCI tag grammar.
    InvalidTag(String),
    /// The registry host was malformed.
    InvalidRegistry(String),
    /// The embedded digest failed to parse.
    InvalidDigest(DigestError),
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty image reference"),
            Self::InvalidRepository(r) => write!(f, "invalid repository: {r}"),
            Self::InvalidTag(t) => write!(f, "invalid tag: {t}"),
            Self::InvalidRegistry(r) => write!(f, "invalid registry: {r}"),
            Self::InvalidDigest(e) => write!(f, "invalid digest in reference: {e}"),
        }
    }
}

impl std::error::Error for ReferenceError {}

impl From<DigestError> for ReferenceError {
    fn from(e: DigestError) -> Self {
        Self::InvalidDigest(e)
    }
}

/// A fully-normalised image reference.
///
/// Every field is canonical: `registry` is a real host, `repository` is the
/// full path (namespace-expanded for docker.io), and exactly one of `tag` /
/// `digest` is always populated (a bare name defaults to `:latest`). A
/// reference that carries an explicit digest keeps it; if it carries neither
/// tag nor digest it gets the default tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    registry: String,
    repository: String,
    tag: Option<String>,
    digest: Option<Digest>,
}

impl Reference {
    /// Parses and normalises an image reference string.
    ///
    /// # Errors
    /// Returns a [`ReferenceError`] variant when the reference is empty or
    /// any component (registry, repository, tag, embedded digest) is invalid.
    pub fn parse(input: &str) -> Result<Self, ReferenceError> {
        if input.trim().is_empty() {
            return Err(ReferenceError::Empty);
        }

        // Peel off the digest first (`@algorithm:encoded`). A digest, if
        // present, is always the final component.
        let (without_digest, digest) = match input.split_once('@') {
            Some((head, dig)) => (head, Some(Digest::parse(dig)?)),
            None => (input, None),
        };

        // Split the registry from the remainder using the documented
        // splitDockerDomain heuristic.
        let (registry, remainder) = split_registry(without_digest);

        // Peel off the tag. The tag separator is the last `:` in the
        // remainder, but only if what follows is a valid tag (this avoids
        // mis-reading a registry port — already removed — and is the
        // documented behaviour).
        let (repo_path, tag) = split_tag(remainder)?;

        if repo_path.is_empty() {
            return Err(ReferenceError::InvalidRepository(remainder.to_owned()));
        }

        let repository = normalise_repository(&registry, repo_path);
        validate_repository(&repository)?;

        // Apply the default tag only when neither a tag nor a digest exists.
        let tag = match (tag, &digest) {
            (Some(t), _) => Some(t),
            (None, Some(_)) => None,
            (None, None) => Some(DEFAULT_TAG.to_owned()),
        };

        Ok(Self {
            registry,
            repository,
            tag,
            digest,
        })
    }

    /// The normalised registry host (e.g. `docker.io`, `ghcr.io`).
    #[must_use]
    pub fn registry(&self) -> &str {
        &self.registry
    }

    /// The full repository path (namespace-expanded for docker.io).
    #[must_use]
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// The tag, if this reference is tag-addressed.
    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// The digest, if this reference is digest-addressed.
    #[must_use]
    pub const fn digest(&self) -> Option<&Digest> {
        self.digest.as_ref()
    }

    /// True when the reference pins an exact digest (content-addressed).
    #[must_use]
    pub const fn is_canonical(&self) -> bool {
        self.digest.is_some()
    }

    /// The canonical familiar string form (round-trips through [`Reference::parse`]).
    ///
    /// ```
    /// use cave_home_containerd_rs::reference::Reference;
    /// let r = Reference::parse("nginx").expect("valid");
    /// assert_eq!(r.to_string(), "docker.io/library/nginx:latest");
    /// ```
    #[must_use]
    pub fn canonical(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.registry, self.repository)?;
        if let Some(t) = &self.tag {
            write!(f, ":{t}")?;
        }
        if let Some(d) = &self.digest {
            write!(f, "@{d}")?;
        }
        Ok(())
    }
}

/// Splits the registry host (if any) from the rest, normalising the default
/// and legacy Docker Hub hosts. Returns `(registry, remainder)`.
fn split_registry(s: &str) -> (String, &str) {
    match s.split_once('/') {
        Some((head, rest)) if is_registry_host(head) => {
            let registry = if head == LEGACY_REGISTRY || head == "registry-1.docker.io" {
                DEFAULT_REGISTRY.to_owned()
            } else {
                head.to_owned()
            };
            (registry, rest)
        }
        _ => (DEFAULT_REGISTRY.to_owned(), s),
    }
}

/// A leading component is a registry host iff it contains `.` or `:`, or is
/// exactly `localhost`. Otherwise it is the first path component of a
/// docker.io repository (the documented `splitDockerDomain` rule).
fn is_registry_host(head: &str) -> bool {
    head == "localhost" || head.contains('.') || head.contains(':')
}

/// Peels a trailing `:tag` from the repository remainder. The tag is whatever
/// follows the final `:`; if there is no `:` the whole string is the path.
fn split_tag(s: &str) -> Result<(&str, Option<String>), ReferenceError> {
    match s.rsplit_once(':') {
        Some((path, tag)) => {
            validate_tag(tag)?;
            Ok((path, Some(tag.to_owned())))
        }
        None => Ok((s, None)),
    }
}

/// Validates the OCI tag grammar: `[A-Za-z0-9_][A-Za-z0-9._-]{0,127}`.
fn validate_tag(tag: &str) -> Result<(), ReferenceError> {
    if tag.is_empty() || tag.len() > 128 {
        return Err(ReferenceError::InvalidTag(tag.to_owned()));
    }
    let mut bytes = tag.bytes();
    let first = bytes.next().unwrap_or(b'/');
    let valid_first = first.is_ascii_alphanumeric() || first == b'_';
    if !valid_first {
        return Err(ReferenceError::InvalidTag(tag.to_owned()));
    }
    if !bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
        return Err(ReferenceError::InvalidTag(tag.to_owned()));
    }
    Ok(())
}

/// Applies docker.io's `library/` namespacing to a single-component repository.
fn normalise_repository(registry: &str, repo_path: &str) -> String {
    if registry == DEFAULT_REGISTRY && !repo_path.contains('/') {
        format!("{DEFAULT_NAMESPACE}/{repo_path}")
    } else {
        repo_path.to_owned()
    }
}

/// Validates the repository path grammar: lower-alphanumeric components
/// separated by `/`, with `.`, `_`, `-` allowed as internal separators within
/// a component (the documented `distribution/reference` path-component rule).
fn validate_repository(repo: &str) -> Result<(), ReferenceError> {
    if repo.is_empty() {
        return Err(ReferenceError::InvalidRepository(repo.to_owned()));
    }
    for component in repo.split('/') {
        if component.is_empty() {
            return Err(ReferenceError::InvalidRepository(repo.to_owned()));
        }
        let bytes = component.as_bytes();
        let edge_ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        if !edge_ok(bytes[0]) || !edge_ok(bytes[bytes.len() - 1]) {
            return Err(ReferenceError::InvalidRepository(repo.to_owned()));
        }
        if !bytes.iter().all(|&b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        }) {
            return Err(ReferenceError::InvalidRepository(repo.to_owned()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Reference {
        Reference::parse(s).unwrap_or_else(|e| panic!("parse {s:?}: {e}"))
    }

    #[test]
    fn bare_name_gets_docker_io_library_latest() {
        let r = parse("nginx");
        assert_eq!(r.registry(), "docker.io");
        assert_eq!(r.repository(), "library/nginx");
        assert_eq!(r.tag(), Some("latest"));
        assert!(r.digest().is_none());
        assert_eq!(r.to_string(), "docker.io/library/nginx:latest");
    }

    #[test]
    fn explicit_tag_is_kept() {
        let r = parse("nginx:1.27");
        assert_eq!(r.repository(), "library/nginx");
        assert_eq!(r.tag(), Some("1.27"));
    }

    #[test]
    fn user_namespace_is_not_library_wrapped() {
        let r = parse("grafana/grafana");
        assert_eq!(r.registry(), "docker.io");
        assert_eq!(r.repository(), "grafana/grafana");
        assert_eq!(r.tag(), Some("latest"));
    }

    #[test]
    fn custom_registry_with_dot_is_detected() {
        let r = parse("ghcr.io/home-assistant/core:2026.1");
        assert_eq!(r.registry(), "ghcr.io");
        assert_eq!(r.repository(), "home-assistant/core");
        assert_eq!(r.tag(), Some("2026.1"));
    }

    #[test]
    fn registry_with_port_is_detected_and_tag_still_parsed() {
        let r = parse("localhost:5000/myimage:dev");
        assert_eq!(r.registry(), "localhost:5000");
        assert_eq!(r.repository(), "myimage");
        assert_eq!(r.tag(), Some("dev"));
    }

    #[test]
    fn localhost_is_a_registry_without_a_dot() {
        let r = parse("localhost/img");
        assert_eq!(r.registry(), "localhost");
        assert_eq!(r.repository(), "img");
    }

    #[test]
    fn digest_only_reference_has_no_default_tag() {
        let dig = format!("sha256:{}", "a".repeat(64));
        let r = parse(&format!("nginx@{dig}"));
        assert_eq!(r.repository(), "library/nginx");
        assert!(r.tag().is_none());
        assert!(r.is_canonical());
        assert_eq!(r.digest().expect("digest").to_string(), dig);
    }

    #[test]
    fn tag_and_digest_both_kept() {
        let dig = format!("sha256:{}", "b".repeat(64));
        let r = parse(&format!("redis:7@{dig}"));
        assert_eq!(r.tag(), Some("7"));
        assert!(r.digest().is_some());
        assert_eq!(r.to_string(), format!("docker.io/library/redis:7@{dig}"));
    }

    #[test]
    fn legacy_index_docker_io_normalises() {
        let r = parse("index.docker.io/library/busybox:latest");
        assert_eq!(r.registry(), "docker.io");
        assert_eq!(r.repository(), "library/busybox");
    }

    #[test]
    fn full_form_round_trips() {
        let dig = format!("sha256:{}", "c".repeat(64));
        let s = format!("ghcr.io/ns/app:v1@{dig}");
        let r = parse(&s);
        assert_eq!(r.to_string(), s);
        assert_eq!(Reference::parse(&r.to_string()), Ok(r));
    }

    #[test]
    fn empty_reference_is_rejected() {
        assert_eq!(Reference::parse(""), Err(ReferenceError::Empty));
        assert_eq!(Reference::parse("   "), Err(ReferenceError::Empty));
    }

    #[test]
    fn malformed_digest_is_rejected() {
        let err = Reference::parse("nginx@sha256:zz").expect_err("bad digest");
        assert!(matches!(err, ReferenceError::InvalidDigest(_)));
    }

    #[test]
    fn invalid_tag_is_rejected() {
        let err = Reference::parse("nginx:.bad").expect_err("bad tag");
        assert!(matches!(err, ReferenceError::InvalidTag(_)));
    }

    #[test]
    fn uppercase_repository_is_rejected() {
        let err = Reference::parse("Nginx").expect_err("upper repo");
        assert!(matches!(err, ReferenceError::InvalidRepository(_)));
    }

    #[test]
    fn empty_path_component_is_rejected() {
        let err = Reference::parse("ghcr.io//app").expect_err("empty component");
        assert!(matches!(err, ReferenceError::InvalidRepository(_)));
    }

    #[test]
    fn long_tag_is_rejected() {
        let s = format!("nginx:{}", "a".repeat(129));
        assert!(matches!(
            Reference::parse(&s),
            Err(ReferenceError::InvalidTag(_))
        ));
    }
}
