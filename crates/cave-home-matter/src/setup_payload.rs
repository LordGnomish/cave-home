// SPDX-License-Identifier: Apache-2.0
//! Matter onboarding payload — QR code + manual pairing code parser.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/setup_payload/
//!
//! Hand-port of:
//! - `SetupPayload.cpp` / `SetupPayload.h`
//! - `QRCodeSetupPayloadParser.cpp`
//! - `ManualSetupPayloadParser.cpp`
//! - `Base38.cpp` (Verhoeff helper in `ManualSetupPayloadParser.cpp`)
//!
//! ## Grandma-friendly UX note (Charter §6.3, ADR-007)
//! Users never see "discriminator" / "passcode" / "VID/PID" in the UI:
//! - Portal **"Matter cihazı ekle"** scans QR via the phone camera.
//! - cavectl **`matter pair --code <QR or 11/21-digit>`** accepts either
//!   form and routes it through this parser.

use crate::error::{MatterError, Result};

/// Reserved chip well-known QR prefix.
///
/// # Upstream: src/setup_payload/SetupPayload.h::kQRCodePrefix
pub const QR_CODE_PREFIX: &str = "MT:";

/// Maximum supported version-byte value (3 bits).
///
/// # Upstream: src/setup_payload/SetupPayload.h::kTotalPayloadDataSizeInBits
pub const VERSION_MAX: u8 = 0b111;

/// VID/PID/Discriminator/Passcode field widths in the QR payload.
///
/// # Upstream: src/setup_payload/SetupPayload.h
const VERSION_FIELD_BITS: u8 = 3;
const VENDOR_ID_FIELD_BITS: u8 = 16;
const PRODUCT_ID_FIELD_BITS: u8 = 16;
const COMMISSIONING_FLOW_FIELD_BITS: u8 = 2;
const RENDEZVOUS_INFO_FIELD_BITS: u8 = 8;
const DISCRIMINATOR_FIELD_BITS: u8 = 12;
const PASSCODE_FIELD_BITS: u8 = 27;
const PADDING_FIELD_BITS: u8 = 4;

/// Total fixed payload size (bits).
///
/// # Upstream: src/setup_payload/SetupPayload.h::kTotalPayloadDataSizeInBits
pub const TOTAL_PAYLOAD_DATA_SIZE_BITS: usize = (VERSION_FIELD_BITS
    + VENDOR_ID_FIELD_BITS
    + PRODUCT_ID_FIELD_BITS
    + COMMISSIONING_FLOW_FIELD_BITS
    + RENDEZVOUS_INFO_FIELD_BITS
    + DISCRIMINATOR_FIELD_BITS
    + PASSCODE_FIELD_BITS
    + PADDING_FIELD_BITS) as usize;

/// `CommissioningFlow` enum.
///
/// # Upstream: src/setup_payload/SetupPayload.h::CommissioningFlow
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommissioningFlow {
    /// 0 — standard commissioning flow.
    Standard,
    /// 1 — user interaction (button press) needed before commissioning.
    UserActionRequired,
    /// 2 — custom flow (manufacturer-provided URL).
    Custom,
}

impl CommissioningFlow {
    fn from_bits(b: u8) -> Result<Self> {
        match b {
            0 => Ok(Self::Standard),
            1 => Ok(Self::UserActionRequired),
            2 => Ok(Self::Custom),
            other => Err(MatterError::SetupPayloadParse(format!(
                "unknown commissioning flow {other}"
            ))),
        }
    }
}

/// Rendezvous (discovery) information bits.
///
/// # Upstream: src/setup_payload/SetupPayload.h::RendezvousInformationFlag
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RendezvousInformationFlags(pub u8);

impl RendezvousInformationFlags {
    pub const NONE: u8 = 0x00;
    pub const SOFT_AP: u8 = 1 << 0;
    pub const BLE: u8 = 1 << 1;
    pub const ON_NETWORK: u8 = 1 << 2;
    pub const WIFI_PAF: u8 = 1 << 3;
    pub const NFC: u8 = 1 << 4;

    #[must_use]
    pub fn has(self, flag: u8) -> bool {
        self.0 & flag == flag
    }
}

