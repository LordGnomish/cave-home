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
//!
//! **Honesty.** Verbs backed by a real `cave-home-unifi` client — `network
//! devices`, `network clients`, `protect cameras`, `access doors` and the
//! `live` views — talk to the console. Every other verb (including the door
//! `unlock` and emergency `lockdown`) has no wired backend: it changes
//! nothing, prints nothing on stdout and exits `EXIT_NOT_CONNECTED`.
//!
//! **TLS.** The consoles present self-signed certificates. The console
//! password / API key and the Access door-unlock token are only sent to a
//! console whose certificate matches a pin (`--fingerprint` /
//! `CAVEHOME_UNIFI_FINGERPRINT`, `--access-fingerprint` /
//! `CAVEHOME_ACCESS_FINGERPRINT`); read it once with `cavehomectl unifi
//! fingerprint <host>`. With no pin the CLI refuses, unless `--insecure`
//! explicitly accepts any certificate (with a warning).

use clap::{Arg, ArgAction, ArgMatches, Command};

use std::str::FromStr;
use std::time::Duration;

use cave_home_unifi::access::{AccessClient, AccessConfig, DEFAULT_ACCESS_PORT};
use cave_home_unifi::console::Console;
use cave_home_unifi::network::NetworkApi;
use cave_home_unifi::protect::ProtectApi;
use cave_home_unifi::render::{self, Lang, Section};
use cave_home_unifi::tls::{PinSet, TlsTrust, capture_presented_certificate};
use cave_home_unifi::transport::ReqwestTransport;
use cave_home_unifi::{ConsoleClient, Credentials};

use crate::commands::not_connected::{not_connected, not_connected_with_hint};

/// Build the top-level `unifi` command.
#[must_use]
pub fn cmd() -> Command {
    // No `arg_required_else_help` — the cross-agent dispatch contract
    // calls `unifi::run()` when no sub is given, and that prints a
    // summary and returns 0.
    Command::new("unifi")
        .about("Control your UniFi network, cameras, doors and intercoms")
        .arg(
            Arg::new("fingerprint")
                .long("fingerprint")
                .global(true)
                .env("CAVEHOME_UNIFI_FINGERPRINT")
                .value_name("SHA256")
                .help(
                    "Pinned SHA-256 of the console certificate (from `cavehomectl unifi \
                     fingerprint <host>`); comma-separate several",
                ),
        )
        .arg(
            Arg::new("access-fingerprint")
                .long("access-fingerprint")
                .global(true)
                .env("CAVEHOME_ACCESS_FINGERPRINT")
                .value_name("SHA256")
                .help("Pinned SHA-256 of the UniFi Access host certificate (default: --fingerprint)"),
        )
        .arg(
            Arg::new("insecure")
                .long("insecure")
                .global(true)
                .action(ArgAction::SetTrue)
                .help(
                    "Accept ANY certificate — unsafe: any device on your network can read the \
                     password and the door token",
                ),
        )
        .subcommand(network_cmd())
        .subcommand(protect_cmd())
        .subcommand(access_cmd())
        .subcommand(talk_cmd())
        .subcommand(live_cmd())
        .subcommand(fingerprint_cmd())
}

/// `cavehomectl unifi fingerprint <host[:port]>` — trust on first use: show
/// the certificate a console presents so the household can pin it.
fn fingerprint_cmd() -> Command {
    Command::new("fingerprint")
        .about("Show a console's certificate fingerprint, to pin it (sends no password)")
        .arg(
            Arg::new("address")
                .required(true)
                .help("Console host or host:port (default port 443; UniFi Access uses 12445)"),
        )
}

