// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! DNS message header and name on-the-wire codec.

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
