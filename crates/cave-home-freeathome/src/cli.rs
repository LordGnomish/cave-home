// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `cave-home-ctl freeathome` subcommand parsing.

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
