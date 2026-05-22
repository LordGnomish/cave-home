// SPDX-License-Identifier: Apache-2.0
//! Streaming Serial API parser.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/serial/src/parsers/SerialAPIParser.ts
//!
//! Re-implementation of upstream's `SerialAPIParserTransformer` as a small
//! synchronous state machine. The driver feeds UART bytes into [`SerialApiParser::feed`]
//! and pulls completed frames out of the returned `Vec<SerialApiChunk>`.

use bytes::Bytes;

use super::constants::MessageHeader;

/// A single chunk emitted by [`SerialApiParser`].
///
/// # Upstream: `SerialAPIParser.ts::SerialAPIChunk` (and the
/// `ZWaveSerialFrameType` discriminator).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerialApiChunk {
    /// A single-byte signalling header (ACK / NAK / CAN).
    Signal(MessageHeader),
    /// A complete SOF-led data frame, exactly `LEN + 2` bytes long.
    Frame(Bytes),
    /// Stray bytes that fell outside the legal vocabulary — preserved so the
    /// driver can log them, the way upstream's `discarded` log entry does.
    Discarded(Bytes),
}

/// Streaming parser.
///
/// # Upstream: `SerialAPIParser.ts::SerialAPIParserTransformer`
#[derive(Debug, Default)]
pub struct SerialApiParser {
    receive_buffer: Vec<u8>,
    /// Set this to `true` to tolerate a corrupted high nibble on a single
    /// ACK. The 700-series firmware bug that motivates this knob is described
    /// in the upstream code comment we ported below.
    pub ignore_ack_high_nibble: bool,
}

impl SerialApiParser {
    /// Create an empty parser.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed UART bytes and drain any completed chunks.
    ///
    /// # Upstream: `SerialAPIParserTransformer.transform`
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<SerialApiChunk> {
        self.receive_buffer.extend_from_slice(bytes);
        let mut out = Vec::new();

        while !self.receive_buffer.is_empty() {
            let head = self.receive_buffer[0];

            if head != MessageHeader::Sof.as_u8() {
                let mut skip = 1;
                match head {
                    // Emit the single-byte messages directly
                    0x06 => out.push(SerialApiChunk::Signal(MessageHeader::Ack)),
                    0x15 => out.push(SerialApiChunk::Signal(MessageHeader::Nak)),
                    0x18 => out.push(SerialApiChunk::Signal(MessageHeader::Can)),
                    _ => {
                        // INS12350: A host or a Z-Wave chip waiting for new traffic MUST
                        // ignore all other byte values than 0x06 (ACK), 0x15 (NAK),
                        // 0x18 (CAN) or 0x01 (Data frame).
                        //
                        // Work around a bug in the 700 series firmware that causes the
                        // high nibble of an ACK to be corrupted after a soft reset.
                        if self.ignore_ack_high_nibble && (head & 0x0f) == 0x06 {
                            out.push(SerialApiChunk::Signal(MessageHeader::Ack));
                            self.ignore_ack_high_nibble = false;
                        } else {
                            // Scan ahead until the next valid byte and log the
                            // invalid bytes.
                            while skip < self.receive_buffer.len() {
                                let byte = self.receive_buffer[skip];
                                if byte == MessageHeader::Sof.as_u8()
                                    || byte == 0x06
                                    || byte == 0x15
                                    || byte == 0x18
                                {
                                    break;
                                }
                                skip += 1;
                            }
                            let discarded = Bytes::copy_from_slice(&self.receive_buffer[..skip]);
                            out.push(SerialApiChunk::Discarded(discarded));
                        }
                    }
                }
                self.receive_buffer.drain(..skip);
                continue;
            }

            // We start with SOF; check whether the buffer contains a complete
            // message yet.
            if self.receive_buffer.len() < 5 {
                break;
            }
            let msg_length = usize::from(self.receive_buffer[1]) + 2;
            if self.receive_buffer.len() < msg_length {
                break;
            }
            let frame = Bytes::copy_from_slice(&self.receive_buffer[..msg_length]);
            self.receive_buffer.drain(..msg_length);
            out.push(SerialApiChunk::Frame(frame));
        }

        out
    }

    /// Reset the parser state — used by the driver after a hard reset.
    pub fn reset(&mut self) {
        self.receive_buffer.clear();
        self.ignore_ack_high_nibble = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::{FunctionType, Message};

    /// Hand-port of upstream's "ack passthrough" parser test.
    #[test]
    fn single_ack_emits_signal() {
        let mut p = SerialApiParser::new();
        let chunks = p.feed(&[0x06]);
        assert_eq!(chunks, vec![SerialApiChunk::Signal(MessageHeader::Ack)]);
    }

    #[test]
    fn nak_and_can_emit_signals() {
        let mut p = SerialApiParser::new();
        let chunks = p.feed(&[0x15, 0x18]);
        assert_eq!(
            chunks,
            vec![
                SerialApiChunk::Signal(MessageHeader::Nak),
                SerialApiChunk::Signal(MessageHeader::Can)
            ]
        );
    }

    #[test]
    fn single_frame_emits_after_full_arrival() {
        let m = Message::request(FunctionType::GetControllerId, Bytes::from_static(&[0xab]));
        let wire = m.serialize().unwrap();
        let mut p = SerialApiParser::new();
        // Feed byte-by-byte; the parser must emit nothing until all 6 bytes
        // are present, then exactly one frame.
        for byte in &wire[..wire.len() - 1] {
            assert!(p.feed(&[*byte]).is_empty());
        }
        let chunks = p.feed(&wire[wire.len() - 1..]);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            SerialApiChunk::Frame(b) => assert_eq!(b, &wire),
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn ack_then_frame_arrives_together() {
        let m = Message::response(FunctionType::GetControllerVersion, Bytes::new());
        let wire = m.serialize().unwrap();
        let mut joined = vec![0x06];
        joined.extend_from_slice(&wire);
        let mut p = SerialApiParser::new();
        let chunks = p.feed(&joined);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], SerialApiChunk::Signal(MessageHeader::Ack));
        match &chunks[1] {
            SerialApiChunk::Frame(b) => assert_eq!(b, &wire),
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn garbage_bytes_emit_discarded_then_continue() {
        let mut p = SerialApiParser::new();
        let chunks = p.feed(&[0xff, 0xee, 0x06]);
        assert_eq!(chunks.len(), 2);
        match &chunks[0] {
            SerialApiChunk::Discarded(b) => assert_eq!(b.as_ref(), &[0xff, 0xee]),
            other => panic!("expected Discarded, got {other:?}"),
        }
        assert_eq!(chunks[1], SerialApiChunk::Signal(MessageHeader::Ack));
    }

    #[test]
    fn corrupted_high_nibble_ack_is_passed_through_when_workaround_is_on() {
        let mut p = SerialApiParser::new();
        p.ignore_ack_high_nibble = true;
        let chunks = p.feed(&[0x76]);
        assert_eq!(chunks, vec![SerialApiChunk::Signal(MessageHeader::Ack)]);
        // The workaround disarms itself after one use.
        assert!(!p.ignore_ack_high_nibble);
    }
}
