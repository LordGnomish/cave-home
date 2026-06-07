// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl unifi` — UniFi ecosystem command surface (ADR-009).
//!
//! Sub-trees mirror the four sub-crates:
//!   * `cavehomectl unifi network ...` — switches, APs, ports, clients.
//!   * `cavehomectl unifi protect ...` — Protect NVR + cameras + events.
//!   * `cavehomectl unifi access ...`  — door state, unlock, emergency.
//!   * `cavehomectl unifi talk ...`    — phones, calls, control.
//!
//! Charter v6 §6.3 / ADR-007 rendering rules apply: default output is
//! grandma-friendly; raw fields (MAC, port id, vlan, GUIDs) appear
//! only with `--verbose`.

use clap::{Arg, ArgAction, ArgMatches, Command};

/// Build the top-level `unifi` command.
#[must_use]
pub fn cmd() -> Command {
    // No `arg_required_else_help` — the cross-agent dispatch contract
    // calls `unifi::run()` when no sub is given, and that prints a
    // summary and returns 0.
    Command::new("unifi")
        .about("Control your UniFi network, cameras, doors and intercoms")
        .subcommand(network_cmd())
        .subcommand(protect_cmd())
        .subcommand(access_cmd())
        .subcommand(talk_cmd())
}

fn network_cmd() -> Command {
    Command::new("network")
        .about("UniFi Network — switches, Wi-Fi noktası, ports, clients")
        .arg_required_else_help(true)
        .subcommand(Command::new("status").about("Show network health overview"))
        .subcommand(Command::new("devices").about("List switches + access points"))
        .subcommand(Command::new("clients").about("List connected devices (phones, laptops)"))
        .subcommand(
            Command::new("block")
                .about("Block a client by MAC address")
                .arg(Arg::new("mac").required(true)),
        )
        .subcommand(
            Command::new("unblock")
                .about("Unblock a client by MAC address")
                .arg(Arg::new("mac").required(true)),
        )
        .subcommand(
            Command::new("outlet")
                .about("Toggle a PoE outlet on a switch port")
                .arg(Arg::new("device").required(true))
                .arg(Arg::new("idx").required(true))
                .arg(
                    Arg::new("on")
                        .long("on")
                        .action(ArgAction::SetTrue)
                        .help("Turn the outlet on (default: off)"),
                ),
        )
}

fn protect_cmd() -> Command {
    Command::new("protect")
        .about("UniFi Protect — kamera ve kapı zili olayları")
        .arg_required_else_help(true)
        .subcommand(Command::new("status").about("Show NVR health"))
        .subcommand(Command::new("cameras").about("List adopted cameras"))
        .subcommand(Command::new("events").about("Show the last N events"))
        .subcommand(
            Command::new("seam")
                .about("Show the Protect ↔ Frigate ownership table")
                .subcommand(Command::new("list"))
                .subcommand(
                    Command::new("assign")
                        .arg(Arg::new("camera").required(true))
                        .arg(
                            Arg::new("subsystem")
                                .required(true)
                                .value_parser(["native", "frigate-ml", "frigate-only"]),
                        ),
                ),
        )
}

fn access_cmd() -> Command {
    Command::new("access")
        .about("UniFi Access — kapı kilitleri ve geçiş kayıtları")
        .arg_required_else_help(true)
        .subcommand(Command::new("status").about("Show hub + emergency status"))
        .subcommand(Command::new("doors").about("List doors and lock state"))
        .subcommand(Command::new("events").about("Show the last N door events"))
        .subcommand(
            Command::new("unlock")
                .about("Temporarily unlock a door (lock-rule type=unlock)")
                .arg(Arg::new("door").required(true))
                .arg(
                    Arg::new("minutes")
                        .long("minutes")
                        .default_value("10")
                        .value_parser(clap::value_parser!(u32)),
                ),
        )
        .subcommand(
            Command::new("lockdown")
                .about("Trigger / clear emergency lockdown")
                .arg(
                    Arg::new("on")
                        .long("on")
                        .action(ArgAction::SetTrue)
                        .help("Activate lockdown; omit to clear"),
                ),
        )
}

fn talk_cmd() -> Command {
    Command::new("talk")
        .about("UniFi Talk — interkom telefon listesi ve çağrıları")
        .arg_required_else_help(true)
        .subcommand(Command::new("status").about("Show Talk hub roster summary"))
        .subcommand(Command::new("phones").about("List TalkPhones in the roster"))
        .subcommand(Command::new("incoming").about("Show active incoming calls"))
        .subcommand(
            Command::new("control")
                .about("Issue a control verb against a call")
                .arg(Arg::new("call").required(true))
                .arg(
                    Arg::new("verb")
                        .required(true)
                        .value_parser(["answer", "decline", "transfer", "end"]),
                ),
        )
}

