// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-ctl freeathome` subcommand parsing.
//!
//! Hand-rolled to match `cave-home-binary`'s parser style (no clap). The binary
//! strips the leading `freeathome` token and hands the remainder to [`parse`];
//! the async client (see [`crate::client`]) executes the resulting command.
//! Keeping parsing pure makes the CLI surface fully unit-testable without a SysAP.

use std::fmt;

use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial};

use crate::error::{FreeAtHomeError, Result};
use crate::rest::RestRequest;

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

/// Map a parsed command to a single REST request, if it is one.
///
/// `list` and `watch` are not single REST calls, so they map to `None`. `get`
/// and `set` parse their string ids into typed free@home ids (erroring on
/// malformed input).
pub fn to_rest_request(command: &FreeAtHomeCommand) -> Result<Option<RestRequest>> {
    let parse_ids = |serial: &str, channel: &str, datapoint: &str| -> Result<_> {
        let serial = DeviceSerial::parse(serial)
            .map_err(|e| FreeAtHomeError::Domain(e.to_string()))?;
        let channel =
            ChannelId::parse(channel).map_err(|e| FreeAtHomeError::Domain(e.to_string()))?;
        let datapoint =
            DatapointId::parse(datapoint).map_err(|e| FreeAtHomeError::Domain(e.to_string()))?;
        Ok((serial, channel, datapoint))
    };
    match command {
        FreeAtHomeCommand::List | FreeAtHomeCommand::Watch => Ok(None),
        FreeAtHomeCommand::Get {
            serial,
            channel,
            datapoint,
        } => {
            let (s, c, d) = parse_ids(serial, channel, datapoint)?;
            Ok(Some(RestRequest::get_datapoint(s, c, d)))
        }
        FreeAtHomeCommand::Set {
            serial,
            channel,
            datapoint,
            value,
        } => {
            let (s, c, d) = parse_ids(serial, channel, datapoint)?;
            Ok(Some(RestRequest::set_datapoint(s, c, d, value.clone())))
        }
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

    #[test]
    fn get_maps_to_rest_request() {
        let cmd = parse(&args(&["get", "ABB700C12345", "ch0000", "odp0000"])).expect("ok");
        let req = to_rest_request(&cmd).expect("ok").expect("some");
        assert_eq!(req.path(), "datapoint/ABB700C12345/ch0000/odp0000");
    }

    #[test]
    fn set_maps_to_rest_request_with_body() {
        let cmd = parse(&args(&["set", "ABB700C12345", "ch0000", "idp0000", "1"])).expect("ok");
        let req = to_rest_request(&cmd).expect("ok").expect("some");
        assert_eq!(req.method(), crate::rest::HttpMethod::Put);
        assert_eq!(req.body(), Some("1"));
    }

    #[test]
    fn list_and_watch_map_to_no_single_request() {
        let list = parse(&args(&["list"])).expect("ok");
        assert!(to_rest_request(&list).expect("ok").is_none());
        let watch = parse(&args(&["watch"])).expect("ok");
        assert!(to_rest_request(&watch).expect("ok").is_none());
    }

    #[test]
    fn invalid_serial_is_error() {
        let cmd = FreeAtHomeCommand::Get {
            serial: "!!".into(),
            channel: "ch0000".into(),
            datapoint: "odp0000".into(),
        };
        assert!(to_rest_request(&cmd).is_err());
    }
}
