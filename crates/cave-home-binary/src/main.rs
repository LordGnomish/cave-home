// SPDX-License-Identifier: Apache-2.0
//! cave-home unified binary entry point (Charter §5).
//!
//! Every cave-home stack on a node compiles into this one binary. This entry
//! point is intentionally thin: it parses the command line and dispatches into
//! the tested pure-logic library ([`cave_home_binary`]). The async runtime, the
//! real component launch, and signal handling are deferred Phase 1b work — until
//! then `run` reports the bring-up *plan* it would execute rather than starting
//! the process.

use std::process::ExitCode;
use std::time::Duration;

use cave_home_binary::bootstrap::Plan;
use cave_home_binary::cli::{self, Command, ServeRole};
use cave_home_binary::config::{Config, ConfigLayer, Layer};
use cave_home_binary::node::LocalNode;
use cave_home_binary::server::{self, RuntimeConfig};
use cave_home_binary::shutdown::ShutdownPlan;
use cave_home_binary::version::BuildInfo;
use cave_home_orchestration::role::NodeIntent;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&argv) {
        Ok(command) => dispatch(&command),
        Err(err) => {
            eprintln!("cave-home: {err}");
            eprintln!("Try `cave-home help`.");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(command: &Command) -> ExitCode {
    match command {
        Command::Help { topic } => {
            println!("{}", cli::help_text(topic.as_deref()));
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("{}", BuildInfo::current().line());
            ExitCode::SUCCESS
        }
        Command::ConfigCheck => match resolve(None) {
            Ok(_) => {
                println!("Your settings look good.");
                ExitCode::SUCCESS
            }
            Err(code) => code,
        },
        Command::ConfigShow => match resolve(None) {
            Ok(cfg) => {
                show_config(&cfg);
                ExitCode::SUCCESS
            }
            Err(code) => code,
        },
        Command::Run { flags } => match resolve(Some(flags)) {
            Ok(cfg) => report_plan(&cfg),
            Err(code) => code,
        },
        Command::Serve { role, flags } => match resolve(Some(flags)) {
            Ok(cfg) => boot(*role, &cfg),
            Err(code) => code,
        },
        // The following are real surfaces whose action is the deferred Phase 1b
        // wiring; the pure-logic core models them, the launcher executes them.
        Command::Status => {
            println!("Status reporting needs a running home; start it with `cave-home run`.");
            ExitCode::SUCCESS
        }
        Command::NodeList | Command::NodeJoin { .. } | Command::Backup { .. } | Command::Restore { .. } => {
            println!("This needs a running home; start it with `cave-home run`.");
            ExitCode::SUCCESS
        }
    }
}

/// Resolve the layered config. File and environment layers are empty here (the
/// loaders are Phase 1b); CLI flags are merged on top of the built-in defaults.
fn resolve(flags: Option<&ConfigLayer>) -> Result<Config, ExitCode> {
    let empty_flags = ConfigLayer::empty(Layer::Flags);
    let flags = flags.unwrap_or(&empty_flags);
    Config::resolve_standard(
        &ConfigLayer::empty(Layer::File),
        &ConfigLayer::empty(Layer::Env),
        flags,
    )
    .map_err(|e| {
        eprintln!("cave-home: {e}");
        ExitCode::FAILURE
    })
}

fn show_config(cfg: &Config) {
    println!("Home name:   {}", cfg.node_name);
    println!("Node kind:   {}", cfg.role.as_str());
    println!("Data folder: {}", cfg.data_dir);
    println!("Network:     {}:{}", cfg.bind_addr, cfg.bind_port);
    println!("Detail:      {}", cfg.log_level.as_str());
    print!("Features:   ");
    for c in &cfg.components {
        print!(" {}", c.friendly_name());
    }
    println!();
}

/// Boot the cluster runtime in the requested K3s-style role. Builds the tokio
/// runtime, then blocks on [`server::run`] until a shutdown signal arrives.
fn boot(role: ServeRole, cfg: &Config) -> ExitCode {
    let intent = match role {
        // A dedicated etcd member still hosts the datastore + apiserver locally;
        // a fuller etcd-only topology is a follow-up (see the handoff doc).
        ServeRole::Server | ServeRole::Etcd => NodeIntent::PrimaryHub,
        ServeRole::Agent => NodeIntent::Worker,
    };
    // The node advertises a concrete address; a wildcard bind falls back to
    // loopback until real interface discovery lands.
    let internal_ip = match cfg.bind_addr.as_str() {
        "0.0.0.0" | "::" | "" => "127.0.0.1".to_string(),
        other => other.to_string(),
    };
    let rt_cfg = RuntimeConfig {
        intent,
        node: LocalNode::new(cfg.node_name.clone(), internal_ip),
        bind_addr: cfg.bind_addr.clone(),
        bind_port: cfg.bind_port,
        // A snappy reconcile cadence so a freshly-applied pod is scheduled and
        // run within a second or two rather than up to a tick later.
        reconcile_interval: Duration::from_secs(1),
        // TLS is opt-in via config (a follow-up CLI flag will populate this); the
        // default boot serves plain HTTP exactly as before.
        #[cfg(feature = "tls")]
        tls: None,
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cave-home: could not start the runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("Starting your home as a {} node (Ctrl-C to stop)…", role.as_str());
    match runtime.block_on(server::run(rt_cfg)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cave-home: {e}");
            ExitCode::FAILURE
        }
    }
}

fn report_plan(cfg: &Config) -> ExitCode {
    match Plan::compute(cfg) {
        Ok(plan) => {
            println!("Your home `{}` would start these parts, in order:", cfg.node_name);
            for (i, c) in plan.steps().iter().enumerate() {
                println!("  {}. {}", i + 1, c.friendly_name());
            }
            let down = ShutdownPlan::from_bootstrap(&plan);
            println!("(On stop it winds down in the reverse order: {} parts.)", down.len());
            println!("Actually starting the home is the next milestone.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cave-home: {e}");
            ExitCode::FAILURE
        }
    }
}
