// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! SHA-256 fingerprints and pins.
//!
//! Two textual forms are accepted, matching the tools a household (or the
//! installer helping them) already has:
//!
//! - a **certificate** pin — the SHA-256 of the whole DER certificate, as
//!   `openssl x509 -noout -fingerprint -sha256` and every browser print it
//!   (`AB:CD:…`, colons and case optional, `cert:` prefix optional);
//! - a **public-key** pin — the SHA-256 of the DER `SubjectPublicKeyInfo`, in
//!   curl's `--pinnedpubkey` form `sha256//<base64>` or as `spki:<hex>`. It
//!   survives a certificate re-issue that keeps the key.

use std::fmt;
use std::str::FromStr;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rustls::pki_types::CertificateDer;

/// A SHA-256 digest of a certificate or public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Fingerprint([u8; 32]);

impl Sha256Fingerprint {
    /// Hash `data`.
    #[must_use]
    pub fn of(data: &[u8]) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, data);
        let mut out = [0_u8; 32];
        out.copy_from_slice(digest.as_ref());
        Self(out)
    }

    /// Wrap raw digest bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Standard (padded) base64 of the digest — curl's pin encoding.
    #[must_use]
    pub fn to_base64(&self) -> String {
        STANDARD.encode(self.0)
    }

    fn from_hex(s: &str) -> Result<Self, PinParseError> {
        let digits: Vec<u8> = s
            .bytes()
            .filter(|b| !matches!(b, b':' | b' ' | b'\t'))
            .collect();
        if digits.is_empty() {
            return Err(PinParseError::Empty);
        }
        if digits.len() != 64 {
            return Err(PinParseError::BadLength { got: digits.len() / 2 });
        }
        let mut out = [0_u8; 32];
        for (i, pair) in digits.chunks_exact(2).enumerate() {
            let hi = hex_value(pair[0]).ok_or_else(|| PinParseError::BadHex(s.to_owned()))?;
            let lo = hex_value(pair[1]).ok_or_else(|| PinParseError::BadHex(s.to_owned()))?;
            out[i] = (hi << 4) | lo;
        }
        Ok(Self(out))
    }

    fn from_base64(s: &str) -> Result<Self, PinParseError> {
        let bytes = STANDARD
            .decode(s.trim())
            .map_err(|e| PinParseError::BadBase64(e.to_string()))?;
        let bytes: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| PinParseError::BadLength { got: bytes.len() })?;
        Ok(Self(bytes))
    }
}

const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `AB:CD:…` — upper-case, colon-separated (the `openssl` / browser form).
impl fmt::Display for Sha256Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{b:02X}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha256Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Fingerprint({self})")
    }
}

/// Hex, 64 digits, colons / whitespace and case ignored.
impl FromStr for Sha256Fingerprint {
    type Err = PinParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s.trim())
    }
}

/// Why a pin string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PinParseError {
    /// Nothing to parse.
    #[error("no fingerprint given")]
    Empty,
    /// Not hexadecimal.
    #[error("fingerprint {0:?} is not hexadecimal (expected 64 hex digits, colons optional)")]
    BadHex(String),
    /// Wrong number of bytes for SHA-256.
    #[error("a SHA-256 fingerprint is 32 bytes, got {got}")]
    BadLength {
        /// Bytes decoded.
        got: usize,
    },
    /// Malformed base64 in a `sha256//` pin.
    #[error("bad base64 in sha256// pin: {0}")]
    BadBase64(String),
}

/// One pinned identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pin {
    /// SHA-256 of the whole DER certificate.
    Certificate(Sha256Fingerprint),
    /// SHA-256 of the DER `SubjectPublicKeyInfo`.
    PublicKey(Sha256Fingerprint),
}

impl Pin {
    /// Whether `presented` (with its pre-computed SPKI digest) matches.
    fn matches(self, certificate: Sha256Fingerprint, public_key: Option<Sha256Fingerprint>) -> bool {
        match self {
            Self::Certificate(fp) => fp == certificate,
            Self::PublicKey(fp) => public_key == Some(fp),
        }
    }
}

/// Certificate pins print as `AB:CD:…`, public-key pins as `sha256//<base64>`
/// — both parse back.
impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Certificate(fp) => write!(f, "{fp}"),
            Self::PublicKey(fp) => write!(f, "sha256//{}", fp.to_base64()),
        }
    }
}

impl FromStr for Pin {
    type Err = PinParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(PinParseError::Empty);
        }
        if let Some(b64) = s.strip_prefix("sha256//") {
            return Sha256Fingerprint::from_base64(b64).map(Self::PublicKey);
        }
        if let Some(hex) = strip_prefix_ignore_case(s, "spki:") {
            return Sha256Fingerprint::from_hex(hex).map(Self::PublicKey);
        }
        let hex = strip_prefix_ignore_case(s, "cert:").unwrap_or(s);
        Sha256Fingerprint::from_hex(hex).map(Self::Certificate)
    }
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
}

/// A set of acceptable pins — any one matching is enough (lets a household
/// pre-pin a replacement console, or both the old and new certificate during
/// a rotation).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinSet(Vec<Pin>);

impl PinSet {
    /// The pins.
    #[must_use]
    pub fn pins(&self) -> &[Pin] {
        &self.0
    }

    /// Whether no pin is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `certificate` (DER) matches any pin.
    #[must_use]
    pub fn matches_certificate(&self, certificate: &[u8]) -> bool {
        let cert_fp = Sha256Fingerprint::of(certificate);
        let spki_fp = if self.0.iter().any(|p| matches!(p, Pin::PublicKey(_))) {
            spki_fingerprint(certificate)
        } else {
            None
        };
        self.0.iter().any(|p| p.matches(cert_fp, spki_fp))
    }
}

/// SHA-256 of a certificate's DER `SubjectPublicKeyInfo`, if it parses.
pub fn spki_fingerprint(certificate: &[u8]) -> Option<Sha256Fingerprint> {
    let der = CertificateDer::from(certificate);
    let cert = webpki::EndEntityCert::try_from(&der).ok()?;
    Some(Sha256Fingerprint::of(cert.subject_public_key_info().as_ref()))
}

impl From<Pin> for PinSet {
    fn from(pin: Pin) -> Self {
        Self(vec![pin])
    }
}

impl FromIterator<Pin> for PinSet {
    fn from_iter<I: IntoIterator<Item = Pin>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// Comma-separated pins; at least one is required.
impl FromStr for PinSet {
    type Err = PinParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pins = s
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(Pin::from_str)
            .collect::<Result<Vec<_>, _>>()?;
        if pins.is_empty() {
            return Err(PinParseError::Empty);
        }
        Ok(Self(pins))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_rejects_odd_and_non_hex() {
        assert!(matches!(Sha256Fingerprint::from_str("abc"), Err(PinParseError::BadLength { .. })));
        let not_hex = "zz".repeat(32);
        assert!(matches!(Sha256Fingerprint::from_str(&not_hex), Err(PinParseError::BadHex(_))));
    }

    #[test]
    fn prefixes_are_case_insensitive() {
        let fp = Sha256Fingerprint::of(b"k");
        assert_eq!(Pin::from_str(&format!("SPKI:{fp}")).unwrap(), Pin::PublicKey(fp));
        assert_eq!(Pin::from_str(&format!("Cert:{fp}")).unwrap(), Pin::Certificate(fp));
    }

    #[test]
    fn public_key_pin_does_not_match_a_garbage_certificate() {
        let set = PinSet::from(Pin::PublicKey(Sha256Fingerprint::of(b"spki")));
        assert!(!set.matches_certificate(b"not a certificate"));
    }
}
