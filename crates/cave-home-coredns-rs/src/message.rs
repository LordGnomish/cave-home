// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The full DNS message and its question section.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::name::Name;
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use crate::wire::Rcode;
    use std::net::Ipv4Addr;

    fn q(n: &str, t: RecordType) -> Question {
        Question::new(Name::parse(n).unwrap(), t, Class::In)
    }

    fn a(n: &str, ip: [u8; 4]) -> ResourceRecord {
        ResourceRecord::new(
            Name::parse(n).unwrap(),
            Class::In,
            300,
            Rdata::A(Ipv4Addr::from(ip)),
        )
    }

    #[test]
    fn question_round_trips() {
        let question = q("www.example.com", RecordType::Aaaa);
        let mut w = crate::wire::Writer::new();
        question.encode(&mut w);
        let bytes = w.into_bytes();
        let mut r = crate::wire::Reader::new(&bytes);
        assert_eq!(Question::decode(&mut r).unwrap(), question);
    }

    #[test]
    fn query_constructor_sets_rd_and_a_single_question() {
        let m = Message::query(Name::parse("example.com").unwrap(), RecordType::A, 0x42);
        assert_eq!(m.header.id, 0x42);
        assert!(m.header.rd);
        assert!(!m.header.qr);
        assert_eq!(m.questions.len(), 1);
        assert_eq!(m.questions[0].qtype, RecordType::A);
    }

    #[test]
    fn encode_derives_section_counts_from_the_vectors() {
        let mut m = Message::query(Name::parse("example.com").unwrap(), RecordType::A, 1);
        m.answers.push(a("example.com", [1, 1, 1, 1]));
        m.answers.push(a("example.com", [2, 2, 2, 2]));
        m.authority
            .push(ResourceRecord::new(
                Name::parse("example.com").unwrap(),
                Class::In,
                3600,
                Rdata::Ns(Name::parse("ns.example.com").unwrap()),
            ));
        let bytes = m.encode();
        let decoded = Message::decode(&bytes).unwrap();
        assert_eq!(decoded.header.qdcount, 1);
        assert_eq!(decoded.header.ancount, 2);
        assert_eq!(decoded.header.nscount, 1);
        assert_eq!(decoded.header.arcount, 0);
    }

    #[test]
    fn full_message_round_trips_with_all_sections() {
        let mut m = Message::query(Name::parse("example.com").unwrap(), RecordType::A, 0xABCD);
        m.answers.push(a("example.com", [192, 0, 2, 1]));
        m.answers.push(a("example.com", [192, 0, 2, 2]));
        m.authority.push(ResourceRecord::new(
            Name::parse("example.com").unwrap(),
            Class::In,
            3600,
            Rdata::Ns(Name::parse("ns1.example.com").unwrap()),
        ));
        m.additional.push(a("ns1.example.com", [192, 0, 2, 53]));
        let bytes = m.encode();
        let back = Message::decode(&bytes).unwrap();
        assert_eq!(back.questions, m.questions);
        assert_eq!(back.answers, m.answers);
        assert_eq!(back.authority, m.authority);
        assert_eq!(back.additional, m.additional);
    }

    #[test]
    fn reply_echoes_id_and_question_and_sets_qr() {
        let query = Message::query(Name::parse("example.com").unwrap(), RecordType::A, 7);
        let reply = query.reply();
        assert_eq!(reply.header.id, 7);
        assert!(reply.header.qr);
        assert_eq!(reply.questions, query.questions);
        assert!(reply.answers.is_empty());
        let nx = query.reply().with_rcode(Rcode::NxDomain);
        assert_eq!(nx.header.rcode, Rcode::NxDomain);
    }

    #[test]
    fn shared_names_compress_across_sections() {
        let mut m = Message::query(Name::parse("example.com").unwrap(), RecordType::A, 1);
        for _ in 0..8 {
            m.answers.push(a("example.com", [10, 0, 0, 1]));
        }
        let bytes = m.encode();
        // 8 answers all named example.com: every owner name after the first
        // compresses to a 2-byte pointer, so the message is far smaller than the
        // ~16 bytes/owner an uncompressed encoding would need.
        assert!(bytes.len() < 200, "expected compression, got {} bytes", bytes.len());
        assert_eq!(Message::decode(&bytes).unwrap().answers, m.answers);
    }

    #[test]
    fn decode_rejects_a_truncated_message() {
        let bytes = [0u8; 5]; // shorter than even the 12-byte header
        assert!(Message::decode(&bytes).is_err());
    }
}
