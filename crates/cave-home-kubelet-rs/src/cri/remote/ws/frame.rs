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
    pub const fn binary(payload: Vec<u8>) -> Self {
        Self { fin: true, opcode: OpCode::Binary, payload }
    }

    /// Convenience: a close frame with FIN set.
    #[must_use]
    pub const fn close() -> Self {
        Self { fin: true, opcode: OpCode::Close, payload: Vec::new() }
    }
}

/// Upper bound on a single decoded frame (16 MiB) — guards the 64-bit length
/// field against allocating an attacker-chosen amount of memory.
const MAX_FRAME_LEN: u64 = 16 * 1024 * 1024;

const fn opcode_bits(op: OpCode) -> u8 {
    match op {
        OpCode::Continuation => 0x0,
        OpCode::Text => 0x1,
        OpCode::Binary => 0x2,
        OpCode::Close => 0x8,
        OpCode::Ping => 0x9,
        OpCode::Pong => 0xA,
    }
}

const fn opcode_from_bits(bits: u8) -> Option<OpCode> {
    match bits {
        0x0 => Some(OpCode::Continuation),
        0x1 => Some(OpCode::Text),
        0x2 => Some(OpCode::Binary),
        0x8 => Some(OpCode::Close),
        0x9 => Some(OpCode::Ping),
        0xA => Some(OpCode::Pong),
        _ => None,
    }
}

/// Serialise a frame, masking the payload with `mask` when `Some`.
// The length-field casts are bounded by the explicit `< 126` / `<= u16::MAX`
// guards, so each truncating cast is provably lossless on the branch taken.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn encode(frame: &Frame, mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(frame.payload.len() + 14);
    let fin_bit = if frame.fin { 0x80 } else { 0x00 };
    out.push(fin_bit | opcode_bits(frame.opcode));

    let mask_bit = if mask.is_some() { 0x80 } else { 0x00 };
    let len = frame.payload.len();
    if len < 126 {
        out.push(mask_bit | len as u8);
    } else if let Ok(l) = u16::try_from(len) {
        out.push(mask_bit | 0x7E);
        out.extend_from_slice(&l.to_be_bytes());
    } else {
        out.push(mask_bit | 0x7F);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }

    match mask {
        Some(key) => {
            out.extend_from_slice(&key);
            out.extend(
                frame
                    .payload
                    .iter()
                    .enumerate()
                    .map(|(i, b)| b ^ key[i % 4]),
            );
        }
        None => out.extend_from_slice(&frame.payload),
    }
    out
}

/// Parse one frame from `buf`. `Ok(None)` means "need more bytes".
///
/// # Errors
/// Returns [`FrameError::BadOpcode`] for a reserved opcode and
/// [`FrameError::TooLarge`] if the advertised length exceeds [`MAX_FRAME_LEN`].
// `len as usize` follows a `len > MAX_FRAME_LEN` (16 MiB) guard, so it never
// truncates on any pointer width this crate targets.
#[allow(clippy::cast_possible_truncation, clippy::many_single_char_names)]
pub fn decode(buf: &[u8]) -> Result<Option<(Frame, usize)>, FrameError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let b0 = buf[0];
    let b1 = buf[1];
    let fin = b0 & 0x80 != 0;
    let opcode = opcode_from_bits(b0 & 0x0F).ok_or(FrameError::BadOpcode(b0 & 0x0F))?;
    let masked = b1 & 0x80 != 0;

    let mut cursor = 2;
    let len = match b1 & 0x7F {
        126 => {
            if buf.len() < cursor + 2 {
                return Ok(None);
            }
            let l = u64::from(u16::from_be_bytes([buf[cursor], buf[cursor + 1]]));
            cursor += 2;
            l
        }
        127 => {
            if buf.len() < cursor + 8 {
                return Ok(None);
            }
            let l = u64::from_be_bytes(buf[cursor..cursor + 8].try_into().unwrap_or([0; 8]));
            cursor += 8;
            l
        }
        other => u64::from(other),
    };
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    let len = len as usize;

    let key = if masked {
        if buf.len() < cursor + 4 {
            return Ok(None);
        }
        let k = [buf[cursor], buf[cursor + 1], buf[cursor + 2], buf[cursor + 3]];
        cursor += 4;
        Some(k)
    } else {
        None
    };

    if buf.len() < cursor + len {
        return Ok(None);
    }
    let raw = &buf[cursor..cursor + len];
    let payload = key.map_or_else(
        || raw.to_vec(),
        |k| raw.iter().enumerate().map(|(i, b)| b ^ k[i % 4]).collect(),
    );
    Ok(Some((Frame { fin, opcode, payload }, cursor + len)))
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
