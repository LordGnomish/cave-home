// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-knx` — a KNX datapoint codec and group-telegram engine (ADR-011).
//!
//! KNX is the dominant European building-automation bus. This crate is the
//! **pure-logic core** of cave-home's KNX support: it turns raw bus bytes into
//! semantic values and back, models the group telegrams that carry them, and
//! describes the result to a household in plain language.
//!
//! Everything here is **std-only and dependency-free** — no external crates, no
//! network, no hardware. It is built from the *public* KNX standard (the
//! datapoint-type tables and the application-layer framing rules). xknx (MIT) is
//! a useful public-behavior reference; the code is first-party. The GPL-3.0
//! KNXd gateway was **not** read — its clean-room equivalent is deferred (see
//! below).
//!
//! # Scope (Phase-1 MVP)
//!
//! Implemented, real and tested here:
//! - [`address`] — [`GroupAddress`] (3-level / 2-level / free) and
//!   [`IndividualAddress`], with parse ⇄ raw ⇄ string round-trips and range
//!   validation.
//! - [`dpt`] — the datapoint-type codec: booleans (1.x / 2.x), 4-bit dimming
//!   (3.x), 1-byte scaling / angle / signed (5.x / 6.x), 16- and 32-bit
//!   integers (7.x / 8.x / 12.x / 13.x), the KNX 2-byte float (9.x), IEEE
//!   floats (14.x) and strings (16.x).
//! - [`apci`] / [`telegram`] — the group services (read / write / response) and
//!   the [`GroupTelegram`] APDU codec, including the ≤6-bit small-payload
//!   optimization.
//! - [`label`] — grandma-friendly EN / DE / TR descriptions of an action.
//!
//! # Deferred to Phase-1b (network / hardware bound)
//!
//! The KNXnet/IP tunneling + routing transport (the UDP wire protocol), the
//! USB / TPUART serial transport, the clean-room KNXd-equivalent gateway daemon,
//! ETS project import, and cave-home-core integration are all enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with an ADR-011 disposition. They wrap
//! this engine; they add no datapoint logic.
//!
//! # Example
//!
//! ```
//! use cave_home_knx::{
//!     address::{GroupAddress, IndividualAddress},
//!     dpt::dpt9,
//!     label::{Action, Lang},
//!     telegram::{GroupTelegram, Payload},
//! };
//!
//! // The thermostat (1.1.5) tells the living-room heating group (1/2/3) it is 21°C.
//! let source = IndividualAddress::parse("1.1.5")?;
//! let group = GroupAddress::parse("1/2/3")?;
//! let payload = dpt9::encode(21.0)?.to_vec();
//! let telegram = GroupTelegram::write(source, group, payload);
//!
//! // It round-trips through the application data unit unchanged.
//! let apdu = telegram.encode_apdu()?;
//! let decoded = GroupTelegram::decode_apdu(source, group, &apdu)?;
//! if let Payload::Bytes(bytes) = &decoded.payload {
//!     assert!((dpt9::decode(bytes)? - 21.0).abs() < 1e-9);
//! }
//!
//! // And the household sees a plain sentence, never a datapoint type.
//! assert_eq!(Action::Temperature { celsius: 21 }.describe(Lang::En), "Temperature 21°");
//! # Ok::<(), cave_home_knx::KnxError>(())
//! ```

// The DPT codec is, by nature, a wall of deliberate integer/float casts between
// the wire representation and semantic values (the same precision tradeoffs the
// public KNX datapoint tables specify). These pedantic cast lints fire by design.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::too_long_first_doc_paragraph
)]

pub mod address;
pub mod apci;
pub mod dpt;
pub mod error;
pub mod label;
pub mod telegram;

pub use address::{GroupAddress, GroupAddressStyle, IndividualAddress};
pub use apci::GroupService;
pub use error::{KnxError, Result};
pub use label::{Action, Lang};
pub use telegram::{GroupTelegram, Payload};
