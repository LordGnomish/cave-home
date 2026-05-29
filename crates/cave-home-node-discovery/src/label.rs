//! DNS label encode/decode — the length-prefixed wire primitive.
//!
//! A DNS name on the wire is a sequence of *labels*, each one a single length
//! byte (1..=63) followed by that many octets, terminated by a zero-length
//! label (the root). This module handles a single label in isolation;
//! [`crate::dns_name`] composes labels into whole names.
//!
//! Compression pointers (RFC 1035 §4.1.4 — a length byte with the top two bits
//! set, redirecting to an earlier offset) are a *decode-side* optimisation for
//! whole messages and are deferred to Phase 1b (see the parity manifest): a
//! single mDNS service-name round-trip never needs them.

/// The maximum length of one DNS label, in octets (RFC 1035 §2.3.4).
pub const MAX_LABEL_LEN: usize = 63;

/// Why a label could not be encoded or decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelError {
    /// A label was empty (only the root may be zero-length, handled by the
    /// name layer, not here).
    Empty,
    /// A label exceeded the 63-octet limit.
    TooLong(usize),
    /// The wire bytes ran out before the declared label length was read.
    Truncated,
    /// The leading length byte used the compression-pointer bits (0b11xxxxxx);
    /// pointer decode is deferred to Phase 1b.
    CompressionPointer,
}

impl core::fmt::Display for LabelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("DNS label is empty"),
            Self::TooLong(n) => write!(f, "DNS label is {n} octets (max {MAX_LABEL_LEN})"),
            Self::Truncated => f.write_str("DNS label wire bytes are truncated"),
            Self::CompressionPointer => {
                f.write_str("DNS compression pointer not supported (deferred)")
            }
        }
    }
}

impl std::error::Error for LabelError {}

/// Encode one label to its length-prefixed wire form, appending to `out`.
///
/// # Errors
/// Returns [`LabelError::Empty`] if `label` is empty, or
/// [`LabelError::TooLong`] if it exceeds [`MAX_LABEL_LEN`] octets.
pub fn encode_label(label: &str, out: &mut Vec<u8>) -> Result<(), LabelError> {
    let bytes = label.as_bytes();
    if bytes.is_empty() {
        return Err(LabelError::Empty);
    }
    if bytes.len() > MAX_LABEL_LEN {
        return Err(LabelError::TooLong(bytes.len()));
    }
    // len() <= 63 fits a u8 with room to spare; no truncation possible.
    let len = u8::try_from(bytes.len()).unwrap_or(0);
    out.push(len);
    out.extend_from_slice(bytes);
    Ok(())
}

/// Decode one label starting at `wire[pos]`.
///
/// Returns the label bytes and the position immediately after them. A
/// zero-length byte (the root terminator) yields an empty `Vec` and advances
/// by one — callers ([`crate::dns_name`]) use that to detect end-of-name.
///
/// # Errors
/// - [`LabelError::Truncated`] if `pos` is past the end, or the declared
///   length runs off the end of `wire`.
/// - [`LabelError::CompressionPointer`] if the length byte sets the
///   compression bits (deferred).
pub fn decode_label(wire: &[u8], pos: usize) -> Result<(Vec<u8>, usize), LabelError> {
    let len_byte = *wire.get(pos).ok_or(LabelError::Truncated)?;
    if len_byte & 0b1100_0000 != 0 {
        return Err(LabelError::CompressionPointer);
    }
    let len = len_byte as usize;
    let start = pos + 1;
    let end = start + len;
    let slice = wire.get(start..end).ok_or(LabelError::Truncated)?;
    Ok((slice.to_vec(), end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_prefixes_length() {
        let mut out = Vec::new();
        encode_label("local", &mut out).expect("encodes");
        assert_eq!(out, vec![5, b'l', b'o', b'c', b'a', b'l']);
    }

    #[test]
    fn encode_rejects_empty() {
        let mut out = Vec::new();
        assert_eq!(encode_label("", &mut out), Err(LabelError::Empty));
    }

    #[test]
    fn encode_rejects_too_long() {
        let mut out = Vec::new();
        let big = "a".repeat(64);
        assert_eq!(encode_label(&big, &mut out), Err(LabelError::TooLong(64)));
    }

    #[test]
    fn encode_accepts_max_length() {
        let mut out = Vec::new();
        let max = "a".repeat(MAX_LABEL_LEN);
        assert!(encode_label(&max, &mut out).is_ok());
        assert_eq!(out[0] as usize, MAX_LABEL_LEN);
    }

    #[test]
    fn decode_reads_label_and_advances() {
        let wire = vec![5, b'l', b'o', b'c', b'a', b'l', 0];
        let (label, next) = decode_label(&wire, 0).expect("decodes");
        assert_eq!(label, b"local");
        assert_eq!(next, 6);
    }

    #[test]
    fn decode_root_terminator_is_empty() {
        let wire = vec![0u8];
        let (label, next) = decode_label(&wire, 0).expect("decodes root");
        assert!(label.is_empty());
        assert_eq!(next, 1);
    }

    #[test]
    fn decode_truncated_label_errors() {
        // Declares 5 octets but only 2 follow.
        let wire = vec![5, b'a', b'b'];
        assert_eq!(decode_label(&wire, 0), Err(LabelError::Truncated));
    }

    #[test]
    fn decode_past_end_errors() {
        let wire = vec![1u8, b'a'];
        assert_eq!(decode_label(&wire, 5), Err(LabelError::Truncated));
    }

    #[test]
    fn decode_compression_pointer_is_deferred_error() {
        // 0xC0 sets both top bits -> a compression pointer.
        let wire = vec![0xC0, 0x0C];
        assert_eq!(decode_label(&wire, 0), Err(LabelError::CompressionPointer));
    }

    #[test]
    fn round_trip_single_label() {
        let mut out = Vec::new();
        encode_label("cavehome", &mut out).expect("encodes");
        let (back, _) = decode_label(&out, 0).expect("decodes");
        assert_eq!(back, b"cavehome");
    }
}