/// `cavehomectl unifi live <target>` — connect to a real console (configured
/// via environment) and print live data through the `cave-home-unifi` client.
fn live_cmd() -> Command {
    Command::new("live")
        .about("Connect to a real UniFi console and show live data")
        .long_about(
            "Reads connection settings from the environment and talks to a real \
             UniFi console via the cave-home-unifi client:\n\
             \n  CAVEHOME_UNIFI_HOST      console host/IP (Network + Protect)\
             \n  CAVEHOME_UNIFI_PORT      console port (default 443 / legacy 8443)\
             \n  CAVEHOME_UNIFI_API_KEY   API key, or USER+PASS below\
             \n  CAVEHOME_UNIFI_USER / CAVEHOME_UNIFI_PASS\
             \n  CAVEHOME_UNIFI_KIND      unifios (default) | legacy\
             \n  CAVEHOME_UNIFI_SITE      site name (default: default)\
             \n  CAVEHOME_UNIFI_FINGERPRINT  pinned console certificate SHA-256\
             \n  CAVEHOME_ACCESS_HOST / CAVEHOME_ACCESS_TOKEN  (for doors)\
             \n  CAVEHOME_ACCESS_PORT     (default 12445)\
             \n  CAVEHOME_ACCESS_FINGERPRINT  pinned Access certificate SHA-256\
             \n\
             \nNothing is sent unless the certificate matches the pin \
             (`cavehomectl unifi fingerprint <host>` reads it) or --insecure is given.",
        )
        .arg_required_else_help(true)
        .arg(
            Arg::new("target")
                .required(true)
                .value_parser(["devices", "clients", "cameras", "doors"])
                .help("Which live view to fetch"),
        )
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

/// Verbose-aware dispatcher: real-client verbs go to `cave-home-unifi`, the
/// rest report `EXIT_NOT_CONNECTED`.
#[must_use]
pub fn run_matched(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        Some(("network", sub)) => dispatch_network(sub, verbose),
        Some(("protect", sub)) => dispatch_protect(sub, verbose),
        Some(("access", sub)) => dispatch_access(sub, verbose),
        Some(("talk", sub)) => dispatch_talk(sub),
        Some(("live", sub)) => dispatch_live(sub, verbose),
        Some(("fingerprint", sub)) => dispatch_fingerprint(sub),
        // No sub-pillar: keep the cross-agent contract — print the summary and
        // succeed (the same behaviour the bare `unifi::run()` gives).
        None => run(),
        Some((other, _)) => {
            eprintln!("unifi: unknown sub-pillar '{other}'.");
            2
        }
    }
}

fn dispatch_network(sub: &ArgMatches, verbose: bool) -> i32 {
    match sub.subcommand() {
        Some(("devices", m)) => run_live_target(m, LiveTarget::DEVICES, verbose),
        Some(("clients", m)) => run_live_target(m, LiveTarget::CLIENTS, verbose),
        Some(("status", _)) => not_connected("unifi network status"),
        Some(("block", _)) => not_connected("unifi network block"),
        Some(("unblock", _)) => not_connected("unifi network unblock"),
        Some(("outlet", _)) => not_connected("unifi network outlet"),
        _ => 2,
    }
}

fn dispatch_protect(sub: &ArgMatches, verbose: bool) -> i32 {
    match sub.subcommand() {
        Some(("cameras", m)) => run_live_target(m, LiveTarget::CAMERAS, verbose),
        Some(("status", _)) => not_connected("unifi protect status"),
        Some(("events", _)) => not_connected("unifi protect events"),
        Some(("seam", seam_sub)) => match seam_sub.subcommand() {
            Some(("list", _)) => not_connected("unifi protect seam list"),
            Some(("assign", _)) => not_connected("unifi protect seam assign"),
            _ => 2,
        },
        _ => 2,
    }
}

fn dispatch_access(sub: &ArgMatches, verbose: bool) -> i32 {
    match sub.subcommand() {
        Some(("doors", m)) => run_live_target(m, LiveTarget::Doors, verbose),
        Some(("status", _)) => not_connected("unifi access status"),
        Some(("events", _)) => not_connected("unifi access events"),
        Some(("unlock", _)) => not_connected_with_hint(
            "unifi access unlock",
            "Kapıyı UniFi Access uygulamasından açın.",
            "Unlock the door from the UniFi Access app.",
        ),
        Some(("lockdown", _)) => not_connected_with_hint(
            "unifi access lockdown",
            "Acil durum kilitlemesi için UniFi Access uygulamasını kullanın.",
            "For an emergency lockdown use the UniFi Access app.",
        ),
        _ => 2,
    }
}

