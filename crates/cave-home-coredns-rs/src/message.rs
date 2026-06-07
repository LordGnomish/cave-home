// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The full DNS message and its question section.
//!
//! A [`Message`] is the header plus the four sections (question / answer /
//! authority / additional). Encoding derives the section *counts* from the
//! vector lengths, so callers never keep them in sync by hand; a single
//! [`Writer`] spans the whole message, so a name in the answer section
//! compresses against the same name in the question.

use crate::error::Result;
use crate::name::Name;
use crate::rr::{Class, RecordType, ResourceRecord};
use crate::wire::{Header, Opcode, Rcode, Reader, Writer};

/// A single entry in the question section (RFC 1035 §4.1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// The queried name.
    pub name: Name,
    /// The queried type.
    pub qtype: RecordType,
    /// The queried class.
    pub qclass: Class,
}

impl Question {
    /// Build a question.
    #[must_use]
    pub const fn new(name: Name, qtype: RecordType, qclass: Class) -> Self {
        Self { name, qtype, qclass }
    }

    /// Encode the question.
    pub fn encode(&self, w: &mut Writer) {
        w.write_name(&self.name);
        w.write_u16(self.qtype.to_u16());
        w.write_u16(self.qclass.to_u16());
    }

    /// Decode a question.
    ///
    /// # Errors
    /// Any [`crate::WireError`] from the name codec or a short buffer.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(Self {
            name: r.read_name()?,
            qtype: RecordType::from_u16(r.read_u16()?),
            qclass: Class::from_u16(r.read_u16()?),
        })
    }
}

/// A complete DNS message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The 12-octet header. Section counts are recomputed on [`Message::encode`].
    pub header: Header,
    /// The question section.
    pub questions: Vec<Question>,
    /// The answer section.
    pub answers: Vec<ResourceRecord>,
    /// The authority (NS / SOA) section.
    pub authority: Vec<ResourceRecord>,
    /// The additional (glue / OPT) section.
    pub additional: Vec<ResourceRecord>,
}

impl Message {
    /// An empty message with a zeroed header carrying the given id.
    #[must_use]
    pub const fn empty(id: u16) -> Self {
        Self {
            header: Header {
                id,
                qr: false,
                opcode: Opcode::Query,
                aa: false,
                tc: false,
                rd: false,
                ra: false,
                z: false,
                ad: false,
                cd: false,
                rcode: Rcode::NoError,
                qdcount: 0,
                ancount: 0,
                nscount: 0,
                arcount: 0,
            },
            questions: Vec::new(),
            answers: Vec::new(),
            authority: Vec::new(),
            additional: Vec::new(),
        }
    }

    /// A standard recursive query for `name`/`qtype` in class `IN`.
    #[must_use]
    pub fn query(name: Name, qtype: RecordType, id: u16) -> Self {
        let mut m = Self::empty(id);
        m.header.rd = true;
        m.questions.push(Question::new(name, qtype, Class::In));
        m
    }

    /// A response skeleton for this query: same id and question, `qr` set,
    /// recursion-available set, `NOERROR`, and empty answer sections.
    #[must_use]
    pub fn reply(&self) -> Self {
        let mut m = Self::empty(self.header.id);
        m.header.qr = true;
        m.header.opcode = self.header.opcode;
        m.header.rd = self.header.rd;
        m.header.ra = true;
        m.questions.clone_from(&self.questions);
        m
    }

    /// Set the response code (builder style).
    #[must_use]
    pub const fn with_rcode(mut self, rcode: Rcode) -> Self {
        self.header.rcode = rcode;
        self
    }

    /// Set the authoritative-answer bit (builder style).
    #[must_use]
    pub const fn with_aa(mut self, aa: bool) -> Self {
        self.header.aa = aa;
        self
    }

    /// Serialise the whole message, recomputing the section counts.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut header = self.header;
        header.qdcount = self.questions.len() as u16;
        header.ancount = self.answers.len() as u16;
        header.nscount = self.authority.len() as u16;
        header.arcount = self.additional.len() as u16;

        let mut w = Writer::new();
        header.encode(&mut w);
        for q in &self.questions {
            q.encode(&mut w);
        }
        for rr in self.answers.iter().chain(&self.authority).chain(&self.additional) {
            rr.encode(&mut w);
        }
        w.into_bytes()
    }

    /// Parse a message from a complete buffer.
    ///
    /// # Errors
    /// Any [`crate::WireError`] from the header / section codecs, including a
    /// buffer too short for the header or for the declared record counts.
    pub fn decode(buf: &[u8]) -> Result<Self> {
        let mut r = Reader::new(buf);
        let header = Header::decode(&mut r)?;
        let questions = (0..header.qdcount)
            .map(|_| Question::decode(&mut r))
            .collect::<Result<Vec<_>>>()?;
        let read_section = |r: &mut Reader<'_>, n: u16| {
            (0..n).map(|_| ResourceRecord::decode(r)).collect::<Result<Vec<_>>>()
        };
        let answers = read_section(&mut r, header.ancount)?;
        let authority = read_section(&mut r, header.nscount)?;
        let additional = read_section(&mut r, header.arcount)?;
        Ok(Self { header, questions, answers, authority, additional })
    }
}

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
