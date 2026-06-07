// SPDX-License-Identifier: Apache-2.0
//! Response compression (the runtime half of the `Compress` middleware).
//!
//! Spec basis: Traefik's `Compress` middleware negotiates a content encoding
//! from the request's `Accept-Encoding` header, skips already-compressed or
//! tiny payloads, and gzip/deflate-encodes the body otherwise.
//!
//! Brotli is not available offline, so the supported set is gzip + deflate; the
//! negotiation honours client `q`-values and falls back to identity.

use std::io::Write as _;

use flate2::write::{DeflateEncoder, GzEncoder};
use flate2::Compression;

/// A supported content encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// gzip (`Content-Encoding: gzip`).
    Gzip,
    /// raw deflate (`Content-Encoding: deflate`).
    Deflate,
    /// no transformation.
    Identity,
}

impl Encoding {
    /// The `Content-Encoding` token, or `None` for identity.
    #[must_use]
    pub const fn header_value(self) -> Option<&'static str> {
        match self {
            Self::Gzip => Some("gzip"),
            Self::Deflate => Some("deflate"),
            Self::Identity => None,
        }
    }
}

/// Choose the best encoding the client accepts from `supported` (server
/// preference order), honouring `q`-values. Returns [`Encoding::Identity`] when
/// nothing usable is offered.
#[must_use]
pub fn negotiate(accept_encoding: Option<&str>, supported: &[Encoding]) -> Encoding {
    unimplemented!()
}

/// Whether a body of `content_type` / `content_length` is worth compressing:
/// at or above `min_size` and not an already-compressed media type.
#[must_use]
pub fn should_compress(content_type: &str, content_length: Option<u64>, min_size: u64) -> bool {
    unimplemented!()
}

/// gzip-encode `data`.
#[must_use]
pub fn gzip(data: &[u8]) -> Vec<u8> {
    unimplemented!()
}

/// raw-deflate-encode `data`.
#[must_use]
pub fn deflate(data: &[u8]) -> Vec<u8> {
    unimplemented!()
}

/// Encode `data` with `encoding` (identity returns it unchanged).
#[must_use]
pub fn encode(encoding: Encoding, data: &[u8]) -> Vec<u8> {
    match encoding {
        Encoding::Gzip => gzip(data),
        Encoding::Deflate => deflate(data),
        Encoding::Identity => data.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    const SUPPORTED: &[Encoding] = &[Encoding::Gzip, Encoding::Deflate];

    #[test]
    fn negotiate_prefers_server_order_when_unweighted() {
        assert_eq!(negotiate(Some("gzip, deflate"), SUPPORTED), Encoding::Gzip);
    }

    #[test]
    fn negotiate_honours_q_values() {
        assert_eq!(negotiate(Some("gzip;q=0.5, deflate;q=1.0"), SUPPORTED), Encoding::Deflate);
    }

    #[test]
    fn negotiate_skips_explicitly_disabled() {
        assert_eq!(negotiate(Some("gzip;q=0"), SUPPORTED), Encoding::Identity);
    }

    #[test]
    fn negotiate_identity_when_absent_or_unsupported() {
        assert_eq!(negotiate(None, SUPPORTED), Encoding::Identity);
        assert_eq!(negotiate(Some("br"), SUPPORTED), Encoding::Identity);
    }

    #[test]
    fn should_compress_text_above_threshold() {
        assert!(should_compress("text/html; charset=utf-8", Some(5000), 1024));
    }

    #[test]
    fn should_not_compress_small_or_precompressed() {
        assert!(!should_compress("text/html", Some(100), 1024)); // too small
        assert!(!should_compress("image/png", Some(99999), 1024)); // already compressed
        assert!(!should_compress("application/gzip", Some(99999), 1024));
    }

    #[test]
    fn gzip_roundtrips() {
        let body = b"the quick brown fox jumps over the lazy dog".repeat(20);
        let compressed = gzip(&body);
        assert_ne!(compressed, body);
        let mut dec = flate2::read::GzDecoder::new(&compressed[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out, body);
    }

    #[test]
    fn deflate_roundtrips() {
        let body = b"abcabcabcabc".repeat(50);
        let compressed = deflate(&body);
        let mut dec = flate2::read::DeflateDecoder::new(&compressed[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out, body);
    }
}
