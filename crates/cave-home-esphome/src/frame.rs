// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `ESPHome` plaintext native-API frame codec.
//!
//! Every native-API message travels in a tiny frame:
//!
//! ```text
//!   0x00            preamble (plaintext; 0x01 = Noise-encrypted, unsupported here)
//!   varint          payload length, in bytes
//!   varint          message type (see [`crate::message::MessageType`])
//!   <payload>       the protobuf message body
//! ```
//!
//! Encoding is a straight serialisation. Decoding is *streaming-aware*: a TCP
//! read rarely lands on a frame boundary, so [`ApiFrame::decode`] reports
//! [`FrameDecode::Incomplete`] when the buffer holds only part of a frame, and
//! returns how many bytes a complete frame consumed so the caller can drain
//! exactly that much and keep any trailing bytes for the next frame.

use crate::EsphomeError;
use crate::varint;

/// Plaintext indicator: the frame body is an unencrypted protobuf message.
const PLAINTEXT_PREAMBLE: u8 = 0x00;
/// Noise indicator: the frame body is encrypted. Phase-1b (not handled here).
const NOISE_PREAMBLE: u8 = 0x01;

/// A single decoded (or to-be-encoded) native-API message frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiFrame {
    /// The `api.proto` message-type id (see [`crate::message::MessageType`]).
    pub message_type: u32,
    /// The raw protobuf message body (this crate frames it; decoding the body
    /// itself is Phase-2).
    pub payload: Vec<u8>,
}

/// The result of attempting to decode one frame from a byte buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameDecode {
    /// A complete frame was read; `consumed` leading bytes belong to it.
    Frame {
        /// The decoded frame.
        frame: ApiFrame,
        /// How many leading bytes of the input the frame occupied.
        consumed: usize,
    },
    /// The buffer does not yet contain a whole frame — read more and retry.
    Incomplete,
}

impl ApiFrame {
    /// Build a frame from a message type and payload.
    #[must_use]
    pub const fn new(message_type: u32, payload: Vec<u8>) -> Self {
        Self { message_type, payload }
    }

    /// Serialise the frame to a fresh byte vector.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            1 + varint::encoded_len(self.payload.len() as u32)
                + varint::encoded_len(self.message_type)
                + self.payload.len(),
        );
        self.encode_into(&mut out);
        out
    }

    /// Serialise the frame, appending to an existing buffer.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.push(PLAINTEXT_PREAMBLE);
        varint::encode(self.payload.len() as u32, out);
        varint::encode(self.message_type, out);
        out.extend_from_slice(&self.payload);
    }

    /// Try to decode one frame from the front of `buf`.
    ///
    /// # Errors
    ///
    /// Returns [`EsphomeError::EncryptedFrame`] for a Noise (`0x01`) preamble,
    /// [`EsphomeError::BadPreamble`] for any other non-plaintext first byte, and
    /// [`EsphomeError::VarintOverflow`] for a malformed length/type field. A
    /// merely-truncated buffer is **not** an error — it is
    /// [`FrameDecode::Incomplete`].
    pub fn decode(buf: &[u8]) -> Result<FrameDecode, EsphomeError> {
        let Some((&preamble, rest)) = buf.split_first() else {
            return Ok(FrameDecode::Incomplete);
        };
        match preamble {
            PLAINTEXT_PREAMBLE => {}
            NOISE_PREAMBLE => return Err(EsphomeError::EncryptedFrame),
            other => return Err(EsphomeError::BadPreamble(other)),
        }

        let Some((payload_len, len_n)) = varint::decode(rest)? else {
            return Ok(FrameDecode::Incomplete);
        };
        let after_len = &rest[len_n..];

        let Some((message_type, type_n)) = varint::decode(after_len)? else {
            return Ok(FrameDecode::Incomplete);
        };
        let after_type = &after_len[type_n..];

        let payload_len = payload_len as usize;
        if after_type.len() < payload_len {
            return Ok(FrameDecode::Incomplete);
        }

        let payload = after_type[..payload_len].to_vec();
        let consumed = 1 + len_n + type_n + payload_len;
        Ok(FrameDecode::Frame {
            frame: Self { message_type, payload },
            consumed,
        })
    }
}
