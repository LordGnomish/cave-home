// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-coredns-rs` — the embedded cluster-DNS decision core: the
//! in-process `CoreDNS` that K3s runs for Kubernetes service discovery
//! (ADR-004, Orchestration Phase 3).
//!
//! # What this is
//!
//! `CoreDNS` is a DNS server assembled from a *chain of plugins* configured by a
//! Caddy-style `Corefile`. This crate is the **decision core** of that server:
//! the DNS wire protocol and the per-plugin answer logic, implemented as pure,
//! `std`-only code so they can be tested exhaustively without a socket, a
//! cluster, or a clock.
//!
//! * [`name`] — RFC 1035 domain names: labels, length limits, case-insensitive
//!   comparison and RFC 4034 canonical ordering.
//! * [`wire`] — the DNS message header / question on-the-wire codec, including
//!   RFC 1035 §4.1.4 name compression.
//! * [`rr`] — resource-record types, classes and the RDATA codecs (`A`, `AAAA`,
//!   `NS`, `CNAME`, `SOA`, `PTR`, `MX`, `TXT`, `SRV`).
//! * [`message`] — the full DNS [`message::Message`] (header + question +
//!   answer / authority / additional sections) and reply assembly.
//! * [`plugin`] — the Caddy-style plugin chain: the [`plugin::Plugin`] trait,
//!   the request/response state and `fallthrough` dispatch.
//! * [`hosts`] — the `hosts` plugin: a hostfile answered for `A` / `AAAA` and
//!   reverse `PTR`.
//! * [`rewrite`] — the `rewrite` plugin: name / type / class query rewriting.
//! * [`cache`] — the `cache` plugin: positive + negative TTL caching and the
//!   capacity-eviction decision (caller-supplied `now`).
//! * [`kubernetes`] — the `kubernetes` plugin: `svc` / `pod` name parsing and
//!   `ClusterIP` / headless / `ExternalName` / `SRV` / reverse-`PTR` answers.
//! * [`file`] — the `file` plugin: authoritative zone lookup with `CNAME`
//!   chasing, wildcards, delegation and the `NXDOMAIN` / `NODATA` distinction.
//! * [`forward`] — the `forward` plugin: upstream selection policy and the
//!   health/`max_fails` exclusion decision (the network proxy is deferred).
//! * [`corefile`] — the Caddy-style `Corefile` parser producing a config AST.
//!
//! # Honest port method (Charter §6)
//!
//! This is a **behavioural reimplementation** of documented `CoreDNS` plugin
//! semantics and the DNS protocol RFCs from public sources (RFC 1035 / 2782 /
//! 3596 / 4034 and the Apache-2.0 `coredns/coredns` plugin docs). It is **not**
//! a verbatim line-by-line port and is labelled as such in the parity manifest.
//! The UDP/TCP/TLS/QUIC/DoH listeners, the real network `forward` proxy, the
//! live Kubernetes API watch, DNSSEC online signing and Prometheus exposition
//! are **deferred** — they are the I/O and crypto shells around exactly the
//! logic implemented here, each enumerated in `parity.manifest.toml`.
//!
//! # Charter §6.3
//!
//! Cluster DNS is pure infrastructure, hidden from the homeowner. This crate
//! produces **no user-facing strings** — its errors model DNS wire vocabulary
//! (`RCODE`s, parse failures), never the Portal.
//!
//! # Example
//!
//! ```
//! use cave_home_coredns_rs::{Chain, Hosts, Message, Name, Rcode, RecordType};
//!
//! // A one-plugin chain that answers from a hostfile.
//! let chain = Chain::new(vec![Box::new(Hosts::parse("10.0.0.1 web.local"))]);
//!
//! let query = Message::query(Name::parse("web.local").unwrap(), RecordType::A, 0x42);
//! let reply = chain.handle(&query);
//!
//! assert_eq!(reply.header.rcode, Rcode::NoError);
//! assert!(reply.header.aa);
//! assert_eq!(reply.answers.len(), 1);
//!
//! // The reply re-encodes to valid wire bytes and decodes back.
//! let bytes = reply.encode();
//! let decoded = Message::decode(&bytes).unwrap();
//! assert_eq!(decoded.answers, reply.answers);
//! assert_eq!(decoded.header.ancount, 1); // section counts are derived on encode
//! ```

#![forbid(unsafe_code)]
// DNS counts and TTLs are unsigned 16/32-bit wire fields; the casts between
// them and `usize`/`i64` indices are intentional and bounded by the protocol.
#![allow(clippy::cast_possible_truncation)]

pub mod arpa;
pub mod build;
pub mod builtins;
pub mod cache;
pub mod corefile;
pub mod error;
pub mod file;
pub mod forward;
pub mod hosts;
pub mod kubernetes;
pub mod message;
pub mod name;
pub mod plugin;
pub mod rewrite;
pub mod rr;
pub mod server;
pub mod wire;

pub use build::{DIRECTIVES, build_chain, priority};
pub use builtins::{Errors, Metrics, MetricsSnapshot, Ready, format_log_line};
pub use cache::{Cache, CacheKey, CachePlugin};
pub use corefile::{Corefile, Directive, ServerBlock};
pub use error::{Result, WireError};
pub use file::{FilePlugin, Zone, ZoneReply};
// `forward::Policy` (load-balancing) is re-exported under a distinct name to
// avoid colliding with `rewrite::Policy` (rule continue/stop).
pub use forward::{Forward, Policy as ForwardPolicy, Pool};
pub use hosts::Hosts;
pub use kubernetes::{Endpoint, Kubernetes, Port, Protocol, Service};
pub use message::{Message, Question};
pub use name::Name;
pub use plugin::{Chain, Next, Outcome, Plugin, Request, ServerError};
pub use rewrite::{NameRule, Policy, Rewriter, Rule};
pub use rr::{Class, Rdata, RecordType, ResourceRecord};
pub use server::{MAX_UDP_PAYLOAD, Resolver, serve_tcp, serve_udp};
pub use wire::{Header, Opcode, Rcode, Reader, Writer};
