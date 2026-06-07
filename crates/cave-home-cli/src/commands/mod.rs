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

pub mod get;
pub mod init;
pub mod join;
pub mod status;
pub mod destroy;
pub mod device;
pub mod room;
pub mod automation;
pub mod scene;

pub mod solar;
pub mod energy;
pub mod unifi;
pub mod hue;
pub mod knx;
pub mod free_home;

// Phase 1 G8 (4-track) stubs — one verb per user-facing crate. Each
// module exposes `cmd()` for the clap surface and `run()` returning an
// exit code; the dispatcher in `main.rs` wires them to top-level verbs.
pub mod lights;
pub mod cover;
pub mod lock;
pub mod vacuum;
pub mod hvac;
pub mod camera;
pub mod doorbell;
pub mod alarm;
pub mod water;
pub mod garden;
pub mod pool;
pub mod voice;
pub mod music;
pub mod notify;
pub mod display;
pub mod history;
pub mod wellness;
pub mod calendar;
pub mod household;
pub mod matter;
pub mod zigbee;
pub mod zwave;
pub mod mobile;
