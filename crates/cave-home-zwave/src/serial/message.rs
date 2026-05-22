// SPDX-License-Identifier: Apache-2.0
//! Serial API message (data-frame) envelope.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/serial/src/message/Message.ts
//!
//! A Z-Wave Serial API data frame (INS12350 §5):
//!
//! ```text
//!   SOF | LEN | TYPE | FN | <payload …> | CHK
//!   1B  | 1B  | 1B   | 1B | LEN-3 bytes | 1B
//! ```
//!
//! - `LEN` covers `TYPE`, `FN`, the payload, and `CHK` (i.e. everything after
//!   itself except `SOF`). The minimum legal frame is 5 bytes (LEN = 3, empty
//!   payload).
//! - `CHK` is `0xff` XOR-folded over every byte except `SOF` and the checksum
//!   byte itself.
//!
//! See [`compute_checksum`] for the byte-for-byte port of the upstream
//! function (`Message.ts::computeChecksum`).

use bytes::{BufMut, Bytes, BytesMut};

use super::constants::{FunctionType, MessageType};
use crate::error::{ZwaveError, ZwaveResult};

/// Compute the XOR checksum of a serialized frame.
///
/// # Upstream: `Message.ts::computeChecksum`
///
/// ```ignore
/// // function computeChecksum(message: BytesView): number {
/// //   let ret = 0xff;
/// //   // exclude SOF and checksum byte from the computation
/// //   for (let i = 1; i < message.length - 1; i++) {
/// //     ret ^= message[i];
/// //   }
/// //   return ret;
/// // }
/// ```
#[must_use]
pub fn compute_checksum(frame: &[u8]) -> u8 {
    let mut ret = 0xff_u8;
    if frame.len() < 2 {
        return ret;
    }
    for &b in &frame[1..frame.len() - 1] {
        ret ^= b;
    }
    ret
}

/// Pre-parse view of a data frame.
///
/// # Upstream: `Message.ts::MessageRaw`
///
/// A `MessageRaw` is the byte-accurate decomposition of a complete frame — it
/// only knows the type / function / payload, not what command lives inside
/// the payload. [`super::message::Message`] is built on top of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageRaw {
    /// The `TYPE` byte (request vs response).
    pub message_type: MessageType,
    /// The `FN` byte.
    pub function_type: FunctionType,
    /// The payload (everything between `FN` and `CHK`).
    pub payload: Bytes,
}

impl MessageRaw {
    /// Parse the leading frame out of `data`. The frame must be complete (use
    /// the streaming [`super::parser::SerialApiParser`] if you only have a
    /// growing buffer of bytes from a UART).
    ///
    /// # Upstream: `Message.ts::MessageRaw.parse`
    ///
    /// # Errors
    /// Returns [`ZwaveError::PacketFormat`] for truncated or malformed
    /// frames, and [`ZwaveError::PacketChecksum`] if the trailing checksum
    /// does not match the computed XOR over the message.
    pub fn parse(data: &[u8]) -> ZwaveResult<Self> {
        // SOF, length, type, commandId and checksum must be present
        if data.len() < 5 {
            return Err(ZwaveError::PacketFormat(
                "truncated frame: minimum 5 bytes".into(),
            ));
        }
        if data[0] != super::constants::MessageHeader::Sof.as_u8() {
            return Err(ZwaveError::PacketFormat(format!(
                "frame does not start with SOF (got 0x{:02x})",
                data[0]
            )));
        }
        let message_length = usize::from(data[1]) + 2;
        if data.len() < message_length {
            return Err(ZwaveError::PacketFormat(
                "truncated frame: length byte exceeds available bytes".into(),
            ));
        }
        let frame = &data[..message_length];
        let expected = compute_checksum(frame);
        let got = frame[message_length - 1];
        if got != expected {
            return Err(ZwaveError::PacketChecksum { expected, got });
        }
        let message_type = MessageType::from_u8(frame[2]).ok_or_else(|| {
            ZwaveError::PacketFormat(format!("unknown MessageType byte 0x{:02x}", frame[2]))
        })?;
        let function_type = FunctionType::from_u8(frame[3]);
        let payload_len = message_length - 5;
        let payload = Bytes::copy_from_slice(&frame[4..4 + payload_len]);
        Ok(Self {
            message_type,
            function_type,
            payload,
        })
    }

    /// Number of bytes this frame would occupy on the wire (`LEN + 2`).
    #[must_use]
    pub fn wire_length(&self) -> usize {
        // SOF + LEN + TYPE + FN + payload + CHK
        5 + self.payload.len()
    }
}

/// Wire-level message envelope (Serial API data frame).
///
/// # Upstream: `Message.ts::Message`
///
/// This is the host-side view of a frame. It is intentionally thin —
/// command-class decoding lives in [`crate::cc`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    /// Request vs Response.
    pub message_type: MessageType,
    /// `FunctionType` byte.
    pub function_type: FunctionType,
    /// Inner payload bytes (without framing).
    pub payload: Bytes,
}

impl Message {
    /// Build a Request-class message with the given function and payload.
    #[must_use]
    pub fn request(function_type: FunctionType, payload: Bytes) -> Self {
        Self {
            message_type: MessageType::Request,
            function_type,
            payload,
        }
    }

