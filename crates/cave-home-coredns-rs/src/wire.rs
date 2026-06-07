// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! DNS message header and name on-the-wire codec.
//!
//! [`Reader`] and [`Writer`] are the cursor primitives the whole message codec
//! is built on. [`Reader::read_name`] resolves RFC 1035 §4.1.4 compression
//! pointers against the full message buffer (rejecting non-backward pointers so
//! it cannot loop); [`Writer::write_name`] performs the inverse, sharing a
//! suffix that has already been emitted.

use crate::error::{Result, WireError};
use crate::name::{Name, MAX_NAME_WIRE};
use std::collections::HashMap;

/// The two high bits of a length octet that mark a compression pointer.
const PTR_MASK: u8 = 0b1100_0000;
/// The largest offset a compression pointer can encode (14 bits).
const MAX_PTR_OFFSET: usize = 0x3FFF;

/// A forward cursor over a DNS message buffer.
///
/// `read_name` needs the *whole* message because compression pointers reference
/// earlier offsets, so the reader holds the full slice and tracks a position.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Create a reader over a complete message buffer.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// The current read offset.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Octets remaining after the cursor.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Read one octet.
    ///
    /// # Errors
    /// [`WireError::UnexpectedEof`] past the end of the buffer.
    pub fn read_u8(&mut self) -> Result<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or(WireError::UnexpectedEof { needed: "u8" })?;
        self.pos += 1;
        Ok(b)
    }

    /// Read a big-endian `u16`.
    ///
    /// # Errors
    /// [`WireError::UnexpectedEof`] if fewer than two octets remain.
    pub fn read_u16(&mut self) -> Result<u16> {
        Ok((u16::from(self.read_u8()?) << 8) | u16::from(self.read_u8()?))
    }

    /// Read a big-endian `u32`.
    ///
    /// # Errors
    /// [`WireError::UnexpectedEof`] if fewer than four octets remain.
    pub fn read_u32(&mut self) -> Result<u32> {
        Ok((u32::from(self.read_u16()?) << 16) | u32::from(self.read_u16()?))
    }

    /// Borrow `n` octets and advance.
    ///
    /// # Errors
    /// [`WireError::UnexpectedEof`] if fewer than `n` octets remain.
    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or(WireError::UnexpectedEof { needed: "bytes" })?;
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    /// Advance past `n` octets without reading them.
    ///
    /// # Errors
    /// [`WireError::UnexpectedEof`] if fewer than `n` octets remain.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.read_bytes(n).map(|_| ())
    }

    /// Decode a domain name, following RFC 1035 §4.1.4 compression pointers.
    ///
    /// The cursor is left just past the *first* pointer (or the root octet of an
    /// uncompressed name), matching the wire's notion of "consumed".
    ///
    /// # Errors
    /// [`WireError::BadCompressionPointer`] for a non-backward pointer,
    /// [`WireError::ReservedLabelType`] for an undefined label-type,
    /// [`WireError::NameTooLong`] past 255 octets, or
    /// [`WireError::UnexpectedEof`] on a truncated label/pointer.
    pub fn read_name(&mut self) -> Result<Name> {
        let mut labels: Vec<Vec<u8>> = Vec::new();
        let mut wire_len = 1usize; // the root terminator always counts
        let mut pos = self.pos;
        let mut jumped = false;

        loop {
            let len_byte = *self
                .buf
                .get(pos)
                .ok_or(WireError::UnexpectedEof { needed: "label length" })?;
            match len_byte & PTR_MASK {
                0x00 => {
                    let llen = len_byte as usize;
                    if llen == 0 {
                        pos += 1;
                        if !jumped {
                            self.pos = pos;
                        }
                        break;
                    }
                    let start = pos + 1;
                    let label = self
                        .buf
                        .get(start..start + llen)
                        .ok_or(WireError::UnexpectedEof { needed: "label" })?
                        .to_vec();
                    wire_len += 1 + llen;
                    if wire_len > MAX_NAME_WIRE {
                        return Err(WireError::NameTooLong { len: wire_len });
                    }
                    labels.push(label);
                    pos = start + llen;
                    if !jumped {
                        self.pos = pos;
                    }
                }
                PTR_MASK => {
                    let lo = *self
                        .buf
                        .get(pos + 1)
                        .ok_or(WireError::UnexpectedEof { needed: "pointer" })?;
                    let target = ((usize::from(len_byte & !PTR_MASK)) << 8) | usize::from(lo);
                    if !jumped {
                        self.pos = pos + 2;
                        jumped = true;
                    }
                    // Pointers must reference a strictly earlier offset; this both
                    // matches the protocol and makes loops impossible.
                    if target >= pos {
                        return Err(WireError::BadCompressionPointer { offset: target });
                    }
                    pos = target;
                }
                _ => return Err(WireError::ReservedLabelType),
            }
        }
        Name::from_labels(labels)
    }
}