/// Default dispatcher signature kept compatible with the cross-agent
/// stub in `cli/src/main.rs`.
#[must_use]
pub fn run() -> i32 {
    println!(
        "unifi: subcommand required. Try 'cavehomectl unifi network status', \
             'cavehomectl unifi protect cameras', or '--help'."
    );
    0
}

/// Verbose-aware dispatcher (Phase 1: prints help-style summaries
/// since the wire client isn't connected yet). Phase 2 ticket: pass
/// each subcommand to the matching crate's `XxxClient`.
#[must_use]
pub fn run_matched(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        Some(("network", sub)) => dispatch_network(sub, verbose),
        Some(("protect", sub)) => dispatch_protect(sub, verbose),
        Some(("access", sub)) => dispatch_access(sub, verbose),
        Some(("talk", sub)) => dispatch_talk(sub, verbose),
        _ => {
            eprintln!("unifi: pick a sub-pillar (network/protect/access/talk).");
            2
        }
    }
}

fn dispatch_network(sub: &ArgMatches, verbose: bool) -> i32 {
    match sub.subcommand() {
        Some(("status", _)) => {
            println!("UniFi Network: Wi-Fi sağlıklı (demo).");
            if verbose {
                println!("  controller: not connected (Phase 2 wire-up pending)");
            }
            0
        }
        Some(("devices", _)) => {
            println!("Cihazlar:");
            println!("  • Salon switch (Switch)");
            println!("  • Üst kat Wi-Fi noktası (Wi-Fi noktası)");
            if verbose {
                println!("  ^-- demo data; wire-side fetch is Phase 2");
            }
            0
        }
        Some(("clients", _)) => {
            println!("Bağlı cihazlar:");
            println!("  • Anne iPhone");
            println!("  • Salon TV");
            0
        }
        Some(("block", m)) | Some(("unblock", m)) => {
            let mac = m.get_one::<String>("mac").map(String::as_str).unwrap_or("");
            let verb = if sub.subcommand_name() == Some("block") {
                "engellendi"
            } else {
                "engel kaldırıldı"
            };
            println!("Cihaz {verb} (Phase 1 stub).");
            if verbose {
                println!("  raw mac: {mac}");
            }
            0
        }
        Some(("outlet", m)) => {
            let on = m.get_flag("on");
            let device = m.get_one::<String>("device").map(String::as_str).unwrap_or("");
            let idx = m.get_one::<String>("idx").map(String::as_str).unwrap_or("");
            println!(
                "Outlet {} (Phase 1 stub).",
                if on { "açıldı" } else { "kapandı" }
            );
            if verbose {
                println!("  raw device {device} port {idx}");
            }
            0
        }
        _ => 2,
    }
}

fn dispatch_protect(sub: &ArgMatches, verbose: bool) -> i32 {
    match sub.subcommand() {
        Some(("status", _)) => {
            println!("UniFi Protect: NVR ulaşılabilir (demo).");
            if verbose {
                println!("  REST bootstrap: Phase 2 wire-up pending");
            }
            0
        }
        Some(("cameras", _)) => {
            println!("Kameralar:");
            println!("  • Salon kamerası");
            println!("  • Ön kapı kamerası");
            0
        }
        Some(("events", _)) => {
            println!("Son olaylar: (demo)");
            println!("  • Ön kapı zili çaldı");
            println!("  • Salon kamerasında hareket");
            0
        }
        Some(("seam", seam_sub)) => match seam_sub.subcommand() {
            Some(("list", _)) => {
                println!("Frigate seam (kamera→sistem):");
                println!("  • Salon kamerası → UniFi Protect (Native)");
                println!("  • Garaj RTSP    → Frigate ML");
                0
            }
            Some(("assign", m)) => {
                let cam = m.get_one::<String>("camera").map(String::as_str).unwrap_or("");
                let sys = m
                    .get_one::<String>("subsystem")
                    .map(String::as_str)
                    .unwrap_or("");
                println!("Kamera '{cam}' → '{sys}' atandı (Phase 1 stub).");
                if verbose {
                    println!("  persistence: in-memory only, M2 ticket");
                }
                0
            }
            _ => 2,
        },
        _ => 2,
    }
}