    /// Build a Response-class message with the given function and payload.
    #[must_use]
    pub fn response(function_type: FunctionType, payload: Bytes) -> Self {
        Self {
            message_type: MessageType::Response,
            function_type,
            payload,
        }
    }

    /// Serialize this message into a fully framed Z-Wave Serial API data frame
    /// (`SOF | LEN | TYPE | FN | payload | CHK`).
    ///
    /// # Upstream: `Message.ts::Message.serialize`
    ///
    /// # Errors
    /// Returns [`ZwaveError::Argument`] if the payload is longer than 252
    /// bytes — the Serial API encodes `LEN` in a single byte and `LEN`
    /// covers `TYPE + FN + payload + CHK`, so the maximum payload is
    /// `0xff - 3 = 252`.
    pub fn serialize(&self) -> ZwaveResult<Bytes> {
        if self.payload.len() > 252 {
            return Err(ZwaveError::Argument(format!(
                "payload too large: {} bytes (max 252)",
                self.payload.len()
            )));
        }
        let total = self.payload.len() + 5;
        let mut buf = BytesMut::with_capacity(total);
        buf.put_u8(super::constants::MessageHeader::Sof.as_u8());
        #[allow(clippy::cast_possible_truncation)]
        buf.put_u8((self.payload.len() + 3) as u8);
        buf.put_u8(self.message_type as u8);
        buf.put_u8(self.function_type.as_u8());
        buf.put_slice(&self.payload);
        buf.put_u8(0); // checksum slot, overwritten below
        let chk = compute_checksum(&buf);
        let last = buf.len() - 1;
        buf[last] = chk;
        Ok(buf.freeze())
    }

    /// Parse a Z-Wave Serial API data frame from the leading bytes of `data`.
    ///
    /// # Upstream: `Message.ts::Message.parse`
    ///
    /// # Errors
    /// Mirrors [`MessageRaw::parse`].
    pub fn parse(data: &[u8]) -> ZwaveResult<Self> {
        let raw = MessageRaw::parse(data)?;
        Ok(Self {
            message_type: raw.message_type,
            function_type: raw.function_type,
            payload: raw.payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-port of `Message.test.ts::checksum`. The XOR-over-bytes-1..n-1
    /// invariant is the most-tested rule in the upstream serial layer.
    #[test]
    fn checksum_xors_all_bytes_except_sof_and_chk_slot() {
        // SOF=0x01, LEN=0x03, TYPE=0x00, FN=0x07, CHK=?
        let frame = [0x01, 0x03, 0x00, 0x07, 0x00];
        let chk = compute_checksum(&frame);
        // 0xff ^ 0x03 ^ 0x00 ^ 0x07 = 0xfb
        assert_eq!(chk, 0xfb);
    }

    #[test]
    fn checksum_empty_returns_ff() {
        assert_eq!(compute_checksum(&[]), 0xff);
        assert_eq!(compute_checksum(&[0xab]), 0xff);
    }

    #[test]
    fn message_round_trips_through_wire() {
        let m = Message::request(
            FunctionType::GetControllerId,
            Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]),
        );
        let wire = m.serialize().unwrap();
        let back = Message::parse(&wire).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn message_parse_rejects_short_frame() {
        let err = Message::parse(&[0x01, 0x03, 0x00]).unwrap_err();
        assert!(matches!(err, ZwaveError::PacketFormat(_)));
    }

    #[test]
    fn message_parse_rejects_bad_sof() {
        let err = Message::parse(&[0xff, 0x03, 0x00, 0x07, 0xfb]).unwrap_err();
        assert!(matches!(err, ZwaveError::PacketFormat(_)));
    }

    #[test]
    fn message_parse_rejects_bad_checksum() {
        // Same as `checksum_xors_all_bytes_except_sof_and_chk_slot` but
        // tamper with the checksum byte.
        let err = Message::parse(&[0x01, 0x03, 0x00, 0x07, 0xaa]).unwrap_err();
        match err {
            ZwaveError::PacketChecksum { expected, got } => {
                assert_eq!(expected, 0xfb);
                assert_eq!(got, 0xaa);
            }
            other => panic!("expected PacketChecksum, got {other:?}"),
        }
    }

    #[test]
    fn serialize_rejects_oversize_payload() {
        let m = Message::request(FunctionType::SendData, Bytes::from(vec![0u8; 253]));
        let err = m.serialize().unwrap_err();
        assert!(matches!(err, ZwaveError::Argument(_)));
    }

    #[test]
    fn serialize_max_payload_succeeds() {
        let m = Message::request(FunctionType::SendData, Bytes::from(vec![0u8; 252]));
        let wire = m.serialize().unwrap();
        assert_eq!(wire.len(), 257);
        assert_eq!(wire[0], 0x01);
        assert_eq!(wire[1], 0xff);
    }

    #[test]
    fn unknown_function_type_round_trips_as_other() {
        let m = Message::request(FunctionType::Other(0xc4), Bytes::from_static(&[0x11]));
        let wire = m.serialize().unwrap();
        let back = Message::parse(&wire).unwrap();
        assert_eq!(back.function_type, FunctionType::Other(0xc4));
    }
}
