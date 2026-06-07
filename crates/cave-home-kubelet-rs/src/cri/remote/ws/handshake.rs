// SPDX-License-Identifier: Apache-2.0
//! WebSocket opening-handshake helpers (RFC 6455 §4).
//!
//! Self-contained, dependency-free: a small SHA-1 and base64 implementation
//! back the `Sec-WebSocket-Accept` derivation so the streaming transport adds
//! no crates to the kubelet build. (The CRI streaming server speaks the same
//! RFC 6455 handshake regardless of the channel sub-protocol layered on top.)

/// The RFC 6455 GUID concatenated to the client key before hashing.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

// stub — replaced in the GREEN step
/// SHA-1 digest of `data` (RFC 3174).
#[must_use]
pub fn sha1(_data: &[u8]) -> [u8; 20] {
    [0u8; 20]
}

// stub — replaced in the GREEN step
/// Standard base64 encoding (RFC 4648, with `=` padding).
#[must_use]
pub fn base64_encode(_data: &[u8]) -> String {
    String::new()
}

/// Derive the `Sec-WebSocket-Accept` value from the client's
/// `Sec-WebSocket-Key` per RFC 6455 §4.2.2.
#[must_use]
pub fn accept_key(client_key: &str) -> String {
    let mut buf = client_key.as_bytes().to_vec();
    buf.extend_from_slice(WS_GUID.as_bytes());
    base64_encode(&sha1(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_known_vectors() {
        // FIPS 180-1 / RFC 3174 sample vectors.
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn accept_key_rfc6455_example() {
        // The worked example from RFC 6455 §1.3.
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
