// SPDX-License-Identifier: Apache-2.0
//! Content-addressable digest model and the OCI content descriptor.
//!
//! Behavioural reimplementation of the digest grammar documented by the
//! `opencontainers/go-digest` package and the OCI image-spec descriptor
//! (`opencontainers/image-spec`, `descriptor.md`). This is pure value-type
//! parsing/validation; no I/O, no crypto backend is required for the model
//! itself (the hex is validated against the algorithm's declared length).
//!
//! Spec sources:
//!   * OCI image-spec `descriptor.md` — digest grammar
//!     `algorithm ":" encoded`, algorithm `sha256` / `sha512`.
//!   * `opencontainers/go-digest` — canonical algorithm = sha256, the
//!     `Validate()` length + lower-hex rules.

use std::fmt;

/// A digest algorithm. The OCI spec defines `sha256` and `sha512`; `sha256`
/// is the canonical algorithm (`go-digest.Canonical`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Algorithm {
    /// SHA-256 — the OCI canonical algorithm. 64 lower-hex characters.
    Sha256,
    /// SHA-512. 128 lower-hex characters.
    Sha512,
}

impl Algorithm {
    /// The registered algorithm token used in the `algorithm:encoded` form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }

    /// The exact number of lower-hex characters a valid encoding must have.
    #[must_use]
    pub const fn hex_len(self) -> usize {
        match self {
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }

    /// Parses a registered algorithm token.
    ///
    /// # Errors
    /// Returns [`DigestError::UnsupportedAlgorithm`] for an unknown token.
    pub fn parse(s: &str) -> Result<Self, DigestError> {
        match s {
            "sha256" => Ok(Self::Sha256),
            "sha512" => Ok(Self::Sha512),
            other => Err(DigestError::UnsupportedAlgorithm(other.to_owned())),
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors raised when parsing or validating a [`Digest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestError {
    /// The string had no `algorithm:encoded` separator.
    Malformed(String),
    /// The algorithm token is not one this crate supports.
    UnsupportedAlgorithm(String),
    /// The encoded part is the wrong length for the algorithm.
    BadLength {
        /// Algorithm whose length rule was violated.
        algorithm: Algorithm,
        /// Number of encoded characters that were supplied.
        got: usize,
    },
    /// The encoded part contained a non lower-hex character.
    NonHex(String),
}

impl fmt::Display for DigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(s) => write!(f, "malformed digest (want algorithm:encoded): {s}"),
            Self::UnsupportedAlgorithm(a) => write!(f, "unsupported digest algorithm: {a}"),
            Self::BadLength { algorithm, got } => write!(
                f,
                "digest encoding wrong length for {algorithm}: got {got}, want {}",
                algorithm.hex_len()
            ),
            Self::NonHex(s) => write!(f, "digest encoding is not lower-hex: {s}"),
        }
    }
}

impl std::error::Error for DigestError {}

/// A validated content digest in `algorithm:encoded` form.
///
/// Construction is total: a `Digest` only exists if it round-trips its own
/// canonical string form. The encoded part is always lower-hex of the exact
/// length the algorithm requires.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest {
    algorithm: Algorithm,
    encoded: String,
}

impl Digest {
    /// Parses and validates an `algorithm:encoded` digest string.
    ///
    /// Mixed-case hex is rejected (the OCI grammar requires lower-hex), so a
    /// digest never silently normalises its case — callers that produced an
    /// upper-case digest get an explicit error rather than a mismatch later.
    ///
    /// # Errors
    /// Returns a [`DigestError`] variant when the string is not a valid
    /// `algorithm:encoded` digest (missing separator, unknown algorithm,
    /// wrong encoding length, or non lower-hex characters).
    pub fn parse(s: &str) -> Result<Self, DigestError> {
        let (algo, encoded) = s
            .split_once(':')
            .ok_or_else(|| DigestError::Malformed(s.to_owned()))?;
        if algo.is_empty() || encoded.is_empty() {
            return Err(DigestError::Malformed(s.to_owned()));
        }
        let algorithm = Algorithm::parse(algo)?;
        if encoded.len() != algorithm.hex_len() {
            return Err(DigestError::BadLength { algorithm, got: encoded.len() });
        }
        if !encoded.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(DigestError::NonHex(encoded.to_owned()));
        }
        Ok(Self { algorithm, encoded: encoded.to_owned() })
    }

    /// The digest algorithm.
    #[must_use]
    pub const fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// The lower-hex encoded part, without the algorithm prefix.
    #[must_use]
    pub fn encoded(&self) -> &str {
        &self.encoded
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.encoded)
    }
}

/// An OCI media type string newtype, kept distinct from a bare `String` so the
/// descriptor's intent is explicit at call sites.
pub mod media_type {
    /// `application/vnd.oci.image.manifest.v1+json`.
    pub const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
    /// `application/vnd.oci.image.index.v1+json`.
    pub const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
    /// `application/vnd.oci.image.config.v1+json`.
    pub const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
    /// `application/vnd.oci.image.layer.v1.tar+gzip`.
    pub const OCI_LAYER_GZIP: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
}

/// An OCI content descriptor: `(mediaType, digest, size)` plus the documented
/// optional fields. Mirrors `opencontainers/image-spec` `descriptor.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    /// The media type of the referenced content.
    pub media_type: String,
    /// The digest of the targeted content.
    pub digest: Digest,
    /// The size, in bytes, of the raw content.
    pub size: u64,
    /// Optional platform/url/annotation extras kept as raw key/value pairs.
    pub urls: Vec<String>,
}

/// Validation failures for a [`Descriptor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptorError {
    /// `media_type` was empty (the spec requires it).
    EmptyMediaType,
    /// `size` was zero — the OCI spec requires a non-negative, and in
    /// practice non-zero, content length for a real blob.
    ZeroSize,
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMediaType => f.write_str("descriptor mediaType must not be empty"),
            Self::ZeroSize => f.write_str("descriptor size must be non-zero"),
        }
    }
}

impl std::error::Error for DescriptorError {}

impl Descriptor {
    /// Builds a descriptor, validating the documented required fields.
    ///
    /// # Errors
    /// Returns [`DescriptorError::EmptyMediaType`] if `media_type` is empty or
    /// [`DescriptorError::ZeroSize`] if `size` is zero.
    pub fn new(
        media_type: impl Into<String>,
        digest: Digest,
        size: u64,
    ) -> Result<Self, DescriptorError> {
        let media_type = media_type.into();
        if media_type.is_empty() {
            return Err(DescriptorError::EmptyMediaType);
        }
        if size == 0 {
            return Err(DescriptorError::ZeroSize);
        }
        Ok(Self { media_type, digest, size, urls: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_sha256() {
        let s = format!("sha256:{}", "a".repeat(64));
        let d = Digest::parse(&s).expect("valid");
        assert_eq!(d.algorithm(), Algorithm::Sha256);
        assert_eq!(d.encoded().len(), 64);
        assert_eq!(d.to_string(), s);
    }

    #[test]
    fn parses_sha512_with_full_length() {
        let s = format!("sha512:{}", "0".repeat(128));
        let d = Digest::parse(&s).expect("valid");
        assert_eq!(d.algorithm(), Algorithm::Sha512);
        assert_eq!(d.encoded().len(), 128);
    }

    #[test]
    fn rejects_missing_separator() {
        assert_eq!(
            Digest::parse("deadbeef"),
            Err(DigestError::Malformed("deadbeef".to_owned()))
        );
    }

    #[test]
    fn rejects_empty_algorithm_or_encoding() {
        assert!(matches!(Digest::parse(":abc"), Err(DigestError::Malformed(_))));
        assert!(matches!(Digest::parse("sha256:"), Err(DigestError::Malformed(_))));
    }

    #[test]
    fn rejects_unknown_algorithm() {
        assert_eq!(
            Digest::parse("md5:abcd"),
            Err(DigestError::UnsupportedAlgorithm("md5".to_owned()))
        );
    }

    #[test]
    fn rejects_wrong_length() {
        let err = Digest::parse("sha256:abcd").expect_err("too short");
        assert_eq!(err, DigestError::BadLength { algorithm: Algorithm::Sha256, got: 4 });
    }

    #[test]
    fn rejects_uppercase_hex() {
        let s = format!("sha256:{}", "A".repeat(64));
        assert!(matches!(Digest::parse(&s), Err(DigestError::NonHex(_))));
    }

    #[test]
    fn rejects_non_hex_chars() {
        let s = format!("sha256:{}g", "a".repeat(63));
        assert!(matches!(Digest::parse(&s), Err(DigestError::NonHex(_))));
    }

    #[test]
    fn digest_round_trips_and_orders() {
        let a = Digest::parse(&format!("sha256:{}", "0".repeat(64))).expect("valid");
        let b = Digest::parse(&format!("sha256:{}", "1".repeat(64))).expect("valid");
        assert!(a < b);
        let re = Digest::parse(&a.to_string()).expect("round trip");
        assert_eq!(a, re);
    }

    #[test]
    fn descriptor_requires_media_type_and_size() {
        let d = Digest::parse(&format!("sha256:{}", "a".repeat(64))).expect("valid");
        assert_eq!(
            Descriptor::new("", d.clone(), 10),
            Err(DescriptorError::EmptyMediaType)
        );
        assert_eq!(
            Descriptor::new(media_type::OCI_CONFIG, d.clone(), 0),
            Err(DescriptorError::ZeroSize)
        );
        let ok = Descriptor::new(media_type::OCI_MANIFEST, d, 1234).expect("valid");
        assert_eq!(ok.media_type, media_type::OCI_MANIFEST);
        assert_eq!(ok.size, 1234);
    }
}
