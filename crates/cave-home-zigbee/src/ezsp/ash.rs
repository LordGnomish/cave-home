// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! ASH (Asynchronous Serial Host) framer — Silicon Labs UG100 §5.
//!
//! ASH wraps EZSP application frames inside a small reliability layer
//! suitable for raw UART links. Frames are delimited by `0x7e`, every
//! 0x7d / 0x7e / 0x11 / 0x13 / 0x18 / 0x1a byte inside the payload is
//! escaped, and each frame ends with a CRC-16/CCITT (poly 0x1021,
//! init 0xffff) computed over the unstuffed bytes.
//!
//! Phase 1 implements the DATA frame type plus the framing primitives
//! (byte stuffing + CRC). Higher-level connection management (RST /
//! RSTACK / ACK / NAK sliding window) is implemented at a level above
//! by [`AshFramer`], but the connection FSM proper lands in Phase 1b
//! (the test bench drives a Mock NCP that ignores sequence wrap).

use bytes::BytesMut;

use crate::error::{Result, ZigbeeError};

/// Frame delimiter byte (UG100 §5.2.1).
pub const FLAG: u8 = 0x7e;
/// Escape byte (UG100 §5.2.2).
pub const ESCAPE: u8 = 0x7d;
/// XON byte (filtered + escaped — UG100 §5.2.3).
pub const XON: u8 = 0x11;
/// XOFF byte.
pub const XOFF: u8 = 0x13;
/// SUBSTITUTE byte.
pub const SUBSTITUTE: u8 = 0x18;
/// CANCEL byte.
pub const CANCEL: u8 = 0x1a;

/// A decoded ASH frame (control byte + unstuffed payload).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AshFrame {
    /// Control byte (UG100 §5.3).
    pub control: u8,
    /// Frame payload (post-unstuff, CRC validated).
    pub payload: Vec<u8>,
}

impl AshFrame {
    /// Build a DATA frame with the given control byte and EZSP payload.
    #[must_use]
    pub fn data(control: u8, payload: Vec<u8>) -> Self {
        Self { control, payload }
    }
}