fn dispatch_talk(sub: &ArgMatches) -> i32 {
    match sub.subcommand() {
        Some(("status", _)) => not_connected("unifi talk status"),
        Some(("phones", _)) => not_connected("unifi talk phones"),
        Some(("incoming", _)) => not_connected("unifi talk incoming"),
        Some(("control", _)) => not_connected("unifi talk control"),
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// live — the real cave-home-unifi client path (4-track integration).
// ---------------------------------------------------------------------------

/// A view served by the Network / Protect console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsoleView {
    Devices,
    Clients,
    Cameras,
}

/// A live view backed by a real `cave-home-unifi` client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveTarget {
    /// Network / Protect, on the console (`CAVEHOME_UNIFI_*`).
    Console(ConsoleView),
    /// Doors, on the Access host (`CAVEHOME_ACCESS_*`).
    Doors,
}

impl LiveTarget {
    const DEVICES: Self = Self::Console(ConsoleView::Devices);
    const CLIENTS: Self = Self::Console(ConsoleView::Clients);
    const CAMERAS: Self = Self::Console(ConsoleView::Cameras);

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "devices" => Some(Self::DEVICES),
            "clients" => Some(Self::CLIENTS),
            "cameras" => Some(Self::CAMERAS),
            "doors" => Some(Self::Doors),
            _ => None,
        }
    }
}

fn env_port(var: &str) -> Result<Option<u16>, String> {
    std::env::var(var).ok().map_or(Ok(None), |raw| {
        raw.trim()
            .parse::<u16>()
            .map(Some)
            .map_err(|_| format!("{var} must be a port number, got {raw:?}"))
    })
}

/// Network / Protect console settings, read from `CAVEHOME_UNIFI_*`.
struct ConsoleEnv {
    console: Console,
    site: String,
    creds: Credentials,
}

impl ConsoleEnv {
    fn from_env() -> Result<Self, String> {
        let host = std::env::var("CAVEHOME_UNIFI_HOST")
            .map_err(|_| "set CAVEHOME_UNIFI_HOST to your console IP/hostname".to_string())?;
        let kind = std::env::var("CAVEHOME_UNIFI_KIND").unwrap_or_else(|_| "unifios".into());
        let mut console = if kind.eq_ignore_ascii_case("legacy") {
            Console::legacy(host)
        } else {
            Console::unifi_os(host)
        };
        if let Some(port) = env_port("CAVEHOME_UNIFI_PORT")? {
            console = console.with_port(port);
        }
        let site = std::env::var("CAVEHOME_UNIFI_SITE").unwrap_or_else(|_| "default".into());
        let creds = if let Ok(key) = std::env::var("CAVEHOME_UNIFI_API_KEY") {
            Credentials::api_key(key)
        } else {
            let user = std::env::var("CAVEHOME_UNIFI_USER").map_err(|_| {
                "set CAVEHOME_UNIFI_API_KEY, or CAVEHOME_UNIFI_USER + CAVEHOME_UNIFI_PASS"
                    .to_string()
            })?;
            let pass = std::env::var("CAVEHOME_UNIFI_PASS")
                .map_err(|_| "set CAVEHOME_UNIFI_PASS".to_string())?;
            Credentials::password(user, pass)
        };
        Ok(Self {
            console,
            site,
            creds,
        })
    }
}

/// Access (door) host settings, read from `CAVEHOME_ACCESS_*`.
fn access_from_env() -> Result<AccessConfig, String> {
    let (Ok(host), Ok(token)) = (
        std::env::var("CAVEHOME_ACCESS_HOST"),
        std::env::var("CAVEHOME_ACCESS_TOKEN"),
    ) else {
        return Err("set CAVEHOME_ACCESS_HOST + CAVEHOME_ACCESS_TOKEN".into());
    };
    let port = env_port("CAVEHOME_ACCESS_PORT")?.unwrap_or(DEFAULT_ACCESS_PORT);
    Ok(AccessConfig::new(host, token).with_port(port))
}

