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

use cave_home_binary::bootstrap::Plan;
use cave_home_binary::cli::{self, Command};
use cave_home_binary::config::{Config, ConfigLayer, Layer};
use cave_home_binary::shutdown::ShutdownPlan;
use cave_home_binary::version::BuildInfo;

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
