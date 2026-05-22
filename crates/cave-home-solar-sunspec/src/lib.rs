// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-solar-sunspec` — vendor-agnostic SunSpec Modbus model
//! parser and reader.
//!
//! # Source
//!
//! SunSpec is a **public open standard** maintained by the SunSpec
//! Alliance: <https://sunspec.org/sunspec-information-model-specifications/>.
//! No upstream source code is read or ported — only the public model
//! definitions (PDF specs and the `models` repository:
//! <https://github.com/sunspec/models>).
//!
//! # Coverage
//!
//! cave-home implements the models that cover the SMA / Fronius /
//! SolarEdge / Huawei / Goodwe / Kostal inverter families:
//!
//! | Model | Meaning                                |
//! | ----- | -------------------------------------- |
//! | 1     | Common (manufacturer, model, version)  |
//! | 101   | Single-phase inverter, integer SF      |
//! | 102   | Split-phase inverter, integer SF       |
//! | 103   | Three-phase inverter, integer SF       |
//! | 120   | Nameplate                              |
//! | 121   | Basic settings                         |
//! | 122   | Measurements & status                  |
//! | 123   | Immediate controls                     |
//! | 124   | Storage (battery)                      |
//! | 160   | Multiple-MPPT inverter extension       |
//! | 64204 | Vendor-extension placeholder (Fronius) |
//!
//! Models above 64xxx are vendor-private and treated as opaque blobs.
//!
//! # Charter §6.3 grandma-friendly UX
//!
//! Public types use home-world names — `InverterReading`,
//! `BatteryReading`, `SolarReading`. The model numbers, register
//! addresses, and scale-factor handling are Developer-view-only —
//! exposed through [`raw`] but never re-exported at crate root.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]

pub mod common;
pub mod discovery;
pub mod error;
pub mod inverter;
pub mod mppt;
pub mod nameplate;
pub mod raw;
pub mod reader;
pub mod scale;
pub mod storage;

pub use common::{CommonModel, InverterFamily};
pub use discovery::{DiscoveredModel, ModelHeader, discover_models};
pub use error::{Error, Result};
pub use inverter::{InverterPhase, InverterReading, InverterStatus};
pub use mppt::{MpptModule, MpptReading};
pub use nameplate::{Nameplate, NameplateInverterType};
pub use reader::{ModbusRead, SunSpecReader};
pub use scale::ScaleFactor;
pub use storage::{BatteryReading, StorageChargeStatus};

/// SunSpec well-known TCP base register. Devices place the
/// `0x53756e53` (`"SunS"`) marker at one of these addresses;
/// cave-home probes them in this order. Source: SunSpec Modbus
/// Specification v1.7 §A.3.
pub const SUNSPEC_BASE_REGISTERS: &[u16] = &[40_000, 50_000, 0];

/// The 32-bit SunSpec identifier marker — ASCII `"SunS"`.
pub const SUNSPEC_MARKER: u32 = 0x5375_6e53;

/// End-of-model sentinel. A model ID of 0xFFFF (65535) means
/// "no more models". Source: SunSpec Modbus Specification §B.1.
pub const SUNSPEC_END_MODEL_ID: u16 = 0xFFFF;
