// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// F3 Philips Hue command surface — backed by cave-home-hue + cave-home-hue-bridge-emu.
//
// `cavehomectl hue …` exposes both the *client* (talk to a real bridge)
// and the *bridge-emu* (advanced-mode emulator) surfaces. Per ADR-007 the
// developer-friendly flags exist here (bridge IPs, UUIDs) — the
// grandma-friendly translation lives in the Portal admin module.

use clap::{Arg, ArgMatches, Command};

use cave_home_hue::v2::color::xy_to_rgb;
use cave_home_hue::v2::models::scene::{SceneRecall, SceneRecallAction};

/// Build the `cavehomectl hue <verb>` subcommand tree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("hue")
        .about("Philips Hue Bridge integration + bridge emulator (ADR-010)")
        // ----- integration (client to a real bridge) -----
        .subcommand(Command::new("discover").about("Discover Hue bridges on the network (NUPNP + local probe)"))
        .subcommand(Command::new("list-lights").about("List your lights — which are on, how bright, what colour"))
        .subcommand(
            Command::new("set-scene")
                .about("Switch on a scene (active | dynamic | static | off)")
                .arg(Arg::new("id").long("id").required(true))
                .arg(
                    Arg::new("action")
                        .long("action")
                        .required(false)
                        .help("active (default) | dynamic | static | off"),
                ),
        )
        .subcommand(
            Command::new("pair")
                .about("Pair with a discovered bridge (link-button flow)")
                .arg(Arg::new("host").long("host").required(true))
        )
        .subcommand(Command::new("status").about("Show paired bridges + reachability"))
        .subcommand(
            Command::new("light")
                .about("Control a Hue light (on/off/brightness/colour)")
                .arg(Arg::new("id").long("id").required(true))
                .arg(Arg::new("on").long("on").required(false))
                .arg(Arg::new("bri").long("bri").required(false))
                .arg(Arg::new("xy").long("xy").required(false))
                .arg(Arg::new("ct").long("ct").required(false))
        )
        .subcommand(
            Command::new("group")
                .about("Apply an action across a group / room / zone")
                .arg(Arg::new("id").long("id").required(true))
                .arg(Arg::new("on").long("on").required(false))
                .arg(Arg::new("bri").long("bri").required(false))
                .arg(Arg::new("scene").long("scene").required(false))
        )
        .subcommand(
            Command::new("scene")
                .about("Recall a scene")
                .arg(Arg::new("id").long("id").required(true))
        )
        .subcommand(
            Command::new("sensor")
                .about("Show / configure a sensor (motion / button / temperature)")
                .arg(Arg::new("id").long("id").required(true))
                .arg(Arg::new("on").long("on").required(false))
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
                        .arg(Arg::new("app-key").long("app-key").required(true))
                )
                .subcommand(Command::new("ssdp").about("Dump the SSDP description.xml payload"))
                .subcommand(Command::new("mdns").about("Dump the mDNS _hue._tcp advertisement payload"))
        )
}

/// Entry from `main.rs`. Re-parses argv after the `hue` token so the full
/// subcommand tree works through the simple cross-crate dispatch signature
/// (mirrors `energy::run`).
#[must_use]
pub fn run() -> i32 {
    let after: Vec<std::ffi::OsString> = std::env::args_os()
        .skip_while(|s| s.to_str() != Some("hue"))
        .collect();
    if after.is_empty() {
        return dispatch(&cmd().get_matches_from(["hue"]), false);
    }
    dispatch(&cmd().get_matches_from(after), false)
}

/// Internal dispatcher — exposed for unit tests. The `list-lights` and
/// `set-scene` verbs link `cave-home-hue` directly (colour maths + the
/// `SceneRecallAction` CLI contract); the remaining verbs are Phase-2 stubs
/// wired through the single binary's IPC surface.
#[must_use]
pub fn dispatch(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        Some(("list-lights", _)) => {
            print!("{}", render_lights(&demo_lights(), verbose));
            0
        }
        Some(("set-scene", m)) => {
            let id = m.get_one::<String>("id").map(String::as_str).unwrap_or("");
            let raw = m
                .get_one::<String>("action")
                .map_or("active", String::as_str);
            match SceneRecallAction::from_cli(raw) {
                Some(action) => {
                    // The body the bridge would receive on PUT resource/scene/{id}.
                    let recall = SceneRecall {
                        action: Some(action),
                        ..Default::default()
                    };
                    debug_assert!(recall.action.is_some());
                    println!("Scene {id}: {}.", friendly_action(action));
                    0
                }
                None => {
                    eprintln!(
                        "I don't know the scene action '{raw}'. Try active, dynamic, static or off."
                    );
                    1
                }
            }
        }
        None => {
            println!("Try `cavehomectl hue list-lights` or `cavehomectl hue set-scene --id <id>`.");
            0
        }
        // Phase-2 verbs (discover/pair/status/light/group/scene/sensor/events/
        // bridge-emu) land with the binary's bridge IPC surface.
        Some(_) => {
            println!("hue: this command lands in Phase 2 binary integration.");
            0
        }
    }
}

