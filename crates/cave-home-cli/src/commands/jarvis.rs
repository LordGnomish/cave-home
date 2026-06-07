// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl jarvis` — the local voice assistant.
//!
//! ```text
//!   cavehomectl jarvis ask --text "turn the kitchen light on"   # try one command
//!   cavehomectl jarvis tools                                     # what it can do
//!   cavehomectl jarvis wake                                      # wake-word config
//! ```
//!
//! The backend is the `cave-home-jarvis` crate compiled into the one binary
//! (Charter §5). `ask` really runs the crate's NLU fast-path — the same
//! `cave_home_voice` intent match and `intent_to_tool_call` bridge the live
//! pipeline uses — so the CLI and the assistant agree on what a sentence means.
//! The wake/STT/LLM/TTS engines need on-device models and a microphone, so the
//! live `listen` loop is Phase-1b.

use clap::{Arg, ArgMatches, Command};

use cave_home_jarvis::tools::{builtin_tools, intent_to_tool_call};
use cave_home_jarvis::JarvisConfig;
use cave_home_voice::{understand, Lang, Understanding};

/// Build the `jarvis` clap subtree.
#[must_use]
pub fn cmd() -> Command {
    Command::new("jarvis")
        .about("Talk to your home — the local voice assistant")
        .subcommand_required(false)
        .subcommand(
            Command::new("ask")
                .about("Try a spoken command as text (no microphone needed)")
                .arg(Arg::new("text").long("text").required(true).help("what you'd say")),
        )
        .subcommand(Command::new("tools").about("What the assistant can control"))
        .subcommand(Command::new("wake").about("Show the wake words it listens for"))
}

/// Entry from `main.rs`.
#[must_use]
pub fn run() -> i32 {
    let after: Vec<std::ffi::OsString> = std::env::args_os()
        .skip_while(|s| s.to_str() != Some("jarvis"))
        .collect();
    if after.is_empty() {
        return dispatch(&cmd().get_matches_from(["jarvis"]), false);
    }
    dispatch(&cmd().get_matches_from(after), false)
}

/// Internal dispatcher — exposed for unit tests.
#[must_use]
pub fn dispatch(matches: &ArgMatches, verbose: bool) -> i32 {
    match matches.subcommand() {
        Some(("ask", m)) => {
            let text = m.get_one::<String>("text").map(String::as_str).unwrap_or("");
            print!("{}", render_ask(text, verbose));
            0
        }
        None | Some(("tools", _)) => {
            print!("{}", render_tools(verbose));
            0
        }
        Some(("wake", _)) => {
            print!("{}", render_wake(&JarvisConfig::default()));
            0
        }
        _ => 2,
    }
}

/// Run the NLU fast-path on `text` and describe what would happen.
#[must_use]
pub fn render_ask(text: &str, verbose: bool) -> String {
    let intents = match cave_home_voice::intents::builtin_intents() {
        Ok(i) => i,
        Err(_) => return "I couldn't load my command set.\n".to_string(),
    };
    let mut out = String::new();
    match understand(text, &intents, Lang::En) {
        Understanding::Acted { action, reply, .. } => {
            out.push_str(&reply);
            out.push('\n');
            if verbose {
                let call = intent_to_tool_call(&action);
                out.push_str(&format!(
                    "[developer] tool={} args={}\n",
                    call.name(),
                    call.arguments()
                ));
            }
        }
        Understanding::NeedsClarification { .. } => {
            out.push_str("I'm not sure which one you meant — could you be more specific?\n");
        }
        Understanding::NotUnderstood { .. } => {
            out.push_str(
                "I didn't catch a command in that. I'd pass it to the local assistant model \
                 (that needs the on-device model, coming in Phase 2).\n",
            );
        }
    }
    out
}

/// List the assistant's tool surface in home-world language.
#[must_use]
pub fn render_tools(verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("What I can do\n");
    out.push_str("=============\n");
    for spec in builtin_tools() {
        out.push_str(&format!("  • {}\n", spec.function.description));
        if verbose {
            out.push_str(&format!("      [developer] {}\n", spec.function.name));
        }
    }
    out
}

/// Show the configured wake words.
#[must_use]
pub fn render_wake(config: &JarvisConfig) -> String {
    let mut out = String::new();
    out.push_str("Wake words\n");
    out.push_str("==========\n");
    for kw in &config.wake_keywords {
        out.push_str(&format!("  • \"{kw}\"\n"));
    }
    out.push_str("Say one of these, then your command.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_name_is_jarvis() {
        assert_eq!(cmd().get_name(), "jarvis");
    }

    #[test]
    fn cmd_has_subcommands() {
        let names: Vec<_> = cmd().get_subcommands().map(|s| s.get_name().to_string()).collect();
        for n in ["ask", "tools", "wake"] {
            assert!(names.iter().any(|x| x == n), "missing sub {n}");
        }
    }

    #[test]
    fn ask_understands_a_real_command() {
        let out = render_ask("turn the kitchen light on", false);
        assert!(out.to_lowercase().contains("turning"), "got: {out}");
    }

    #[test]
    fn ask_verbose_shows_the_resolved_tool() {
        let out = render_ask("turn the kitchen light on", true);
        assert!(out.contains("tool=set_light"));
        assert!(out.contains("kitchen"));
    }

    #[test]
    fn ask_gibberish_falls_back_to_model_note() {
        let out = render_ask("wibble wobble flim", false);
        assert!(out.contains("Phase 2") || out.contains("didn't catch"));
    }

    #[test]
    fn tools_lists_the_builtin_surface() {
        let out = render_tools(false);
        assert!(out.contains("light"));
        // Six tools -> six bullet lines.
        assert_eq!(out.matches('•').count(), 6);
    }

    #[test]
    fn wake_shows_default_keyword() {
        let out = render_wake(&JarvisConfig::default());
        assert!(out.contains("jarvis"));
    }

    #[test]
    fn dispatch_ask_exits_zero() {
        let m = cmd().get_matches_from(["jarvis", "ask", "--text", "turn the kitchen light on"]);
        assert_eq!(dispatch(&m, false), 0);
    }

    #[test]
    fn dispatch_tools_exits_zero() {
        let m = cmd().get_matches_from(["jarvis", "tools"]);
        assert_eq!(dispatch(&m, false), 0);
    }
}
