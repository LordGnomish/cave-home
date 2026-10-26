// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! SHA-256 fingerprints and pins (RED skeleton — behaviour lands next commit).

use std::fmt;
use std::str::FromStr;

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

    /// Standard base64 of the digest.
    #[must_use]
    pub fn to_base64(&self) -> String {
        String::new()
    }
}

impl fmt::Display for Sha256Fingerprint {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl fmt::Debug for Sha256Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Fingerprint({self})")
    }
}

impl FromStr for Sha256Fingerprint {
    type Err = PinParseError;
    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        Err(PinParseError::Empty)
    }
}

/// Why a pin string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PinParseError {
    /// Nothing to parse.
    #[error("empty pin")]
    Empty,
}

/// One pinned identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pin {
    /// SHA-256 of the whole DER certificate.
    Certificate(Sha256Fingerprint),
    /// SHA-256 of the DER SubjectPublicKeyInfo.
    PublicKey(Sha256Fingerprint),
}

impl fmt::Display for Pin {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl FromStr for Pin {
    type Err = PinParseError;
    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        Err(PinParseError::Empty)
    }
}

/// A set of acceptable pins.
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
    pub fn matches_certificate(&self, _certificate: &[u8]) -> bool {
        true
    }
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

impl FromStr for PinSet {
    type Err = PinParseError;
    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        Err(PinParseError::Empty)
    }
}