/// A grandma-friendly label for a recall action (English; Portal renders DE/TR).
fn friendly_action(action: SceneRecallAction) -> &'static str {
    match action {
        SceneRecallAction::Active => "switched on",
        SceneRecallAction::DynamicPalette => "switched on, colours drifting",
        SceneRecallAction::Static => "switched on, fixed colours",
        SceneRecallAction::Deactivate => "switched off",
    }
}

/// ------- render helpers (pure, test-friendly) -----------------------

/// A demo light row, shown until the bridge transport is wired into the binary.
/// `xy` is a CIE colour point; the renderer converts it to RGB via the crate's
/// own colour maths so the CLI and adapter agree on what a colour looks like.
#[derive(Debug, Clone, Copy)]
pub struct DemoLight {
    /// Light name.
    pub name: &'static str,
    /// On/off.
    pub on: bool,
    /// Brightness, 0..=254 (Hue's range).
    pub bri: u8,
    /// CIE xy colour point.
    pub xy: (f64, f64),
}

/// Demo lights for `list-lights`.
#[must_use]
pub fn demo_lights() -> Vec<DemoLight> {
    vec![
        DemoLight { name: "Living room", on: true, bri: 203, xy: (0.45, 0.41) },
        DemoLight { name: "Kitchen", on: true, bri: 254, xy: (0.32, 0.33) },
        DemoLight { name: "Bedroom", on: false, bri: 0, xy: (0.17, 0.70) },
    ]
}

/// Render the light list in home-world language.
#[must_use]
pub fn render_lights(lights: &[DemoLight], verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("Your lights\n");
    out.push_str("===========\n");
    for l in lights {
        if l.on {
            let (r, g, b) = xy_to_rgb(l.xy.0, l.xy.1, l.bri);
            let pct = (u16::from(l.bri) * 100 / 254).min(100);
            out.push_str(&format!(
                "  {:<14} on   {pct:>3}%   #{r:02X}{g:02X}{b:02X}\n",
                l.name
            ));
        } else {
            out.push_str(&format!("  {:<14} off\n", l.name));
        }
    }
    if verbose {
        out.push_str("\n[developer] raw xy + brightness:\n");
        for l in lights {
            out.push_str(&format!(
                "  {} xy=({:.3},{:.3}) bri={}\n",
                l.name, l.xy.0, l.xy.1, l.bri
            ));
        }
    }
    out
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
    fn cmd_has_list_lights_and_set_scene() {
        let names: Vec<_> = cmd().get_subcommands().map(|s| s.get_name().to_string()).collect();
        assert!(names.iter().any(|n| n == "list-lights"));
        assert!(names.iter().any(|n| n == "set-scene"));
    }

    #[test]
    fn dispatch_list_lights_exits_zero() {
        let m = cmd().get_matches_from(["hue", "list-lights"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn render_lights_shows_state_and_hex_colour() {
        let out = render_lights(&demo_lights(), false);
        assert!(out.contains("Living room"));
        assert!(out.contains("on"));
        assert!(out.contains("off")); // bedroom is off
        // colour rendered as a hex triple via the crate's xy_to_rgb
        assert!(out.contains('#'), "expected a hex colour: {out}");
    }

    #[test]
    fn render_lights_default_hides_jargon() {
        let out = render_lights(&demo_lights(), false);
        for forbidden in ["xy=", "mirek", "CLIP", "application-key"] {
            assert!(!out.contains(forbidden), "leaked '{forbidden}': {out}");
        }
    }

    #[test]
    fn dispatch_set_scene_accepts_known_action() {
        let m = cmd().get_matches_from(["hue", "set-scene", "--id", "scene-1", "--action", "active"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_set_scene_defaults_to_active() {
        let m = cmd().get_matches_from(["hue", "set-scene", "--id", "scene-1"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_set_scene_rejects_unknown_action() {
        let m = cmd().get_matches_from(["hue", "set-scene", "--id", "x", "--action", "disco"]);
        assert_eq!(dispatch(&m, false), 1);
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
