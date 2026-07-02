// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl command modules.
//!
//! Ownership map:
//! * F5 (this agent): `init`, `join`, `status`, `destroy`,
//!   `device`, `automation`, `scene` (full implementations) +
//!   the dispatcher in `main.rs`.
//! * F1 agent: `solar` (stub here until F1 merge).
//! * F2 agent: `unifi` (stub here until F2 merge).
//! * F3 agent: `hue` (stub here until F3 merge).
//! * F4 agent: `knx`, `free_home` (stubs here until F4 merge).
//!
//! Each submodule exposes:
//! * `cmd() -> clap::Command` — the clap subtree for the dispatcher.
//! * `run(matches: &clap::ArgMatches) -> i32` — the handler.

pub mod automation;
pub mod destroy;
pub mod device;
pub mod init;
pub mod join;
pub mod room;
pub mod scene;
pub mod status;

pub mod energy;
pub mod free_home;
pub mod hue;
pub mod knx;
pub mod solar;
pub mod unifi;

// Phase 1 G8 (4-track) stubs — one verb per user-facing crate. Each
// module exposes `cmd()` for the clap surface and `run()` returning an
// exit code; the dispatcher in `main.rs` wires them to top-level verbs.
pub mod alarm;
pub mod calendar;
pub mod camera;
pub mod cover;
pub mod display;
pub mod doorbell;
pub mod garden;
pub mod history;
pub mod household;
pub mod hvac;
pub mod lights;
pub mod lock;
pub mod matter;
pub mod mobile;
pub mod music;
pub mod notify;
pub mod pool;
pub mod top;
pub mod vacuum;
pub mod voice;
pub mod water;
pub mod wellness;
pub mod zigbee;
pub mod zwave;
