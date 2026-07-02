// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// ESPHome command surface — backed by the cave-home-esphome crate (the
// native-API wire codec). The transport that drives the codec is Phase-2.

use clap::{Arg, Command};

/// Build the `cavehomectl esphome <verb>` subcommand tree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("esphome")
        .about("Talk to ESPHome devices over the native API (TCP 6053)")
        .subcommand(Command::new("devices").about("List discovered ESPHome devices on the network"))
        .subcommand(
            Command::new("connect")
                .about("Connect to an ESPHome device and list its entities")
                .arg(Arg::new("host").long("host").required(true))
                .arg(Arg::new("port").long("port").default_value("6053")),
        )
        .subcommand(
            Command::new("monitor")
                .about("Tail live state updates / logs from a connected device")
                .arg(Arg::new("filter").long("filter").required(false)),
        )
        .subcommand(
            Command::new("key")
                .about("Show the native-API entity key for an object id (FNV-1)")
                .arg(Arg::new("object-id").long("object-id").required(true)),
        )
}

/// Default no-op handler — Phase 2 wires the verbs to the cave-home-esphome
/// transport (the codec itself already exists in the crate).
pub fn run() -> i32 {
    println!("esphome: device transport not yet attached — Phase 2.");
    0
}