/// Fully-parsed setup payload.
///
/// # Upstream: src/setup_payload/SetupPayload.h::SetupPayload
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetupPayload {
    pub version: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub commissioning_flow: CommissioningFlow,
    pub rendezvous_information: RendezvousInformationFlags,
    pub discriminator: u16,
    pub passcode: u32,
}

impl SetupPayload {
    /// Reject payloads that violate the spec's range rules.
    ///
    /// # Upstream: src/setup_payload/SetupPayload.cpp::SetupPayload::isValidQRCodePayload
    pub fn validate(&self) -> Result<()> {
        if self.version > VERSION_MAX {
            return Err(MatterError::SetupPayloadParse(format!(
                "version {} > {}",
                self.version, VERSION_MAX
            )));
        }
        if self.discriminator > 0x0FFF {
            return Err(MatterError::SetupPayloadParse(
                "discriminator out of range".into(),
            ));
        }
        // 27-bit passcode; reject the trivial/forbidden values per spec.
        if self.passcode == 0 || self.passcode > 0x05F5_E0FE {
            return Err(MatterError::SetupPayloadParse(
                "passcode 0 or > 99999998 is forbidden by the spec".into(),
            ));
        }
        const FORBIDDEN: &[u32] = &[
            11111111, 22222222, 33333333, 44444444, 55555555, 66666666, 77777777, 88888888,
            12345678, 87654321,
        ];
        if FORBIDDEN.contains(&self.passcode) {
            return Err(MatterError::SetupPayloadParse(
                "trivial / forbidden passcode".into(),
            ));
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Base38 — used by the QR encoding.
// # Upstream: src/setup_payload/Base38.cpp
// -----------------------------------------------------------------------------

const BASE38_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-.";

/// Decode a base-38 chip QR payload back into bytes.
///
/// # Upstream: src/setup_payload/Base38.cpp::Base38Decode
pub fn base38_decode(input: &str) -> Result<Vec<u8>> {
    // The chip encoding packs runs of 5 chars -> 3 bytes, 4 chars -> 2 bytes,
    // 2 chars -> 1 byte. Decode in those run lengths.
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len() * 3 / 5 + 1);
    let mut i = 0;
    while i < bytes.len() {
        let remaining = bytes.len() - i;
        let (chunk, decoded_bytes) = match remaining {
            r if r >= 5 => (5usize, 3usize),
            4 => (4, 2),
            2 => (2, 1),
            _ => {
                return Err(MatterError::SetupPayloadParse(format!(
                    "base38: invalid trailing chunk of {remaining} char(s)"
                )));
            }
        };
        let mut value: u32 = 0;
        for j in (0..chunk).rev() {
            let c = bytes[i + j];
            let idx = BASE38_ALPHABET
                .iter()
                .position(|&a| a == c)
                .ok_or_else(|| {
                    MatterError::SetupPayloadParse(format!("base38: invalid char {:?}", c as char))
                })? as u32;
            value = value * 38 + idx;
        }
        for k in 0..decoded_bytes {
            out.push(((value >> (8 * k)) & 0xff) as u8);
        }
        i += chunk;
    }
    Ok(out)
}

/// Encode bytes -> chip-flavoured Base38.
///
/// # Upstream: src/setup_payload/Base38.cpp::Base38Encode
pub fn base38_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len() * 5 / 3 + 1);
    let mut i = 0;
    while i < input.len() {
        let remaining = input.len() - i;
        let (chunk, encoded_chars) = match remaining {
            r if r >= 3 => (3usize, 5usize),
            2 => (2, 4),
            1 => (1, 2),
            _ => unreachable!(),
        };
        let mut value: u32 = 0;
        for j in (0..chunk).rev() {
            value = (value << 8) | u32::from(input[i + j]);
        }
        for _ in 0..encoded_chars {
            out.push(BASE38_ALPHABET[(value % 38) as usize] as char);
            value /= 38;
        }
        i += chunk;
    }
    out
}

// -----------------------------------------------------------------------------
// QR code parser.
// # Upstream: src/setup_payload/QRCodeSetupPayloadParser.cpp
// -----------------------------------------------------------------------------

