// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// F3 Philips Hue command surface — backed by cave-home-hue + cave-home-hue-bridge-emu.
//
// `cavehomectl hue …` exposes both the *client* (talk to a real bridge)
// and the *bridge-emu* (advanced-mode emulator) surfaces. Per ADR-007 the
// developer-friendly flags exist here (bridge IPs, UUIDs) — the
// grandma-friendly translation lives in the Portal admin module.

use clap::{Arg, Command};

/// Build the `cavehomectl hue <verb>` subcommand tree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("hue")
        .about("Philips Hue Bridge integration + bridge emulator (ADR-010)")
        // ----- integration (client to a real bridge) -----
        .subcommand(
            Command::new("discover")
                .about("Discover Hue bridges on the network (NUPNP + local probe)"),
        )
        .subcommand(
            Command::new("pair")
                .about("Pair with a discovered bridge (link-button flow)")
                .arg(Arg::new("host").long("host").required(true)),
        )
        .subcommand(Command::new("status").about("Show paired bridges + reachability"))
        .subcommand(
            Command::new("light")
                .about("Control a Hue light (on/off/brightness/colour)")
                .arg(Arg::new("id").long("id").required(true))
                .arg(Arg::new("on").long("on").required(false))
                .arg(Arg::new("bri").long("bri").required(false))
                .arg(Arg::new("xy").long("xy").required(false))
                .arg(Arg::new("ct").long("ct").required(false)),
        )
        .subcommand(
            Command::new("group")
                .about("Apply an action across a group / room / zone")
                .arg(Arg::new("id").long("id").required(true))
                .arg(Arg::new("on").long("on").required(false))
                .arg(Arg::new("bri").long("bri").required(false))
                .arg(Arg::new("scene").long("scene").required(false)),
        )
        .subcommand(
            Command::new("scene")
                .about("Recall a scene")
                .arg(Arg::new("id").long("id").required(true)),
        )
        .subcommand(
            Command::new("sensor")
                .about("Show / configure a sensor (motion / button / temperature)")
                .arg(Arg::new("id").long("id").required(true))
                .arg(Arg::new("on").long("on").required(false)),
        )
        .subcommand(Command::new("events").about("Tail the v2 eventstream"))
        // ----- bridge-emu (advanced-mode) -----
        .subcommand(
            Command::new("bridge-emu")
                .about("Manage the cave-home Hue Bridge emulator (advanced-mode)")
                .subcommand(Command::new("status").about("Show emulator on/off + paired apps"))
                .subcommand(Command::new("enable").about("Begin broadcasting as a Hue Bridge"))
                .subcommand(Command::new("disable").about("Stop broadcasting"))
                .subcommand(Command::new("press").about("Open the link-button window for 30s"))
                .subcommand(Command::new("clients").about("List paired third-party apps"))
                .subcommand(
                    Command::new("revoke")
                        .about("Revoke a paired client by app-key")
                        .arg(Arg::new("app-key").long("app-key").required(true)),
                )
                .subcommand(Command::new("ssdp").about("Dump the SSDP description.xml payload"))
                .subcommand(
                    Command::new("mdns").about("Dump the mDNS _hue._tcp advertisement payload"),
                ),
        )
}

/// Default no-op handler — Phase 2 wires this to cave-home-hue +
/// cave-home-hue-bridge-emu via the cave-home-binary IPC surface.
pub fn run() -> i32 {
    println!("hue: bridge client + emulator wiring lands in Phase 2 binary integration.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_has_all_integration_subcommands() {
        let c = cmd();
        for sub in [
            "discover", "pair", "status", "light", "group", "scene", "sensor", "events",
        ] {
            assert!(
                c.get_subcommands().any(|s| s.get_name() == sub),
                "subcommand {sub} missing"
            );
        }
    }

    #[test]
    fn cmd_has_bridge_emu_subtree() {
        let c = cmd();
        let bridge_emu = c
            .get_subcommands()
            .find(|s| s.get_name() == "bridge-emu")
            .expect("bridge-emu subcommand");
        for sub in [
            "status", "enable", "disable", "press", "clients", "revoke", "ssdp", "mdns",
        ] {
            assert!(
                bridge_emu.get_subcommands().any(|s| s.get_name() == sub),
                "bridge-emu subcommand {sub} missing"
            );
        }
    }

    #[test]
    fn pair_requires_host_arg() {
        let c = cmd();
        let pair = c
            .get_subcommands()
            .find(|s| s.get_name() == "pair")
            .unwrap();
        let args: Vec<_> = pair
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        assert!(args.contains(&"host".into()));
    }

    #[test]
    fn light_takes_id_plus_optional_state_flags() {
        let c = cmd();
        let light = c
            .get_subcommands()
            .find(|s| s.get_name() == "light")
            .unwrap();
        let args: Vec<_> = light
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        for name in ["id", "on", "bri", "xy", "ct"] {
            assert!(args.contains(&name.into()), "arg {name} missing on `light`");
        }
    }

    #[test]
    fn revoke_requires_app_key() {
        let c = cmd();
        let revoke = c
            .get_subcommands()
            .find(|s| s.get_name() == "bridge-emu")
            .unwrap()
            .get_subcommands()
            .find(|s| s.get_name() == "revoke")
            .unwrap();
        let args: Vec<_> = revoke
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        assert!(args.contains(&"app-key".into()));
    }
}
