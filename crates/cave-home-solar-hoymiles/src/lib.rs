//! `cave-home-solar-hoymiles` — Hoymiles microinverter telemetry (clean-room).
//!
//! **Clean-room (Charter §6.1 / ADR-002).** The GPL-3.0 references for Hoymiles
//! microinverters (`lumapu/ahoy` / `AhoyDTU` and `tbnobody/OpenDTU`) **must not be
//! read**. Everything in this crate is implemented from the **public protocol
//! description** of the Hoymiles NRF24 / CMT radio framing and the public field
//! layout of the real-time data record. Upstream source was not read.
//!
//! This crate is the **brain** that turns the bytes a Hoymiles inverter sends
//! over the radio into a verdict a household can act on, with no hardware and no
//! network — all of that is deferred to Phase 1b (see `parity.manifest.toml`).
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`crc`] — the CRC-8 (per-fragment) and CRC-16/Modbus (whole-payload)
//!   checksums, from their published parameter definitions.
//! - [`frame`] — the inverter serial / radio-address model and the request
//!   opcodes (real-time data, device info, alarm data).
//! - [`reassembly`] — numbered-fragment reassembly with out-of-order handling
//!   and missing-fragment detection.
//! - [`telemetry`] — fixed-point decode of a real-time payload into per-panel
//!   and grid readings, with validation (truncated / bad-checksum rejection).
//! - [`family`] — the HM one- / two- / four-panel families and their channel
//!   counts.
//! - [`label`] — the grandma-friendly status + EN/DE/TR localisation
//!   (Charter §6.3, ADR-007).
//!
//! The **radio I/O driver** (NRF24L01 / CMT2300A SPI, the DTU poll loop, power-
//! limit command transmission) and the cave-home-core / cave-home-history glue
//! are hardware/SPI-bound and are deferred to Phase 1b — each is enumerated in
//! `parity.manifest.toml` `[[unmapped]]` with a disposition.
//!
//! # Example
//!
//! ```
//! use cave_home_solar_hoymiles::{
//!     decode, AlarmState, Family, Lang, Reassembler, Fragment, headline,
//! };
//!
//! // An inverter answers a real-time request as numbered radio fragments.
//! // Reassemble them (here a one-panel HM-400 payload split into two parts).
//! # use cave_home_solar_hoymiles::crc::crc16_modbus;
//! # let regs: [u16; 11] = [340, 850, 6400, 0, 2301, 5000, 6400, 123, 0, 5000, 452];
//! # let mut body = Vec::new();
//! # for r in regs { body.extend_from_slice(&r.to_be_bytes()); }
//! # body.extend_from_slice(&crc16_modbus(&body).to_le_bytes());
//! # let (a, b) = body.split_at(12);
//! let mut rx = Reassembler::new();
//! rx.accept(&Fragment::new(1, false, a.to_vec())).unwrap();
//! rx.accept(&Fragment::new(2, true, b.to_vec())).unwrap();
//! let payload = rx.assemble().unwrap();
//!
//! // Decode it and ask for a household-friendly headline.
//! let telem = decode(&payload, Family::OnePanel).unwrap();
//! let line = headline(&telem, AlarmState::Ok, Lang::En);
//! assert_eq!(line, "Your solar panels are making 640 W");
//! ```

pub mod crc;
pub mod family;
pub mod frame;
pub mod label;
pub mod reassembly;
pub mod telemetry;

pub use family::Family;
pub use frame::{Command, InverterSerial, SerialError};
pub use label::{headline, AlarmState, Lang, SolarStatus};
pub use reassembly::{Fragment, Reassembler, ReassemblyError};
pub use telemetry::{decode, DecodeError, PanelReading, Telemetry};
