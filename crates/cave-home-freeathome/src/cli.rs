// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-ctl freeathome` subcommand parsing.
//!
//! Hand-rolled to match `cave-home-binary`'s parser style (no clap). The binary
//! strips the leading `freeathome` token and hands the remainder to [`parse`];
//! the async client (see [`crate::client`]) executes the resulting command.
//! Keeping parsing pure makes the CLI surface fully unit-testable without a SysAP.

use std::fmt;

/// A parsed `freeathome` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeAtHomeCommand {
    /// List discovered devices and their state.
    List,
    /// Read one datapoint's current value.
    Get {
        /// Device serial.
        serial: String,
        /// Channel id (`ch0000`).
        channel: String,
        /// Datapoint id (`odp0000`).
        datapoint: String,
    },
    /// Write a value to one datapoint.
    Set {
        /// Device serial.
        serial: String,
        /// Channel id (`ch0000`).
        channel: String,
        /// Datapoint id (`idp0000`).
        datapoint: String,
        /// Wire value to write.
        value: String,
    },
    /// Stream live device-state changes.
    Watch,
}

/// Why a `freeathome` command line failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// A subcommand was missing required positional arguments.
    MissingArgs(String),
    /// The subcommand is not recognised.
    UnknownSubcommand(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArgs(m) => write!(f, "missing arguments: {m}\n\n{}", usage()),
            Self::UnknownSubcommand(s) => {
                write!(f, "unknown subcommand: {s}\n\n{}", usage())
            }
        }
    }
}

impl std::error::Error for CliError {}

/// Parse the arguments that follow `cave-home-ctl freeathome`.
pub fn parse(args: &[String]) -> core::result::Result<FreeAtHomeCommand, CliError> {
    let (head, rest) = args
        .split_first()
        .ok_or_else(|| CliError::MissingArgs("expected a subcommand".to_string()))?;
    match head.as_str() {
        "list" => Ok(FreeAtHomeCommand::List),
        "watch" => Ok(FreeAtHomeCommand::Watch),
        "get" => match rest {
            [serial, channel, datapoint] => Ok(FreeAtHomeCommand::Get {
                serial: serial.clone(),
                channel: channel.clone(),
                datapoint: datapoint.clone(),
            }),
            _ => Err(CliError::MissingArgs(
                "get <serial> <channel> <datapoint>".to_string(),
            )),
        },
        "set" => match rest {
            [serial, channel, datapoint, value] => Ok(FreeAtHomeCommand::Set {
                serial: serial.clone(),
                channel: channel.clone(),
                datapoint: datapoint.clone(),
                value: value.clone(),
            }),
            _ => Err(CliError::MissingArgs(
                "set <serial> <channel> <datapoint> <value>".to_string(),
            )),
        },
        other => Err(CliError::UnknownSubcommand(other.to_string())),
    }
}

/// The usage text for the `freeathome` subcommand.
pub fn usage() -> String {
    [
        "usage: cave-home-ctl freeathome <command>",
        "  list                                  list devices and state",
        "  get  <serial> <channel> <datapoint>   read a datapoint",
        "  set  <serial> <channel> <datapoint> <value>   write a datapoint",
        "  watch                                 stream live state changes",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_list() {
        assert_eq!(parse(&args(&["list"])).expect("ok"), FreeAtHomeCommand::List);
    }

    #[test]
    fn parse_watch() {
        assert_eq!(parse(&args(&["watch"])).expect("ok"), FreeAtHomeCommand::Watch);
    }

    #[test]
    fn parse_get() {
        let c = parse(&args(&["get", "ABB700C12345", "ch0000", "odp0000"])).expect("ok");
        assert_eq!(
            c,
            FreeAtHomeCommand::Get {
                serial: "ABB700C12345".into(),
                channel: "ch0000".into(),
                datapoint: "odp0000".into(),
            }
        );
    }

    #[test]
    fn parse_set() {
        let c = parse(&args(&["set", "ABB700C12345", "ch0000", "idp0000", "1"])).expect("ok");
        assert_eq!(
            c,
            FreeAtHomeCommand::Set {
                serial: "ABB700C12345".into(),
                channel: "ch0000".into(),
                datapoint: "idp0000".into(),
                value: "1".into(),
            }
        );
    }

    #[test]
    fn get_missing_args_is_error() {
        assert!(matches!(
            parse(&args(&["get", "ABB700C12345"])),
            Err(CliError::MissingArgs(_))
        ));
    }

    #[test]
    fn unknown_subcommand_is_error() {
        assert!(matches!(
            parse(&args(&["frobnicate"])),
            Err(CliError::UnknownSubcommand(_))
        ));
    }

    #[test]
    fn empty_args_is_error() {
        assert!(matches!(parse(&[]), Err(CliError::MissingArgs(_))));
    }

    #[test]
    fn usage_mentions_all_subcommands() {
        let u = usage();
        for s in ["list", "get", "set", "watch"] {
            assert!(u.contains(s), "usage should mention {s}");
        }
    }
}