/// Which host's pin applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinFor {
    Console,
    Access,
}

/// Resolve the TLS trust for `which` from the (global) `--fingerprint`,
/// `--access-fingerprint` and `--insecure` args. With no pin and no
/// `--insecure` this refuses — credentials are never sent unverified.
fn trust_for(m: &ArgMatches, which: PinFor) -> Result<TlsTrust, String> {
    let console_pin = m.get_one::<String>("fingerprint");
    let (pins, env_name) = match which {
        PinFor::Console => (console_pin, "CAVEHOME_UNIFI_FINGERPRINT"),
        PinFor::Access => (
            m.get_one::<String>("access-fingerprint").or(console_pin),
            "CAVEHOME_ACCESS_FINGERPRINT",
        ),
    };
    if let Some(raw) = pins {
        let set = PinSet::from_str(raw).map_err(|e| format!("bad certificate fingerprint ({env_name}): {e}"))?;
        return TlsTrust::pinned(set).map_err(|e| e.to_string());
    }
    if m.get_flag("insecure") {
        eprintln!(
            "UYARI: --insecure — sertifika doğrulaması KAPALI; ağınızdaki herhangi bir cihaz şifreyi ve kapı anahtarını okuyabilir."
        );
        eprintln!(
            "WARNING: --insecure — certificate verification is OFF; any device on your network can read the password and the door token."
        );
        return Ok(TlsTrust::dangerous_accept_any_certificate());
    }
    Err(format!(
        "sertifika sabitlenmemiş — şifre / anahtar gönderilmedi.\n\
         no certificate is pinned for this host — refusing to send the password, API key or door token.\n  \
         Pin it: run `cavehomectl unifi fingerprint <host>`, compare with the console, then set {env_name}.\n  \
         Or knowingly accept any certificate on your network: --insecure"
    ))
}

fn dispatch_live(sub: &ArgMatches, verbose: bool) -> i32 {
    let target = sub
        .get_one::<String>("target")
        .and_then(|t| LiveTarget::parse(t))
        .unwrap_or(LiveTarget::DEVICES);
    run_live_target(sub, target, verbose)
}

/// Everything resolved before a single byte goes on the wire.
enum Prepared {
    Console(ConsoleEnv, TlsTrust, ConsoleView),
    Access(AccessConfig, TlsTrust),
}

fn prepare(m: &ArgMatches, target: LiveTarget) -> Result<Prepared, String> {
    match target {
        LiveTarget::Doors => {
            let cfg = access_from_env()?;
            Ok(Prepared::Access(cfg, trust_for(m, PinFor::Access)?))
        }
        LiveTarget::Console(view) => {
            let env = ConsoleEnv::from_env()?;
            Ok(Prepared::Console(env, trust_for(m, PinFor::Console)?, view))
        }
    }
}

/// Configuration and TLS trust are resolved first; only then is a runtime
/// started and the console contacted.
fn run_live_target(m: &ArgMatches, target: LiveTarget, verbose: bool) -> i32 {
    let prepared = match prepare(m, target) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("unifi: {why}");
            return 2;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("unifi: could not start async runtime: {e}");
            return 1;
        }
    };
    match prepared {
        Prepared::Console(env, trust, view) => runtime.block_on(run_console(&env, &trust, view, verbose)),
        Prepared::Access(cfg, trust) => runtime.block_on(run_doors(cfg, &trust)),
    }
}