/// A growable DNS message buffer with name-compression bookkeeping.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
    /// Canonical suffix → offset of the first place it was written.
    ptrs: HashMap<String, u16>,
}

impl Writer {
    /// A new, empty writer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of octets written so far (also the next write offset).
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether nothing has been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Append one octet.
    pub fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Append a big-endian `u16`.
    pub fn write_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    /// Append a big-endian `u32`.
    pub fn write_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    /// Append raw octets.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Write a domain name, compressing any suffix already present (RFC 1035
    /// §4.1.4).
    pub fn write_name(&mut self, name: &Name) {
        let labels = name.labels();
        for i in 0..labels.len() {
            let key = suffix_key(&labels[i..]);
            if let Some(&off) = self.ptrs.get(&key) {
                self.write_u16(u16::from(PTR_MASK) << 8 | off);
                return;
            }
            let here = self.buf.len();
            if here <= MAX_PTR_OFFSET {
                self.ptrs.insert(key, here as u16);
            }
            self.buf.push(labels[i].len() as u8);
            self.buf.extend_from_slice(&labels[i]);
        }
        self.buf.push(0);
    }

    /// Consume the writer, yielding the message bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

/// The canonical (lower-cased, dot-joined) form of a label suffix, used as the
/// compression key so the same name compresses regardless of letter case.
fn suffix_key(labels: &[Vec<u8>]) -> String {
    let mut s = String::new();
    for label in labels {
        for &b in label {
            s.push(b.to_ascii_lowercase() as char);
        }
        s.push('.');
    }
    s
}

/// The DNS message OPCODE (RFC 1035 §4.1.1, RFC 2136 Update, RFC 1996 Notify).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    /// Standard query.
    Query = 0,
    /// Inverse query (obsolete).
    IQuery = 1,
    /// Server status request.
    Status = 2,
    /// Zone-change notification (RFC 1996).
    Notify = 4,
    /// Dynamic update (RFC 2136).
    Update = 5,
}

impl Opcode {
    /// Decode an opcode; unknown values fall back to [`Opcode::Query`].
    #[must_use]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::IQuery,
            2 => Self::Status,
            4 => Self::Notify,
            5 => Self::Update,
            _ => Self::Query,
        }
    }
}

/// The DNS RCODE (RFC 1035 §4.1.1, RFC 2136).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Rcode {
    /// No error.
    NoError = 0,
    /// Format error — the server could not interpret the query.
    FormErr = 1,
    /// Server failure.
    ServFail = 2,
    /// Non-existent domain.
    NxDomain = 3,
    /// Not implemented.
    NotImp = 4,
    /// Query refused by policy.
    Refused = 5,
    /// A name that should not exist does (RFC 2136).
    YxDomain = 6,
    /// An `RRset` that should not exist does (RFC 2136).
    YxrrSet = 7,
    /// An `RRset` that should exist does not (RFC 2136).
    NxrrSet = 8,
    /// Server not authoritative for the zone (RFC 2136).
    NotAuth = 9,
    /// Name not contained in the zone (RFC 2136).
    NotZone = 10,
}

impl Rcode {
    /// Decode a 4-bit RCODE; unknown values fall back to [`Rcode::ServFail`].
    #[must_use]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::NoError,
            1 => Self::FormErr,
            3 => Self::NxDomain,
            4 => Self::NotImp,
            5 => Self::Refused,
            6 => Self::YxDomain,
            7 => Self::YxrrSet,
            8 => Self::NxrrSet,
            9 => Self::NotAuth,
            10 => Self::NotZone,
            _ => Self::ServFail,
        }
    }
}

