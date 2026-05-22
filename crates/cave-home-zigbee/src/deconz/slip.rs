// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! SLIP framer — RFC 1055 + deCONZ serial protocol §3.
//!
//! SLIP framing rules (RFC 1055):
//! - A frame is terminated by an END byte (0xc0).
//! - END bytes inside the payload are escaped as ESC (0xdb), ESC_END (0xdc).
//! - ESC bytes inside the payload are escaped as ESC, ESC_ESC (0xdd).
//!
//! deCONZ wraps each SLIP frame around a (command id, sequence, status,
//! frame length, payload, CRC-16) payload. This module is only about
//! the SLIP delimiter / escape layer; the deCONZ command layer above
//! parses the contents.

use bytes::BytesMut;

use crate::error::{Result, ZigbeeError};

/// SLIP frame delimiter (RFC 1055 END).
pub const SLIP_END: u8 = 0xc0;
/// SLIP escape byte (RFC 1055 ESC).
pub const SLIP_ESC: u8 = 0xdb;
/// SLIP escape-for-END (RFC 1055 ESC_END).
pub const SLIP_ESC_END: u8 = 0xdc;
/// SLIP escape-for-ESC (RFC 1055 ESC_ESC).
pub const SLIP_ESC_ESC: u8 = 0xdd;

/// Encode `payload` as a single SLIP-framed packet.
///
/// The result starts with END (the spec recommends an initial END to
/// flush any prior garbage on the line) and ends with END.
#[must_use]
pub fn encode(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.push(SLIP_END);
    for &b in payload {
        match b {
            SLIP_END => {
                out.push(SLIP_ESC);
                out.push(SLIP_ESC_END);
            }
            SLIP_ESC => {
                out.push(SLIP_ESC);
                out.push(SLIP_ESC_ESC);
            }
            other => out.push(other),
        }
    }
    out.push(SLIP_END);
    out
}

/// Stateful SLIP framer. Feed bytes via [`SlipFramer::feed`] and collect
/// completed payloads (any number, in order).
pub struct SlipFramer {
    buf: BytesMut,
}

impl Default for SlipFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl SlipFramer {
    /// Construct an empty framer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    /// Feed bytes; return any decoded payloads (with escape sequences resolved).
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Integrity`] for a malformed escape sequence.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            let Some(end) = self.buf.iter().position(|b| *b == SLIP_END) else {
                break;
            };
            let body: Vec<u8> = self.buf[..end].to_vec();
            let _ = self.buf.split_to(end + 1);
            if body.is_empty() {
                // Initial END markers / inter-frame gaps.
                continue;
            }
            out.push(decode(&body)?);
        }
        Ok(out)
    }
}

/// Decode the SLIP-escaped body bytes (between two END markers).
///
/// # Errors
/// Returns [`ZigbeeError::Integrity`] on an invalid escape sequence.
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            SLIP_END => {
                return Err(ZigbeeError::Integrity("unexpected SLIP END in body"));
            }
            SLIP_ESC => {
                if i + 1 >= bytes.len() {
                    return Err(ZigbeeError::Integrity("trailing SLIP escape"));
                }
                match bytes[i + 1] {
                    SLIP_ESC_END => out.push(SLIP_END),
                    SLIP_ESC_ESC => out.push(SLIP_ESC),
                    other => {
                        return Err(ZigbeeError::Integrity(if other == 0 {
                            "bogus SLIP escape (00)"
                        } else {
                            "unknown SLIP escape"
                        }));
                    }
                }
                i += 2;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip_no_special_bytes() {
        let payload = b"hello";
        let framed = encode(payload);
        assert_eq!(framed.first(), Some(&SLIP_END));
        assert_eq!(framed.last(), Some(&SLIP_END));
        let mut framer = SlipFramer::new();
        let decoded = framer.feed(&framed).unwrap();
        assert_eq!(decoded, vec![payload.to_vec()]);
    }

    #[test]
    fn end_byte_in_payload_gets_escaped() {
        let payload = vec![SLIP_END];
        let framed = encode(&payload);
        // Body should be [ESC, ESC_END].
        let body = &framed[1..framed.len() - 1];
        assert_eq!(body, &[SLIP_ESC, SLIP_ESC_END]);
        let mut framer = SlipFramer::new();
        assert_eq!(framer.feed(&framed).unwrap(), vec![payload]);
    }

    #[test]
    fn esc_byte_in_payload_gets_escaped() {
        let payload = vec![SLIP_ESC];
        let framed = encode(&payload);
        let body = &framed[1..framed.len() - 1];
        assert_eq!(body, &[SLIP_ESC, SLIP_ESC_ESC]);
        let mut framer = SlipFramer::new();
        assert_eq!(framer.feed(&framed).unwrap(), vec![payload]);
    }

    #[test]
    fn framer_handles_multiple_frames_in_one_chunk() {
        let mut chunk = encode(b"one");
        chunk.extend(encode(b"two"));
        let mut framer = SlipFramer::new();
        let frames = framer.feed(&chunk).unwrap();
        assert_eq!(frames, vec![b"one".to_vec(), b"two".to_vec()]);
    }

    #[test]
    fn framer_assembles_across_split_chunks() {
        let framed = encode(&[0x01, 0x02, 0x03]);
        let (a, b) = framed.split_at(framed.len() / 2);
        let mut framer = SlipFramer::new();
        let first = framer.feed(a).unwrap();
        assert!(first.is_empty());
        let second = framer.feed(b).unwrap();
        assert_eq!(second, vec![vec![0x01, 0x02, 0x03]]);
    }

    #[test]
    fn decode_rejects_trailing_escape() {
        assert!(matches!(
            decode(&[SLIP_ESC]),
            Err(ZigbeeError::Integrity(_))
        ));
    }

    #[test]
    fn decode_rejects_unknown_escape() {
        assert!(matches!(
            decode(&[SLIP_ESC, 0x00]),
            Err(ZigbeeError::Integrity(_))
        ));
    }
}