async fn run_console(env: &ConsoleEnv, trust: &TlsTrust, view: ConsoleView, verbose: bool) -> i32 {
    let transport = match ReqwestTransport::new(Duration::from_secs(15), trust) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("unifi: {e}");
            return 1;
        }
    };
    let client = ConsoleClient::new(env.console.clone(), transport, env.creds.clone());
    if let Err(e) = client.login().await {
        eprintln!("unifi: login failed: {e}");
        return 1;
    }
    let lang = Lang::Tr;
    let lines = match view {
        ConsoleView::Devices => NetworkApi::new(&client).devices(&env.site).await.map(|devices| {
            let mut out: Vec<String> = devices.iter().map(|d| render::device_line(d, lang)).collect();
            if verbose {
                out.push(format!("  ({} devices on site {})", devices.len(), env.site));
            }
            (Section::Devices, out)
        }),
        ConsoleView::Clients => NetworkApi::new(&client)
            .clients(&env.site)
            .await
            .map(|clients| (Section::Clients, clients.iter().map(|c| render::client_line(c, lang)).collect())),
        ConsoleView::Cameras => ProtectApi::new(&client)
            .cameras()
            .await
            .map(|cams| (Section::Cameras, cams.iter().map(|c| render::camera_line(c, lang)).collect())),
    };
    match lines {
        Ok((section, lines)) => {
            println!("{}", render::header(section, lang));
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err(e) => {
            eprintln!("unifi: {e}");
            1
        }
    }
}

async fn run_doors(cfg: AccessConfig, trust: &TlsTrust) -> i32 {
    let transport = match ReqwestTransport::new(Duration::from_secs(15), trust) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("unifi: {e}");
            return 1;
        }
    };
    let lang = Lang::Tr;
    match AccessClient::new(cfg, transport).doors().await {
        Ok(doors) => {
            println!("{}", render::header(Section::Doors, lang));
            for d in &doors {
                println!("{}", render::door_line(d, lang));
            }
            0
        }
        Err(e) => {
            eprintln!("unifi: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// fingerprint — trust on first use for onboarding.
// ---------------------------------------------------------------------------

/// Split `host[:port]` (IPv6 as `[addr]:port`) into the TLS name and a
/// connectable `host:port`.
fn split_host_port(raw: &str, default_port: u16) -> (String, String) {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(default_port);
            return (host.to_owned(), format!("[{host}]:{port}"));
        }
    }
    match raw.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => port.parse::<u16>().map_or_else(
            |_| (raw.to_owned(), format!("{raw}:{default_port}")),
            |port| (host.to_owned(), format!("{host}:{port}")),
        ),
        Some(_) => (raw.to_owned(), format!("[{raw}]:{default_port}")),
        None => (raw.to_owned(), format!("{raw}:{default_port}")),
    }
}