fn dispatch_access(sub: &ArgMatches, verbose: bool) -> i32 {
    match sub.subcommand() {
        Some(("status", _)) => {
            println!("UniFi Access: hub bağlı, acil durum yok (demo).");
            0
        }
        Some(("doors", _)) => {
            println!("Kapılar:");
            println!("  • Ön kapı (kilitli)");
            println!("  • Garaj kapısı (kilitli)");
            0
        }
        Some(("events", _)) => {
            println!("Son geçiş olayları: (demo)");
            println!("  • Ön kapı zili çaldı");
            println!("  • Garaj kapısı açıldı (Burak)");
            0
        }
        Some(("unlock", m)) => {
            let door = m.get_one::<String>("door").map(String::as_str).unwrap_or("");
            let minutes = m.get_one::<u32>("minutes").copied().unwrap_or(10);
            println!("Kapı '{door}' {minutes} dakika açık kalacak (Phase 1 stub).");
            if verbose {
                println!("  lock_rule: type=unlock interval={minutes}");
            }
            0
        }
        Some(("lockdown", m)) => {
            let on = m.get_flag("on");
            println!(
                "Acil durum kilitlemesi {}.",
                if on { "etkin" } else { "kaldırıldı" }
            );
            0
        }
        _ => 2,
    }
}

fn dispatch_talk(sub: &ArgMatches, verbose: bool) -> i32 {
    match sub.subcommand() {
        Some(("status", _)) => {
            println!("UniFi Talk: hub bağlı, aktif çağrı yok (demo).");
            0
        }
        Some(("phones", _)) => {
            println!("Telefonlar:");
            println!("  • Mutfak interkomu (dahili 100)");
            println!("  • Salon interkomu (dahili 101)");
            0
        }
        Some(("incoming", _)) => {
            println!("Gelen çağrı yok (demo).");
            0
        }
        Some(("control", m)) => {
            let call = m.get_one::<String>("call").map(String::as_str).unwrap_or("");
            let verb = m.get_one::<String>("verb").map(String::as_str).unwrap_or("");
            println!("Çağrı '{call}' için '{verb}' isteği gönderildi (Phase 1 stub).");
            if verbose {
                println!("  Phase 1: TalkClient::control_call returns Unavailable");
            }
            0
        }
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_has_four_sub_pillars() {
        let c = cmd();
        let names: Vec<_> = c.get_subcommands().map(|s| s.get_name()).collect();
        for p in ["network", "protect", "access", "talk"] {
            assert!(names.contains(&p), "missing pillar '{p}'");
        }
    }

    #[test]
    fn network_has_block_subcommand() {
        let c = cmd();
        let net = c.find_subcommand("network").unwrap();
        let names: Vec<_> = net.get_subcommands().map(|s| s.get_name()).collect();
        assert!(names.contains(&"block"));
        assert!(names.contains(&"unblock"));
        assert!(names.contains(&"outlet"));
    }

    #[test]
    fn protect_has_seam_subcommand() {
        let c = cmd();
        let p = c.find_subcommand("protect").unwrap();
        let seam = p.find_subcommand("seam").unwrap();
        let names: Vec<_> = seam.get_subcommands().map(|s| s.get_name()).collect();
        assert!(names.contains(&"list"));
        assert!(names.contains(&"assign"));
    }

    #[test]
    fn access_has_unlock_with_minutes() {
        let c = cmd();
        let a = c.find_subcommand("access").unwrap();
        let unlock = a.find_subcommand("unlock").unwrap();
        let m = unlock
            .clone()
            .try_get_matches_from(["unlock", "front", "--minutes", "25"])
            .unwrap();
        assert_eq!(m.get_one::<u32>("minutes").copied(), Some(25));
    }

    #[test]
    fn talk_control_validates_verb() {
        let c = cmd();
        let t = c.find_subcommand("talk").unwrap();
        let ctl = t.find_subcommand("control").unwrap();
        assert!(
            ctl.clone()
                .try_get_matches_from(["control", "c1", "answer"])
                .is_ok()
        );
        assert!(
            ctl.clone()
                .try_get_matches_from(["control", "c1", "shoutAtIt"])
                .is_err()
        );
    }

    #[test]
    fn dispatch_network_status_zero() {
        let c = cmd();
        let m = c
            .try_get_matches_from(["unifi", "network", "status"])
            .unwrap();
        assert_eq!(run_matched(&m, false), 0);
    }

    #[test]
    fn dispatch_protect_seam_list() {
        let c = cmd();
        let m = c
            .try_get_matches_from(["unifi", "protect", "seam", "list"])
            .unwrap();
        assert_eq!(run_matched(&m, false), 0);
    }

    #[test]
    fn dispatch_access_unlock_with_minutes() {
        let c = cmd();
        let m = c
            .try_get_matches_from(["unifi", "access", "unlock", "front", "--minutes", "15"])
            .unwrap();
        assert_eq!(run_matched(&m, true), 0);
    }

    #[test]
    fn dispatch_talk_control() {
        let c = cmd();
        let m = c
            .try_get_matches_from(["unifi", "talk", "control", "c1", "decline"])
            .unwrap();
        assert_eq!(run_matched(&m, false), 0);
    }
}