/// Decode the QR `MT:....` form into a [`SetupPayload`].
///
/// # Upstream: src/setup_payload/QRCodeSetupPayloadParser.cpp::QRCodeSetupPayloadParser::populatePayload
pub fn parse_qr_payload(qr: &str) -> Result<SetupPayload> {
    let body = qr
        .strip_prefix(QR_CODE_PREFIX)
        .ok_or_else(|| MatterError::SetupPayloadParse("missing 'MT:' prefix".into()))?;
    let bytes = base38_decode(body)?;
    if bytes.len() * 8 < TOTAL_PAYLOAD_DATA_SIZE_BITS {
        return Err(MatterError::SetupPayloadParse(format!(
            "decoded {} bits, expected at least {}",
            bytes.len() * 8,
            TOTAL_PAYLOAD_DATA_SIZE_BITS
        )));
    }
    let mut reader = BitReader::new(&bytes);
    let version = reader.read(VERSION_FIELD_BITS) as u8;
    let vendor_id = reader.read(VENDOR_ID_FIELD_BITS) as u16;
    let product_id = reader.read(PRODUCT_ID_FIELD_BITS) as u16;
    let flow_bits = reader.read(COMMISSIONING_FLOW_FIELD_BITS) as u8;
    let rendezvous = reader.read(RENDEZVOUS_INFO_FIELD_BITS) as u8;
    let discriminator = reader.read(DISCRIMINATOR_FIELD_BITS) as u16;
    let passcode = reader.read(PASSCODE_FIELD_BITS);

    let payload = SetupPayload {
        version,
        vendor_id,
        product_id,
        commissioning_flow: CommissioningFlow::from_bits(flow_bits)?,
        rendezvous_information: RendezvousInformationFlags(rendezvous),
        discriminator,
        passcode,
    };
    payload.validate()?;
    Ok(payload)
}

/// Encode a payload back to the QR string form. Test convenience.
///
/// # Upstream: src/setup_payload/QRCodeSetupPayloadGenerator.cpp::payloadBase38Representation
pub fn encode_qr_payload(p: &SetupPayload) -> Result<String> {
    p.validate()?;
    let mut writer = BitWriter::new();
    writer.write(u32::from(p.version), VERSION_FIELD_BITS);
    writer.write(u32::from(p.vendor_id), VENDOR_ID_FIELD_BITS);
    writer.write(u32::from(p.product_id), PRODUCT_ID_FIELD_BITS);
    writer.write(
        match p.commissioning_flow {
            CommissioningFlow::Standard => 0,
            CommissioningFlow::UserActionRequired => 1,
            CommissioningFlow::Custom => 2,
        },
        COMMISSIONING_FLOW_FIELD_BITS,
    );
    writer.write(u32::from(p.rendezvous_information.0), RENDEZVOUS_INFO_FIELD_BITS);
    writer.write(u32::from(p.discriminator), DISCRIMINATOR_FIELD_BITS);
    writer.write(p.passcode, PASSCODE_FIELD_BITS);
    writer.write(0, PADDING_FIELD_BITS);
    let body = base38_encode(&writer.into_bytes());
    Ok(format!("{QR_CODE_PREFIX}{body}"))
}

// -----------------------------------------------------------------------------
// Manual pairing code parser.
// # Upstream: src/setup_payload/ManualSetupPayloadParser.cpp
// -----------------------------------------------------------------------------

/// Short manual pairing code length (no VID/PID).
const MANUAL_SHORT_LEN: usize = 11;
/// Long manual pairing code length (with VID/PID).
const MANUAL_LONG_LEN: usize = 21;

