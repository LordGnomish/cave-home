// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// F4 free@home command surface — backed by cave-home-free-home crate.

use clap::{Arg, Command};

/// Build the `cavehomectl free-home <verb>` subcommand tree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("free-home")
        .about("Busch-Jaeger free@home / SysAP control")
        .subcommand(
            Command::new("sysap")
                .about("Show System Access Point info")
                .arg(Arg::new("host").long("host").required(false)),
        )
        .subcommand(Command::new("devices").about("List free@home devices"))
        .subcommand(Command::new("channels").about("List channels (lights, blinds, sensors)"))
        .subcommand(
            Command::new("light")
                .about("Turn a light on/off / set brightness")
                .arg(Arg::new("device").long("device").required(true))
                .arg(Arg::new("channel").long("channel").required(true))
                .arg(Arg::new("value").long("value").required(true)),
        )
        .subcommand(
            Command::new("blind")
                .about("Open/close a blind or set its position")
                .arg(Arg::new("device").long("device").required(true))
                .arg(Arg::new("channel").long("channel").required(true))
                .arg(Arg::new("action").long("action").required(true)), // open|close|stop
        )
        .subcommand(
            Command::new("scene")
                .about("Trigger a scene")
                .arg(Arg::new("scene").long("scene").required(true)),
        )
        .subcommand(Command::new("watch").about("Stream live SysAP datapoint updates"))
}

/// Default no-op handler — Phase 2 wires this to cave-home-free-home.
pub fn run() -> i32 {
    println!("free-home: SysAP connection not yet bootstrapped — Phase 2b.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_has_all_phase1_subcommands() {
        let c = cmd();
        for sub in [
            "sysap", "devices", "channels", "light", "blind", "scene", "watch",
        ] {
            assert!(
                c.get_subcommands().any(|s| s.get_name() == sub),
                "subcommand {sub} missing"
            );
        }
    }

    #[test]
    fn light_requires_device_channel_value() {
        let c = cmd();
        let light = c
            .get_subcommands()
            .find(|s| s.get_name() == "light")
            .unwrap();
        let args: Vec<_> = light
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        for name in ["device", "channel", "value"] {
            assert!(
                args.contains(&name.to_string()),
                "arg {name} missing on `light`"
            );
        }
    }
}
