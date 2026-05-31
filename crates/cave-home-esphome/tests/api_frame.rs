// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Integration tests (against the public API) for the ESPHome native-API wire
//! framing: the protobuf base-128 (LEB128) `varint` codec and the plaintext
//! `frame` codec. These target functions that DO NOT YET EXIST — the RED phase
//! of strict TDD.
//!
//! Reference vectors are computed independently from the public specs (protobuf
//! varint; the ESPHome plaintext frame layout `<0x00> <varint len> <varint
//! type> <payload>`), NOT from this crate's implementation.

use cave_home_esphome::error::EsphomeError;
use cave_home_esphome::frame::{ApiFrame, FrameDecode};
use cave_home_esphome::varint;

// ---------------------------------------------------------------------------
// Protobuf base-128 (LEB128) unsigned varint.
//
//   value=0     -> 00
//   value=1     -> 01
//   value=127   -> 7f
//   value=128   -> 80 01
//   value=300   -> ac 02
//   value=16384 -> 80 80 01
//   value=2^32-1-> ff ff ff ff 0f
// ---------------------------------------------------------------------------

#[test]
fn varint_encode_matches_protobuf_vectors() {
    let cases: &[(u32, &[u8])] = &[
        (0, &[0x00]),
        (1, &[0x01]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (300, &[0xac, 0x02]),
        (16384, &[0x80, 0x80, 0x01]),
        (0xFFFF_FFFF, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
    ];
    for (value, expect) in cases {
        let mut out = Vec::new();
        varint::encode(*value, &mut out);
        assert_eq!(out, *expect, "encode({value})");
        assert_eq!(varint::encoded_len(*value), expect.len(), "encoded_len({value})");
    }
}

#[test]
fn varint_decode_round_trips_and_reports_consumed() {
    for value in [0u32, 1, 127, 128, 300, 16384, 70_000, 0xFFFF_FFFF] {
        let mut buf = Vec::new();
        varint::encode(value, &mut buf);
        let n = buf.len();
        // Add trailing junk: decode must consume only the varint's own bytes.
        buf.push(0xAA);
        buf.push(0xBB);
        let decoded = varint::decode(&buf).expect("decode ok");
        assert_eq!(decoded, Some((value, n)), "decode({value})");
    }
}

#[test]
fn varint_decode_incomplete_returns_none() {
    // 300 = ac 02; only its first (continuation) byte is present.
    let decoded = varint::decode(&[0xac]).expect("no error on short buffer");
    assert_eq!(decoded, None);
    // Empty buffer is also just "need more".
    assert_eq!(varint::decode(&[]).expect("ok"), None);
}

#[test]
fn varint_decode_overflow_is_an_error() {
    // Six continuation bytes cannot fit in a u32.
    let err = varint::decode(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]).unwrap_err();
    assert_eq!(err, EsphomeError::VarintOverflow);
}

// ---------------------------------------------------------------------------
// Plaintext native-API frame: <0x00> <varint payload-len> <varint type> <payload>
// ---------------------------------------------------------------------------

#[test]
fn frame_encode_matches_reference_bytes() {
    // type = 1 (HelloRequest), 5-byte payload.
    let payload = vec![0x12, 0x03, 0x61, 0x62, 0x63];
    let frame = ApiFrame::new(1, payload);
    // 00 (preamble) | 05 (len) | 01 (type) | 12 03 61 62 63 (payload)
    assert_eq!(frame.encode(), vec![0x00, 0x05, 0x01, 0x12, 0x03, 0x61, 0x62, 0x63]);
}

#[test]
fn frame_encode_uses_multibyte_length_varint() {
    // A 200-byte payload => length varint is c8 01 (two bytes).
    let payload = vec![0u8; 200];
    let frame = ApiFrame::new(16, payload);
    let bytes = frame.encode();
    assert_eq!(&bytes[..4], &[0x00, 0xc8, 0x01, 0x10]); // type 16 = 0x10
    assert_eq!(bytes.len(), 1 + 2 + 1 + 200);
}

#[test]
fn frame_decode_reads_a_complete_frame() {
    let bytes = [0x00, 0x05, 0x01, 0x12, 0x03, 0x61, 0x62, 0x63];
    match ApiFrame::decode(&bytes).expect("decode ok") {
        FrameDecode::Frame { frame, consumed } => {
            assert_eq!(frame.message_type, 1);
            assert_eq!(frame.payload, vec![0x12, 0x03, 0x61, 0x62, 0x63]);
            assert_eq!(consumed, bytes.len());
        }
        FrameDecode::Incomplete => panic!("expected a complete frame"),
    }
}

#[test]
fn frame_decode_consumes_only_one_frame_when_two_are_buffered() {
    let mut stream = ApiFrame::new(7, vec![0xAA]).encode(); // PingRequest
    let second = ApiFrame::new(8, vec![0xBB, 0xCC]).encode(); // PingResponse
    let first_len = stream.len();
    stream.extend_from_slice(&second);

    let FrameDecode::Frame { frame, consumed } = ApiFrame::decode(&stream).expect("ok") else {
        panic!("expected first frame");
    };
    assert_eq!(frame.message_type, 7);
    assert_eq!(frame.payload, vec![0xAA]);
    assert_eq!(consumed, first_len);

    // Draining `consumed` bytes leaves exactly the second frame.
    let rest = &stream[consumed..];
    let FrameDecode::Frame { frame, .. } = ApiFrame::decode(rest).expect("ok") else {
        panic!("expected second frame");
    };
    assert_eq!(frame.message_type, 8);
    assert_eq!(frame.payload, vec![0xBB, 0xCC]);
}

#[test]
fn frame_decode_incomplete_at_every_truncation() {
    let full = ApiFrame::new(16, vec![1, 2, 3, 4, 5]).encode();
    // Every strict prefix is incomplete, never an error, never a frame.
    for cut in 0..full.len() {
        let got = ApiFrame::decode(&full[..cut]).expect("short buffer is not an error");
        assert!(
            matches!(got, FrameDecode::Incomplete),
            "prefix of len {cut} should be Incomplete"
        );
    }
    // The whole thing decodes.
    assert!(matches!(
        ApiFrame::decode(&full).expect("ok"),
        FrameDecode::Frame { .. }
    ));
}

#[test]
fn frame_decode_rejects_noise_preamble() {
    // 0x01 is the Noise (encrypted) indicator — unsupported in the plaintext MVP.
    let err = ApiFrame::decode(&[0x01, 0x00, 0x01]).unwrap_err();
    assert_eq!(err, EsphomeError::EncryptedFrame);
}

#[test]
fn frame_decode_rejects_garbage_preamble() {
    let err = ApiFrame::decode(&[0x42, 0x00, 0x01]).unwrap_err();
    assert_eq!(err, EsphomeError::BadPreamble(0x42));
}

#[test]
fn frame_round_trips_for_many_sizes() {
    for len in [0usize, 1, 2, 127, 128, 129, 500] {
        let payload: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let encoded = ApiFrame::new(25, payload.clone()).encode();
        let FrameDecode::Frame { frame, consumed } = ApiFrame::decode(&encoded).expect("ok") else {
            panic!("round-trip frame for len {len}");
        };
        assert_eq!(frame.message_type, 25);
        assert_eq!(frame.payload, payload);
        assert_eq!(consumed, encoded.len());
    }
}