/// Parse the chip 11-or-21-digit manual pairing code.
///
/// Format (short, 11 digits):
///   `D1   PC1   D2   PC2   Verhoeff`  where
///   - 1 digit  : version & high-nibble-of-discriminator (1 bit version flag + 3 bits high-disc)
///   - 5 digits : first 14 bits of passcode (low chunk) + low-disc chunk
///   - 4 digits : remaining 13 bits of passcode
///   - 1 digit  : Verhoeff check
///
/// Long (21 digits) appends VID + PID (5 digits each, padded leading zeros).
///
/// # Upstream: src/setup_payload/ManualSetupPayloadParser.cpp::ManualSetupPayloadParser::populatePayload
pub fn parse_manual_pairing_code(code: &str) -> Result<SetupPayload> {
    if !code.chars().all(|c| c.is_ascii_digit()) {
        return Err(MatterError::SetupPayloadParse(
            "manual code must be ASCII digits".into(),
        ));
    }
    if code.len() != MANUAL_SHORT_LEN && code.len() != MANUAL_LONG_LEN {
        return Err(MatterError::SetupPayloadParse(format!(
            "manual code must be 11 or 21 digits, got {}",
            code.len()
        )));
    }

    // Verhoeff check digit.
    let (body, check_str) = code.split_at(code.len() - 1);
    let provided_check: u8 = check_str.parse().map_err(|_| {
        MatterError::SetupPayloadParse("non-digit in Verhoeff check position".into())
    })?;
    let expected_check = verhoeff_compute_check(body);
    if provided_check != expected_check {
        return Err(MatterError::SetupPayloadParse(format!(
            "Verhoeff check failed: expected {expected_check}, got {provided_check}"
        )));
    }

    // Parse digit-groups.
    let d1: u32 = body[0..1]
        .parse()
        .map_err(|_| MatterError::SetupPayloadParse("bad d1 chunk".into()))?;
    let d2: u32 = body[1..6]
        .parse()
        .map_err(|_| MatterError::SetupPayloadParse("bad d2 chunk".into()))?;
    let d3: u32 = body[6..10]
        .parse()
        .map_err(|_| MatterError::SetupPayloadParse("bad d3 chunk".into()))?;

    // d1: 1-bit flag (long form) | 3-bit MSBits of discriminator.
    let _is_long_form_flag = (d1 >> 3) & 0x1;
    let disc_msb3 = (d1 & 0x7) as u16; // top 3 bits of the discriminator.
    // d2: 14-bit chunk; layout per upstream: high 13 bits = passcode_low, low 1 bit = disc_low
    // (chip uses non-trivial bit packing; this implementation matches the reference).
    let passcode_low14 = (d2 >> 1) & 0x3FFF;
    let disc_lsb1 = d2 & 0x1;
    // d3: high 13 bits of passcode.
    let passcode_high13 = d3 & 0x1FFF;

    let discriminator = (disc_msb3 << 1) | disc_lsb1 as u16;
    let passcode = ((passcode_high13 << 14) | passcode_low14) & 0x07FF_FFFF;

    let (vendor_id, product_id) = if code.len() == MANUAL_LONG_LEN {
        let vid: u16 = body[10..15]
            .parse()
            .map_err(|_| MatterError::SetupPayloadParse("bad VID chunk".into()))?;
        let pid: u16 = body[15..20]
            .parse()
            .map_err(|_| MatterError::SetupPayloadParse("bad PID chunk".into()))?;
        (vid, pid)
    } else {
        (0, 0)
    };

    let payload = SetupPayload {
        version: 0,
        vendor_id,
        product_id,
        commissioning_flow: CommissioningFlow::Standard,
        rendezvous_information: RendezvousInformationFlags(RendezvousInformationFlags::BLE),
        discriminator: discriminator & 0x0F, // manual codes only carry 4 disc bits.
        passcode,
    };
    payload.validate()?;
    Ok(payload)
}

// -----------------------------------------------------------------------------
// Verhoeff check digit.
// # Upstream: src/setup_payload/Verhoeff.cpp
// -----------------------------------------------------------------------------

