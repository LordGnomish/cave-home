// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! cave-home-knx — KNX/IP stack (ADR-011).
//!
//! **Mixed-source port (Charter §6.1 / ADR-002).**
//!
//! 1. **Transport + DPTs (`address`, `telegram`, `cemi`, `dpt`, `knxip`)** —
//!    line-by-line port of `XKNX/xknx` v3.15.0
//!    (SHA `50fdf8af8e29b84b96de4487f5bd4f060f7c502c`). MIT-licensed upstream,
//!    Apache-2.0 here; MIT attribution preserved in each ported file's header.
//!
//! 2. **Gateway daemon ([`gateway`])** — **clean-room** KNXd-equivalent
//!    KNXnet/IP routing daemon. KNXd is GPL-3.0; *no KNXd source is ever
//!    consulted*. The daemon is built EXCLUSIVELY from the KNX Association's
//!    public KNX/IP service-code table (KNX Standard 03_08, public summary
//!    pages on knx.org) and the public framing rules already documented in
//!    the `knxip_enum.py` file header of the MIT-licensed xknx upstream.
//!    The Wireshark KNX dissector is also avoided (GPL).
//!
//! Sequenced after `cave-home-free-home` (the immediate Charter §2
//! deliverable). KNX/IP only — no KNX-TP1 serial-bridge support in Phase 1
//! (deferred; tracked in Phase 2 backlog of ADR-011).
//!
//! ## Charter v6 / ADR-007 grandma-friendly UX
//!
//! Nothing in this crate is grandma-facing. Raw KNX group addresses ("1/2/3"),
//! datapoint types ("DPT 9.001"), individual addresses ("1.1.5"), routing
//! multicast endpoint ("224.0.23.12") never leak past the Portal's developer
//! view. End-user-facing labels live in `cave-home-portal::admin::knx`.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
// Numeric arithmetic in DPT decoders trips these by design; xknx's Python
// equivalents accept the same precision tradeoffs.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_lossless)]

pub mod address;
pub mod cemi;
pub mod dpt;
pub mod error;
pub mod gateway;
pub mod knxip;
pub mod telegram;
