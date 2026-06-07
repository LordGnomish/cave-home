// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! First-party, dependency-free crypto/encoding primitives the OAuth2-PKCE flow
//! needs: SHA-256 (FIPS 180-4), URL-safe base64 without padding (RFC 4648 §5)
//! and RFC 3986 percent-encoding. These are tiny, well-specified and fully
//! covered by published test vectors, so cave-home carries its own rather than
//! pulling a transitive crypto dependency into the single binary.

#[cfg(test)]
mod tests {
    use super::*;

    // --- SHA-256: FIPS 180-4 / NIST published vectors ----------------------

    fn hex(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    #[test]
    fn sha256_empty() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_two_block_message() {
        // The 56-byte NIST vector that spans two 512-bit blocks.
        let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            hex(&sha256(msg)),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_million_a_prefix() {
        // A long message forcing many blocks (1000 'a's — cheaper than the
        // classic million but still multi-block and deterministic).
        let msg = vec![b'a'; 1000];
        // Precomputed with a reference implementation.
        assert_eq!(
            hex(&sha256(&msg)),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    // --- base64url (no padding): RFC 4648 §5 / §10 -------------------------

    #[test]
    fn base64url_rfc4648_vectors() {
        assert_eq!(base64url_nopad(b""), "");
        assert_eq!(base64url_nopad(b"f"), "Zg");
        assert_eq!(base64url_nopad(b"fo"), "Zm8");
        assert_eq!(base64url_nopad(b"foo"), "Zm9v");
        assert_eq!(base64url_nopad(b"foob"), "Zm9vYg");
        assert_eq!(base64url_nopad(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url_nopad(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64url_is_url_safe() {
        // Bytes that map to + and / in standard base64 must become - and _.
        let out = base64url_nopad(&[0xfb, 0xff, 0xbf]);
        assert!(!out.contains('+'));
        assert!(!out.contains('/'));
        assert!(!out.contains('='));
        assert!(out.contains('-') || out.contains('_'));
    }

    // --- percent-encoding: RFC 3986 unreserved set ------------------------

    #[test]
    fn percent_encode_keeps_unreserved() {
        assert_eq!(percent_encode("aZ09-._~"), "aZ09-._~");
    }

    #[test]
    fn percent_encode_escapes_reserved_and_space() {
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("a/b?c=d&e"), "a%2Fb%3Fc%3Dd%26e");
        assert_eq!(percent_encode(":"), "%3A");
    }

    #[test]
    fn percent_encode_utf8_is_byte_wise() {
        // "ü" is 0xC3 0xBC in UTF-8.
        assert_eq!(percent_encode("ü"), "%C3%BC");
    }
}