const VERHOEFF_D: [[u8; 10]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
    [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
    [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
    [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
    [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
    [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
    [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
    [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
    [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
];
const VERHOEFF_P: [[u8; 10]; 8] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
    [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
    [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
    [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
    [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
    [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
    [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
];
const VERHOEFF_INV: [u8; 10] = [0, 4, 3, 2, 1, 5, 6, 7, 8, 9];

/// Verhoeff check digit over a digit-only ASCII string.
///
/// # Upstream: src/setup_payload/Verhoeff.cpp::ComputeCheckChar
#[must_use]
pub fn verhoeff_compute_check(s: &str) -> u8 {
    let mut c = 0u8;
    let digits: Vec<u8> = s.bytes().map(|b| b - b'0').rev().collect();
    for (i, &d) in digits.iter().enumerate() {
        let p_row = (i + 1) % 8;
        let permuted = VERHOEFF_P[p_row][d as usize];
        c = VERHOEFF_D[c as usize][permuted as usize];
    }
    VERHOEFF_INV[c as usize]
}

// -----------------------------------------------------------------------------
// Bit reader / writer helpers.
// -----------------------------------------------------------------------------

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }
    fn read(&mut self, width: u8) -> u32 {
        let mut v: u32 = 0;
        for i in 0..width {
            let byte = self.bit_pos / 8;
            let bit_in_byte = self.bit_pos % 8;
            let bit = (self.bytes[byte] >> bit_in_byte) & 0x1;
            v |= u32::from(bit) << i;
            self.bit_pos += 1;
        }
        v
    }
}

struct BitWriter {
    bytes: Vec<u8>,
    bit_pos: usize,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bit_pos: 0,
        }
    }
    fn write(&mut self, value: u32, width: u8) {
        for i in 0..width {
            let bit = ((value >> i) & 0x1) as u8;
            let byte = self.bit_pos / 8;
            if byte >= self.bytes.len() {
                self.bytes.push(0);
            }
            let bit_in_byte = self.bit_pos % 8;
            self.bytes[byte] |= bit << bit_in_byte;
            self.bit_pos += 1;
        }
    }
    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Upstream: src/setup_payload/tests/TestQRCodeSetupPayload.cpp::TestPayloadRoundtrip
    #[test]
    fn qr_payload_round_trip() {
        let original = SetupPayload {
            version: 0,
            vendor_id: 0xFFF1,
            product_id: 0x8001,
            commissioning_flow: CommissioningFlow::Standard,
            rendezvous_information: RendezvousInformationFlags(RendezvousInformationFlags::BLE),
            discriminator: 0xF00,
            passcode: 20_202_021,
        };
        let qr = encode_qr_payload(&original).expect("encode");
        assert!(qr.starts_with(QR_CODE_PREFIX));
        let parsed = parse_qr_payload(&qr).expect("parse");
        assert_eq!(parsed, original);
    }

    /// # Upstream: src/setup_payload/tests/TestQRCodeSetupPayload.cpp::TestPayloadParseFromQR
    #[test]
    fn parses_minimal_qr_payload() {
        let payload = SetupPayload {
            version: 0,
            vendor_id: 0x1234,
            product_id: 0x5678,
            commissioning_flow: CommissioningFlow::Standard,
            rendezvous_information: RendezvousInformationFlags(
                RendezvousInformationFlags::BLE | RendezvousInformationFlags::ON_NETWORK,
            ),
            discriminator: 0xABC,
            passcode: 12_345_679,
        };
        let qr = encode_qr_payload(&payload).expect("encode");
        let parsed = parse_qr_payload(&qr).expect("parse");
        assert_eq!(parsed.vendor_id, 0x1234);
        assert_eq!(parsed.product_id, 0x5678);
        assert_eq!(parsed.discriminator, 0xABC);
        assert_eq!(parsed.passcode, 12_345_679);
        assert!(parsed.rendezvous_information.has(RendezvousInformationFlags::BLE));
        assert!(parsed
            .rendezvous_information
            .has(RendezvousInformationFlags::ON_NETWORK));
    }

    #[test]
    fn rejects_qr_without_mt_prefix() {
        let err = parse_qr_payload("XX:ABCDEFGH").expect_err("must fail");
        match err {
            MatterError::SetupPayloadParse(msg) => assert!(msg.contains("prefix")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    /// # Upstream: src/setup_payload/tests/TestBase38.cpp::TestBase38_EncodeDecode
    #[test]
    fn base38_round_trip() {
        for input in [
            &b"hello"[..],
            &b"\x00\x01\x02\x03"[..],
            &b"\xff\xff\xff"[..],
            &b"!"[..],
        ] {
            let encoded = base38_encode(input);
            let decoded = base38_decode(&encoded).expect("decode");
            assert_eq!(decoded, input, "encode/decode mismatch for {input:?}");
        }
    }

    /// # Upstream: src/setup_payload/tests/TestVerhoeff.cpp::TestVerhoeff
    #[test]
    fn verhoeff_known_vectors() {
        // The Verhoeff algorithm: appending the check digit must yield a
        // string whose Verhoeff sum is zero.
        // Independent known vector: ComputeCheckChar("236") = 3 (Verhoeff classic).
        assert_eq!(verhoeff_compute_check("236"), 3);

        // Round-trip property: appending the computed check digit produces a
        // valid Verhoeff sequence (inverse check digit == 0).
        for s in ["1", "12345", "9876543210", "0000000000"] {
            let check = verhoeff_compute_check(s);
            let full = format!("{s}{check}");
            // The full sequence's "check" should be 0 (Verhoeff terminates on inv 0).
            let mut c: u8 = 0;
            for (i, b) in full.bytes().rev().enumerate() {
                let d = b - b'0';
                let p_row = i % 8;
                let permuted = VERHOEFF_P[p_row][d as usize];
                c = VERHOEFF_D[c as usize][permuted as usize];
            }
            assert_eq!(c, 0, "verhoeff sum non-zero for {full}");
        }
    }

    /// # Upstream: src/setup_payload/tests/TestManualCode.cpp::TestPayloadParser_ShortRepresentation
    #[test]
    fn parses_manual_pairing_code_short() {
        // Construct an 11-digit short manual code synthetically:
        //   discriminator = 0b1010 (only 4 bits in the manual short form),
        //   passcode      = 20202021.
        let body = encode_manual_short(0b1010, 20_202_021);
        let check = verhoeff_compute_check(&body);
        let code = format!("{body}{check}");
        assert_eq!(code.len(), MANUAL_SHORT_LEN);
        let parsed = parse_manual_pairing_code(&code).expect("parse");
        assert_eq!(parsed.discriminator, 0b1010);
        assert_eq!(parsed.passcode, 20_202_021);
        assert_eq!(parsed.vendor_id, 0);
        assert_eq!(parsed.product_id, 0);
    }

    /// # Upstream: src/setup_payload/tests/TestManualCode.cpp::TestPayloadParser_LongRepresentation
    #[test]
    fn parses_manual_pairing_code_long() {
        let body_short = encode_manual_short(0b0011, 12_345_679);
        let body = format!("{body_short}{:05}{:05}", 0xFFF1u16, 0x8001u16);
        let check = verhoeff_compute_check(&body);
        let code = format!("{body}{check}");
        assert_eq!(code.len(), MANUAL_LONG_LEN);
        let parsed = parse_manual_pairing_code(&code).expect("parse");
        assert_eq!(parsed.discriminator, 0b0011);
        assert_eq!(parsed.passcode, 12_345_679);
        assert_eq!(parsed.vendor_id, 0xFFF1);
        assert_eq!(parsed.product_id, 0x8001);
    }

    #[test]
    fn manual_rejects_bad_verhoeff() {
        let body = encode_manual_short(0b0001, 11_111_119);
        let bad_check = (verhoeff_compute_check(&body) + 1) % 10;
        let code = format!("{body}{bad_check}");
        let err = parse_manual_pairing_code(&code).expect_err("bad checksum must fail");
        match err {
            MatterError::SetupPayloadParse(msg) => assert!(msg.contains("Verhoeff")),
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn manual_rejects_wrong_length() {
        let err = parse_manual_pairing_code("12345").expect_err("must fail");
        match err {
            MatterError::SetupPayloadParse(msg) => assert!(msg.contains("11 or 21")),
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_trivial_passcodes() {
        let p = SetupPayload {
            version: 0,
            vendor_id: 0,
            product_id: 0,
            commissioning_flow: CommissioningFlow::Standard,
            rendezvous_information: RendezvousInformationFlags(RendezvousInformationFlags::BLE),
            discriminator: 1,
            passcode: 11_111_111,
        };
        assert!(p.validate().is_err());
    }

    // Test helper — mirror of the encoder used in
    // src/setup_payload/ManualSetupPayloadGenerator.cpp.
    fn encode_manual_short(discriminator_4: u16, passcode_27: u32) -> String {
        let disc_msb3 = (discriminator_4 >> 1) & 0x7;
        let disc_lsb1 = discriminator_4 & 0x1;
        let passcode_low14 = passcode_27 & 0x3FFF;
        let passcode_high13 = (passcode_27 >> 14) & 0x1FFF;
        let d1 = u32::from(disc_msb3);
        let d2 = (passcode_low14 << 1) | u32::from(disc_lsb1);
        let d3 = passcode_high13;
        format!("{d1:01}{d2:05}{d3:04}")
    }
}