fn dispatch_fingerprint(sub: &ArgMatches) -> i32 {
    let raw = sub.get_one::<String>("address").map_or("", String::as_str);
    let (host, addr) = split_host_port(raw, 443);
    let env_name = if addr.ends_with(&format!(":{DEFAULT_ACCESS_PORT}")) {
        "CAVEHOME_ACCESS_FINGERPRINT"
    } else {
        "CAVEHOME_UNIFI_FINGERPRINT"
    };
    match capture_presented_certificate(&addr, &host, Duration::from_secs(10)) {
        Ok(cert) => {
            println!("Sertifika parmak izi (SHA-256) / Certificate fingerprint (SHA-256):");
            println!("  {}", cert.certificate_sha256());
            if let Some(pin) = cert.public_key_pin() {
                println!("Açık anahtar pini / Public-key pin (survives a re-issue with the same key):");
                println!("  {pin}");
            }
            println!();
            println!("Konsolun kendi ekranındaki parmak iziyle karşılaştırın; aynıysa sabitleyin:");
            println!("Compare it with the fingerprint the console itself shows; if they match, pin it:");
            println!("  export {env_name}={}", cert.certificate_sha256());
            0
        }
        Err(e) => {
            eprintln!("unifi fingerprint: {addr}: {e}");
            1
        }
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
    fn live_target_is_validated() {
        let c = cmd();
        let live = c.find_subcommand("live").unwrap();
        assert!(
            live.clone()
                .try_get_matches_from(["live", "devices"])
                .is_ok()
        );
        assert!(
            live.clone()
                .try_get_matches_from(["live", "cameras"])
                .is_ok()
        );
        assert!(
            live.clone()
                .try_get_matches_from(["live", "teleport"])
                .is_err()
        );
    }

    #[test]
    fn live_without_host_env_reports_config_error() {
        // With no CAVEHOME_UNIFI_HOST set, the live path must fail cleanly (2),
        // never panic — exercising the real cave-home-unifi link.
        if std::env::var("CAVEHOME_UNIFI_HOST").is_err() {
            let c = cmd();
            let m = c
                .try_get_matches_from(["unifi", "live", "devices"])
                .unwrap();
            assert_eq!(run_matched(&m, false), 2);
        }
    }

    #[test]
    fn split_host_port_handles_defaults_ports_and_ipv6() {
        assert_eq!(split_host_port("10.0.0.1", 443), ("10.0.0.1".into(), "10.0.0.1:443".into()));
        assert_eq!(split_host_port("console:8443", 443), ("console".into(), "console:8443".into()));
        assert_eq!(split_host_port("[fe80::1]:12445", 443), ("fe80::1".into(), "[fe80::1]:12445".into()));
        assert_eq!(split_host_port("fe80::1", 443), ("fe80::1".into(), "[fe80::1]:443".into()));
    }

    fn live_matches(extra: &[&str]) -> ArgMatches {
        let mut argv = vec!["unifi", "live", "devices"];
        argv.extend_from_slice(extra);
        let m = cmd().try_get_matches_from(argv).unwrap();
        m.subcommand_matches("live").unwrap().clone()
    }

    #[test]
    fn trust_refuses_without_a_pin_or_insecure() {
        if std::env::var("CAVEHOME_UNIFI_FINGERPRINT").is_err() {
            let err = trust_for(&live_matches(&[]), PinFor::Console).unwrap_err();
            assert!(err.contains("CAVEHOME_UNIFI_FINGERPRINT") && err.contains("--insecure"));
        }
    }

    #[test]
    fn trust_uses_the_pin_and_access_falls_back_to_it() {
        let fp = cave_home_unifi::tls::Sha256Fingerprint::of(b"console").to_string();
        let m = live_matches(&["--fingerprint", &fp]);
        assert!(matches!(trust_for(&m, PinFor::Console), Ok(TlsTrust::Pinned(_))));
        if std::env::var("CAVEHOME_ACCESS_FINGERPRINT").is_err() {
            assert_eq!(trust_for(&m, PinFor::Access).unwrap(), trust_for(&m, PinFor::Console).unwrap());
        }
        assert!(trust_for(&live_matches(&["--fingerprint", "nope"]), PinFor::Console).is_err());
    }

    #[test]
    fn insecure_is_the_only_way_to_skip_verification() {
        if std::env::var("CAVEHOME_UNIFI_FINGERPRINT").is_err() {
            let t = trust_for(&live_matches(&["--insecure"]), PinFor::Console).unwrap();
            assert!(t.is_dangerous());
        }
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
        assert_eq!(run_matched(&m, false), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }

    #[test]
    fn dispatch_protect_seam_list() {
        let c = cmd();
        let m = c
            .try_get_matches_from(["unifi", "protect", "seam", "list"])
            .unwrap();
        assert_eq!(run_matched(&m, false), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }

    #[test]
    fn dispatch_access_unlock_with_minutes() {
        let c = cmd();
        let m = c
            .try_get_matches_from(["unifi", "access", "unlock", "front", "--minutes", "15"])
            .unwrap();
        assert_eq!(run_matched(&m, true), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }

    #[test]
    fn dispatch_talk_control() {
        let c = cmd();
        let m = c
            .try_get_matches_from(["unifi", "talk", "control", "c1", "decline"])
            .unwrap();
        assert_eq!(run_matched(&m, false), crate::commands::not_connected::EXIT_NOT_CONNECTED);
    }
}
