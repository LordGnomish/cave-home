// SPDX-License-Identifier: Apache-2.0
//! RFC 6455 WebSocket frame codec.
//!
//! [`encode`] serialises a [`Frame`]; per RFC 6455 §5.3 a client MUST mask the
//! frames it sends, so [`encode`] takes an optional masking key (the server
//! side passes `None`). [`decode`] parses one frame from a buffer, unmasking if
//! the MASK bit is set, and reports how many bytes it consumed so the caller
//! can drive it over a streaming socket.

/// WebSocket opcodes (RFC 6455 §5.2). Only the subset the CRI streaming
/// transport uses is modelled; reserved opcodes are rejected on decode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpCode {
    /// Continuation of a fragmented message.
    Continuation,
    /// UTF-8 text message.
    Text,
    /// Binary message (the channel frames ride on these).
    Binary,
    /// Connection close.
    Close,
    /// Ping (keepalive).
    Ping,
    /// Pong (keepalive response).
    Pong,
}

/// A decoded WebSocket frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    /// Final-fragment flag.
    pub fin: bool,
    /// Frame opcode.
    pub opcode: OpCode,
    /// Application payload (already unmasked on decode).
    pub payload: Vec<u8>,
}

/// Frame-level protocol error.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FrameError {
    /// The opcode field held a reserved/unsupported value.
    #[error("reserved or unsupported opcode: {0}")]
    BadOpcode(u8),
    /// A 64-bit length frame exceeded what we are willing to buffer.
    #[error("frame length {0} exceeds limit")]
    TooLarge(u64),
}

impl Frame {
    /// Convenience: a binary frame with FIN set.
    #[must_use]
    pub fn binary(payload: Vec<u8>) -> Self {
        Self { fin: true, opcode: OpCode::Binary, payload }
    }

    /// Convenience: a close frame with FIN set.
    #[must_use]
    pub fn close() -> Self {
        Self { fin: true, opcode: OpCode::Close, payload: Vec::new() }
    }
}

// stub — replaced in the GREEN step
/// Serialise a frame, masking the payload with `mask` when `Some`.
#[must_use]
pub fn encode(_frame: &Frame, _mask: Option<[u8; 4]>) -> Vec<u8> {
    Vec::new()
}

// stub — replaced in the GREEN step
/// Parse one frame from `buf`. `Ok(None)` means "need more bytes".
pub fn decode(_buf: &[u8]) -> Result<Option<(Frame, usize)>, FrameError> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(frame: &Frame, mask: Option<[u8; 4]>) {
        let bytes = encode(frame, mask);
        let (decoded, consumed) = decode(&bytes).unwrap().expect("complete frame");
        assert_eq!(consumed, bytes.len());
        assert_eq!(&decoded, frame);
    }

    #[test]
    fn roundtrips_small_binary_masked_and_unmasked() {
        let f = Frame::binary(b"hello cri".to_vec());
        roundtrip(&f, Some([0x37, 0xfa, 0x21, 0x3d]));
        roundtrip(&f, None);
    }

    #[test]
    fn roundtrips_16bit_length() {
        let f = Frame::binary(vec![0xAB; 1000]);
        roundtrip(&f, Some([1, 2, 3, 4]));
    }

    #[test]
    fn roundtrips_64bit_length() {
        let f = Frame::binary(vec![0xCD; 70_000]);
        roundtrip(&f, Some([9, 8, 7, 6]));
    }

    #[test]
    fn roundtrips_control_frames() {
        roundtrip(&Frame::close(), Some([5, 5, 5, 5]));
        roundtrip(
            &Frame { fin: true, opcode: OpCode::Ping, payload: b"pp".to_vec() },
            Some([0, 0, 0, 0]),
        );
        roundtrip(
            &Frame { fin: true, opcode: OpCode::Pong, payload: vec![] },
            None,
        );
    }

    #[test]
    fn masking_actually_obscures_payload() {
        // A non-zero key must change the on-wire bytes vs the unmasked form.
        let f = Frame::binary(b"secret".to_vec());
        let masked = encode(&f, Some([0x11, 0x22, 0x33, 0x44]));
        assert!(!masked.windows(6).any(|w| w == b"secret"));
        // The MASK bit (0x80 of byte 1) must be set.
        assert_eq!(masked[1] & 0x80, 0x80);
    }

    #[test]
    fn decode_reports_incomplete() {
        let bytes = encode(&Frame::binary(vec![0u8; 300]), Some([1, 1, 1, 1]));
        // Hand decode only a prefix: header present, payload truncated.
        assert_eq!(decode(&bytes[..5]).unwrap(), None);
    }

    #[test]
    fn decode_consumes_only_one_frame() {
        let mut buf = encode(&Frame::binary(b"a".to_vec()), Some([2, 2, 2, 2]));
        let tail = encode(&Frame::binary(b"bb".to_vec()), Some([3, 3, 3, 3]));
        buf.extend_from_slice(&tail);
        let (f, consumed) = decode(&buf).unwrap().unwrap();
        assert_eq!(f.payload, b"a");
        assert_eq!(&buf[consumed..], tail.as_slice());
    }

    #[test]
    fn decode_rejects_reserved_opcode() {
        // FIN + opcode 0x3 (reserved), zero length, unmasked.
        let bytes = [0x83u8, 0x00];
        assert!(matches!(decode(&bytes), Err(FrameError::BadOpcode(3))));
    }
}
