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

pub mod init;
pub mod join;
pub mod status;
pub mod destroy;
pub mod device;
pub mod room;
pub mod automation;
pub mod scene;

pub mod solar;
pub mod unifi;
pub mod hue;
pub mod knx;
pub mod free_home;