/// The fixed 12-octet DNS message header (RFC 1035 §4.1.1).
// A DNS header is, by the protocol's definition, a bag of single-bit flags;
// the "too many bools" lint does not apply to a fixed wire layout.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Query identifier echoed in the response.
    pub id: u16,
    /// Query (`false`) / response (`true`).
    pub qr: bool,
    /// Operation code.
    pub opcode: Opcode,
    /// Authoritative answer.
    pub aa: bool,
    /// Truncated.
    pub tc: bool,
    /// Recursion desired.
    pub rd: bool,
    /// Recursion available.
    pub ra: bool,
    /// Reserved zero bit (RFC 1035) — kept for exact round-trips.
    pub z: bool,
    /// Authentic data (DNSSEC, RFC 4035).
    pub ad: bool,
    /// Checking disabled (DNSSEC, RFC 4035).
    pub cd: bool,
    /// Response code.
    pub rcode: Rcode,
    /// Question count.
    pub qdcount: u16,
    /// Answer count.
    pub ancount: u16,
    /// Authority count.
    pub nscount: u16,
    /// Additional count.
    pub arcount: u16,
}

impl Header {
    /// Serialise to the fixed 12 octets.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut b = [0u8; 12];
        b[0..2].copy_from_slice(&self.id.to_be_bytes());
        let mut flags1 = 0u8;
        flags1 |= u8::from(self.qr) << 7;
        flags1 |= (self.opcode as u8) << 3;
        flags1 |= u8::from(self.aa) << 2;
        flags1 |= u8::from(self.tc) << 1;
        flags1 |= u8::from(self.rd);
        let mut flags2 = 0u8;
        flags2 |= u8::from(self.ra) << 7;
        flags2 |= u8::from(self.z) << 6;
        flags2 |= u8::from(self.ad) << 5;
        flags2 |= u8::from(self.cd) << 4;
        flags2 |= self.rcode as u8;
        b[2] = flags1;
        b[3] = flags2;
        b[4..6].copy_from_slice(&self.qdcount.to_be_bytes());
        b[6..8].copy_from_slice(&self.ancount.to_be_bytes());
        b[8..10].copy_from_slice(&self.nscount.to_be_bytes());
        b[10..12].copy_from_slice(&self.arcount.to_be_bytes());
        b
    }

    /// Append the header to a [`Writer`].
    pub fn encode(&self, w: &mut Writer) {
        w.write_bytes(&self.to_bytes());
    }

    /// Decode the header from a [`Reader`].
    ///
    /// # Errors
    /// [`WireError::UnexpectedEof`] if fewer than 12 octets remain.
    // The four count fields share the canonical DNS `*count` naming; the
    // similar-names lint is noise here.
    #[allow(clippy::similar_names)]
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let id = r.read_u16()?;
        let flags1 = r.read_u8()?;
        let flags2 = r.read_u8()?;
        let qdcount = r.read_u16()?;
        let ancount = r.read_u16()?;
        let nscount = r.read_u16()?;
        let arcount = r.read_u16()?;
        Ok(Self {
            id,
            qr: flags1 & 0x80 != 0,
            opcode: Opcode::from_u8((flags1 >> 3) & 0x0F),
            aa: flags1 & 0x04 != 0,
            tc: flags1 & 0x02 != 0,
            rd: flags1 & 0x01 != 0,
            ra: flags2 & 0x80 != 0,
            z: flags2 & 0x40 != 0,
            ad: flags2 & 0x20 != 0,
            cd: flags2 & 0x10 != 0,
            rcode: Rcode::from_u8(flags2 & 0x0F),
            qdcount,
            ancount,
            nscount,
            arcount,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::name::Name;

    #[test]
    fn header_round_trips_through_bytes() {
        let h = Header {
            id: 0xBEEF,
            qr: true,
            opcode: Opcode::Query,
            aa: true,
            tc: false,
            rd: true,
            ra: true,
            z: false,
            ad: false,
            cd: false,
            rcode: Rcode::NxDomain,
            qdcount: 1,
            ancount: 2,
            nscount: 1,
            arcount: 0,
        };
        let bytes = h.to_bytes();
        assert_eq!(bytes.len(), 12);
        let mut r = Reader::new(&bytes);
        let decoded = Header::decode(&mut r).unwrap();
        assert_eq!(decoded, h);
        assert_eq!(r.position(), 12);
    }

    #[test]
    fn header_flag_bits_have_the_rfc_layout() {
        let h = Header {
            id: 0x1234,
            qr: true,
            opcode: Opcode::Update,
            aa: false,
            tc: false,
            rd: true,
            ra: false,
            z: false,
            ad: false,
            cd: false,
            rcode: Rcode::NoError,
            qdcount: 0,
            ancount: 0,
            nscount: 0,
            arcount: 0,
        };
        let b = h.to_bytes();
        // byte 2: QR(1) Opcode(0101=5) AA(0) TC(0) RD(1) => 1_0101_0_0_1 = 0xA9
        assert_eq!(b[2], 0b1_0101_0_0_1);
        // byte 3: RA(0) Z(0) AD(0) CD(0) RCODE(0000) => 0x00
        assert_eq!(b[3], 0x00);
        assert_eq!(&b[0..2], &[0x12, 0x34]);
    }

    #[test]
    fn rcode_round_trips_its_wire_value() {
        for rc in [
            Rcode::NoError,
            Rcode::FormErr,
            Rcode::ServFail,
            Rcode::NxDomain,
            Rcode::NotImp,
            Rcode::Refused,
        ] {
            assert_eq!(Rcode::from_u8(rc as u8), rc);
        }
    }

    #[test]
    fn name_round_trips_uncompressed() {
        let mut w = Writer::new();
        let n = Name::parse("www.example.com").unwrap();
        w.write_name(&n);
        let bytes = w.into_bytes();
        // 3www7example3com0 = 4+8+4+1 = 17
        assert_eq!(bytes.len(), 17);
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_name().unwrap(), n);
    }

    #[test]
    fn compression_reuses_a_shared_suffix() {
        let mut w = Writer::new();
        // Put a 12-byte header's worth of offset before the names so pointers
        // land at realistic non-zero offsets.
        w.write_bytes(&[0u8; 12]);
        let a = Name::parse("a.example.com").unwrap();
        let b = Name::parse("b.example.com").unwrap();
        let off_a = w.len();
        w.write_name(&a);
        let after_a = w.len();
        w.write_name(&b);
        let bytes = w.into_bytes();
        // `a` is written in full (1+1 + 7+1 + 3+1 + 1 = 15).
        assert_eq!(after_a - off_a, 15);
        // `b` reuses ".example.com": 1 label `b` (2 bytes) + 2-byte pointer = 4.
        assert_eq!(bytes.len() - after_a, 4);
        // Both decode correctly.
        let mut r = Reader::new(&bytes);
        r.skip(12).unwrap();
        assert_eq!(r.read_name().unwrap(), a);
        assert_eq!(r.read_name().unwrap(), b);
    }

    #[test]
    fn decode_follows_a_pointer() {
        // [0..] 3 f o o 0   then at offset 5 a pointer to offset 0.
        let bytes = [3, b'f', b'o', b'o', 0, 0xC0, 0x00];
        let mut r = Reader::new(&bytes);
        let first = r.read_name().unwrap();
        assert_eq!(first, Name::parse("foo").unwrap());
        assert_eq!(r.position(), 5);
        let second = r.read_name().unwrap();
        assert_eq!(second, Name::parse("foo").unwrap());
        // Consumed only the 2 pointer bytes for the second name.
        assert_eq!(r.position(), 7);
    }

    #[test]
    fn decode_rejects_a_non_backward_pointer() {
        // A pointer at offset 0 that points to offset 0 (itself / forward).
        let bytes = [0xC0, 0x00];
        let mut r = Reader::new(&bytes);
        assert!(matches!(
            r.read_name(),
            Err(crate::WireError::BadCompressionPointer { .. })
        ));
    }

    #[test]
    fn decode_rejects_reserved_label_bits() {
        let bytes = [0b0100_0001, b'x'];
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_name(), Err(crate::WireError::ReservedLabelType));
    }

    #[test]
    fn reader_reports_eof() {
        let bytes = [0x12];
        let mut r = Reader::new(&bytes);
        assert!(r.read_u16().is_err());
    }

    #[test]
    fn decode_enforces_the_255_octet_name_limit() {
        // Six 63-octet labels chained by pointers would exceed 255.
        let mut buf = Vec::new();
        for _ in 0..5 {
            buf.push(63u8);
            buf.extend(std::iter::repeat_n(b'a', 63));
        }
        buf.push(0);
        let mut r = Reader::new(&buf);
        assert!(matches!(
            r.read_name(),
            Err(crate::WireError::NameTooLong { .. })
        ));
    }
}