/// Compute the CRC-16/CCITT (init 0xffff, poly 0x1021, MSB-first) over `bytes`.
///
/// Public so callers exercising the wire format (e.g. integration
/// tests against a real NCP) can verify checksums.
#[must_use]
pub fn crc_ccitt(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &b in bytes {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Encode `bytes` with ASH byte-stuffing (no CRC, no FLAG sentinel).
///
/// The transformation: for every byte in {FLAG, ESCAPE, XON, XOFF,
/// SUBSTITUTE, CANCEL}, emit ESCAPE then byte ^ 0x20.
#[must_use]
pub fn byte_stuff(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 8);
    for &b in bytes {
        if matches!(b, FLAG | ESCAPE | XON | XOFF | SUBSTITUTE | CANCEL) {
            out.push(ESCAPE);
            out.push(b ^ 0x20);
        } else {
            out.push(b);
        }
    }
    out
}

/// Undo ASH byte-stuffing. Returns the original bytes.
///
/// # Errors
/// Returns [`ZigbeeError::Integrity`] if an ESCAPE is at the end of input
/// (no byte to un-escape).
pub fn byte_unstuff(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == ESCAPE {
            if i + 1 >= bytes.len() {
                return Err(ZigbeeError::Integrity("trailing ASH escape"));
            }
            out.push(bytes[i + 1] ^ 0x20);
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(out)
}

/// Stateful framer that turns a contiguous byte stream into [`AshFrame`]s.
///
/// `feed` accumulates bytes and emits any newly decoded frames (one or
/// more, in order). Garbage between FLAG bytes is silently dropped per
/// UG100 §5.2.1.
pub struct AshFramer {
    buf: BytesMut,
}

impl Default for AshFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl AshFramer {
    /// Construct an empty framer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    /// Feed more bytes; return the (possibly empty) list of decoded frames.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Integrity`] on a CRC mismatch or a
    /// truncated escape sequence.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<AshFrame>> {
        self.buf.extend_from_slice(chunk);
        let mut frames = Vec::new();
        loop {
            // Find next FLAG.
            let Some(end) = self.buf.iter().position(|b| *b == FLAG) else {
                break;
            };
            // Slice the bytes up to (but not including) end into a frame body.
            let body: Vec<u8> = self.buf[..end].to_vec();
            // Consume body + FLAG from buf.
            let _ = self.buf.split_to(end + 1);
            // Discard CANCEL/SUBSTITUTE-only frames per UG100 §5.4.
            if body.is_empty() {
                continue;
            }
            let unstuffed = byte_unstuff(&body)?;
            if unstuffed.len() < 3 {
                return Err(ZigbeeError::Integrity("ash frame too short"));
            }
            let (data, crc_bytes) = unstuffed.split_at(unstuffed.len() - 2);
            let observed = u16::from_be_bytes([crc_bytes[0], crc_bytes[1]]);
            let expected = crc_ccitt(data);
            if observed != expected {
                return Err(ZigbeeError::Integrity("ash crc mismatch"));
            }
            let control = data[0];
            let payload = data[1..].to_vec();
            frames.push(AshFrame { control, payload });
        }
        Ok(frames)
    }

    /// Encode an [`AshFrame`] for transmission (control + payload + CRC, byte-stuffed, FLAG-terminated).
    #[must_use]
    pub fn encode(frame: &AshFrame) -> Vec<u8> {
        let mut data = Vec::with_capacity(1 + frame.payload.len() + 2);
        data.push(frame.control);
        data.extend_from_slice(&frame.payload);
        let crc = crc_ccitt(&data);
        data.extend_from_slice(&crc.to_be_bytes());
        let mut stuffed = byte_stuff(&data);
        stuffed.push(FLAG);
        stuffed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_ccitt_known_vector() {
        // Spec test vector: CRC over the single byte 0x00 with init 0xffff
        // is 0xe1f0; over "123456789" the CRC-16/CCITT-FALSE is 0x29b1.
        assert_eq!(crc_ccitt(b"123456789"), 0x29b1);
    }

    #[test]
    fn byte_stuff_round_trip_no_special_bytes() {
        let original = b"\x01\x02\x03\xab";
        let stuffed = byte_stuff(original);
        assert_eq!(stuffed, original);
        let back = byte_unstuff(&stuffed).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn byte_stuff_escapes_flag() {
        let stuffed = byte_stuff(&[0x7e]);
        assert_eq!(stuffed, vec![0x7d, 0x5e]);
        assert_eq!(byte_unstuff(&stuffed).unwrap(), vec![0x7e]);
    }

    #[test]
    fn byte_stuff_escapes_xon_and_xoff() {
        let stuffed = byte_stuff(&[XON, XOFF]);
        assert_eq!(stuffed, vec![ESCAPE, XON ^ 0x20, ESCAPE, XOFF ^ 0x20]);
        assert_eq!(byte_unstuff(&stuffed).unwrap(), vec![XON, XOFF]);
    }

    #[test]
    fn byte_unstuff_rejects_trailing_escape() {
        assert!(matches!(
            byte_unstuff(&[ESCAPE]),
            Err(ZigbeeError::Integrity(_))
        ));
    }

    #[test]
    fn framer_round_trip_single_frame() {
        let f = AshFrame::data(0x81, vec![0x00, 0x01, 0x02]);
        let bytes = AshFramer::encode(&f);
        // Last byte must be FLAG.
        assert_eq!(*bytes.last().unwrap(), FLAG);
        let mut framer = AshFramer::new();
        let decoded = framer.feed(&bytes).unwrap();
        assert_eq!(decoded, vec![f]);
    }

    #[test]
    fn framer_handles_two_frames_in_one_chunk() {
        let f1 = AshFrame::data(0x10, vec![0xaa]);
        let f2 = AshFrame::data(0x11, vec![0xbb, 0xcc]);
        let mut chunk = AshFramer::encode(&f1);
        chunk.extend(AshFramer::encode(&f2));
        let mut framer = AshFramer::new();
        let frames = framer.feed(&chunk).unwrap();
        assert_eq!(frames, vec![f1, f2]);
    }

    #[test]
    fn framer_handles_split_chunks() {
        let f = AshFrame::data(0x20, vec![0x7e, 0x7d, 0x11]);
        let bytes = AshFramer::encode(&f);
        let (head, tail) = bytes.split_at(3);
        let mut framer = AshFramer::new();
        assert!(framer.feed(head).unwrap().is_empty());
        let frames = framer.feed(tail).unwrap();
        assert_eq!(frames, vec![f]);
    }

    #[test]
    fn framer_detects_crc_corruption() {
        let f = AshFrame::data(0x30, vec![0xde, 0xad]);
        let mut bytes = AshFramer::encode(&f);
        // Flip a byte before the trailing FLAG.
        let last_data_idx = bytes.len() - 2;
        bytes[last_data_idx] ^= 0x01;
        let mut framer = AshFramer::new();
        assert!(matches!(
            framer.feed(&bytes),
            Err(ZigbeeError::Integrity(_))
        ));
    }

    #[test]
    fn framer_drops_garbage_between_flags() {
        // Garbage then valid frame.
        let valid = AshFramer::encode(&AshFrame::data(0x42, vec![0x01]));
        let mut chunk = vec![FLAG, FLAG]; // empty garbage frames
        chunk.extend(valid);
        let mut framer = AshFramer::new();
        let frames = framer.feed(&chunk).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].control, 0x42);
    }
}
