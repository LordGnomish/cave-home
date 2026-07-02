// SPDX-License-Identifier: Apache-2.0
//! Hand-rolled argument parsing into a typed [`Command`].
//!
//! No `clap` (std-only). `argv` (the program name already stripped) is parsed
//! into one of the supported subcommands plus its options. Help text is written
//! in plain, grandma-friendly language (Charter §6.3): it talks about *your
//! home*, not about pods, brokers, or manifests.

use crate::config::{ConfigLayer, Layer, LogLevel, NodeRole};
use std::fmt;

/// A fully-parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Start the home (the long-running process). Carries the CLI-flag config
    /// layer so [`crate::config`] can merge it on top of file + env.
    Run { flags: Box<ConfigLayer> },
    /// Boot the cluster runtime in a K3s-style role (`cmd/k3s server|agent|etcd`).
    /// Carries the same CLI-flag config layer as [`Run`](Self::Run).
    Serve { role: ServeRole, flags: Box<ConfigLayer> },
    /// Show a short summary of how the home is doing.
    Status,
    /// Check that the configuration is valid without starting anything.
    ConfigCheck,
    /// Print the resolved configuration.
    ConfigShow,
    /// Join this node to an existing home (carries the invite the wizard gives).
    NodeJoin { invite: String },
    /// List the nodes that make up this home.
    NodeList,
    /// Make a backup of the home's settings and history.
    Backup { dest: Option<String> },
    /// Restore the home from a backup.
    Restore { src: String },
    /// Print version and build information.
    Version,
    /// Print help. `topic` is `None` for the top-level help.
    Help { topic: Option<String> },
}

/// The K3s-style cluster role a [`Command::Serve`] boots in (`cmd/k3s` has the
/// matching `server`, `agent` and `etcd` subcommands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeRole {
    /// A control-plane node: hosts the apiserver and every controller.
    Server,
    /// A worker node: runs the node-side components, joins a remote server.
    Agent,
    /// A dedicated datastore (etcd/kine) member.
    Etcd,
}

impl ServeRole {
    /// The lowercase subcommand token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Agent => "agent",
            Self::Etcd => "etcd",
        }
    }
}

/// Why a command line could not be parsed. No panics — every bad input maps
/// to one of these, with a friendly message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// No subcommand was given.
    NoCommand,
    /// The first word is not a command we know.
    UnknownCommand(String),
    /// An option needs a value but none followed it (e.g. `--name` at the end).
    MissingValue(String),
    /// A required positional argument was absent (e.g. `restore` with no file).
    MissingArgument(&'static str),
    /// An option's value could not be understood (bad port, role, level).
    BadValue { flag: String, value: String },
    /// An unexpected flag for this subcommand.
    UnknownFlag(String),
    /// More positional arguments than the command accepts.
    UnexpectedArgument(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCommand => write!(
                f,
                "please tell cave-home what to do — try `cave-home help` to see the options"
            ),
            Self::UnknownCommand(c) => {
                write!(f, "`{c}` is not something cave-home knows — try `cave-home help`")
            }
            Self::MissingValue(flag) => write!(f, "`{flag}` needs a value after it"),
            Self::MissingArgument(what) => write!(f, "this command needs you to say which {what}"),
            Self::BadValue { flag, value } => {
                write!(f, "`{value}` is not a valid value for `{flag}`")
            }
            Self::UnknownFlag(flag) => write!(f, "`{flag}` is not an option for this command"),
            Self::UnexpectedArgument(a) => write!(f, "did not expect `{a}` here"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse `argv` (program name already removed) into a [`Command`].
///
/// # Errors
/// Returns a [`ParseError`] describing the first problem found.
pub fn parse(argv: &[String]) -> Result<Command, ParseError> {
    let Some((head, rest)) = argv.split_first() else {
        return Err(ParseError::NoCommand);
    };
    // Top-level help flags map to the help command.
    if head == "-h" || head == "--help" {
        return Ok(Command::Help { topic: None });
    }
    if head == "-V" || head == "--version" {
        return Ok(Command::Version);
    }

    match head.as_str() {
        "run" => parse_run(rest),
        "server" | "serve" => parse_serve(ServeRole::Server, rest),
        "agent" => parse_serve(ServeRole::Agent, rest),
        "etcd" => parse_serve(ServeRole::Etcd, rest),
        "status" => no_args(rest, Command::Status),
        "version" => no_args(rest, Command::Version),
        "help" => Ok(Command::Help {
            topic: rest.first().cloned(),
        }),
        "config" => parse_config(rest),
        "node" => parse_node(rest),
        "backup" => parse_backup(rest),
        "restore" => parse_restore(rest),
        other => Err(ParseError::UnknownCommand(other.to_string())),
    }
}

/// A subcommand that takes no further arguments.
fn no_args(rest: &[String], cmd: Command) -> Result<Command, ParseError> {
    if let Some(extra) = rest.first() {
        // A trailing help flag is still help-friendly.
        if extra == "-h" || extra == "--help" {
            return Ok(Command::Help {
                topic: command_topic(&cmd),
            });
        }
        return Err(ParseError::UnexpectedArgument(extra.clone()));
    }
    Ok(cmd)
}

fn command_topic(cmd: &Command) -> Option<String> {
    let t = match cmd {
        Command::Status => "status",
        Command::Version => "version",
        Command::Run { .. } => "run",
        _ => return None,
    };
    Some(t.to_string())
}

fn parse_run(rest: &[String]) -> Result<Command, ParseError> {
    match parse_flags(rest, "run")? {
        FlagParse::Help(topic) => Ok(Command::Help { topic: Some(topic) }),
        FlagParse::Flags(flags) => Ok(Command::Run { flags: Box::new(flags) }),
    }
}

fn parse_serve(role: ServeRole, rest: &[String]) -> Result<Command, ParseError> {
    match parse_flags(rest, role.as_str())? {
        FlagParse::Help(topic) => Ok(Command::Help { topic: Some(topic) }),
        FlagParse::Flags(flags) => Ok(Command::Serve { role, flags: Box::new(flags) }),
    }
}

/// The outcome of parsing a flag list: either a help request or the merged
/// config layer.
enum FlagParse {
    Help(String),
    Flags(ConfigLayer),
}

/// Parse the shared `--name/--role/--data-dir/--bind/--port/--log-level` flags
/// used by both `run` and the K3s-style `server/agent/etcd` commands.
fn parse_flags(rest: &[String], help_topic: &str) -> Result<FlagParse, ParseError> {
    let mut flags = ConfigLayer::empty(Layer::Flags);
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Ok(FlagParse::Help(help_topic.to_string()));
            }
            "--name" => flags.node_name = Some(take(&mut it, arg)?),
            "--role" => {
                let v = take(&mut it, arg)?;
                let role = NodeRole::from_str_ci(&v).ok_or_else(|| ParseError::BadValue {
                    flag: arg.clone(),
                    value: v.clone(),
                })?;
                flags.role = Some(role);
            }
            "--data-dir" => flags.data_dir = Some(take(&mut it, arg)?),
            "--bind" => flags.bind_addr = Some(take(&mut it, arg)?),
            "--port" => {
                let v = take(&mut it, arg)?;
                let port = v.parse::<u16>().map_err(|_| ParseError::BadValue {
                    flag: arg.clone(),
                    value: v.clone(),
                })?;
                flags.bind_port = Some(port);
            }
            "--log-level" => {
                let v = take(&mut it, arg)?;
                let lvl = LogLevel::from_str_ci(&v).ok_or_else(|| ParseError::BadValue {
                    flag: arg.clone(),
                    value: v.clone(),
                })?;
                flags.log_level = Some(lvl);
            }
            other if other.starts_with('-') => {
                return Err(ParseError::UnknownFlag(other.to_string()));
            }
            other => return Err(ParseError::UnexpectedArgument(other.to_string())),
        }
    }
    Ok(FlagParse::Flags(flags))
}

fn parse_config(rest: &[String]) -> Result<Command, ParseError> {
    let Some((sub, tail)) = rest.split_first() else {
        return Err(ParseError::MissingArgument("config action (check or show)"));
    };
    match sub.as_str() {
        "check" => no_args(tail, Command::ConfigCheck),
        "show" => no_args(tail, Command::ConfigShow),
        "-h" | "--help" => Ok(Command::Help {
            topic: Some("config".to_string()),
        }),
        other => Err(ParseError::UnknownCommand(format!("config {other}"))),
    }
}

fn parse_node(rest: &[String]) -> Result<Command, ParseError> {
    let Some((sub, tail)) = rest.split_first() else {
        return Err(ParseError::MissingArgument("node action (join or list)"));
    };
    match sub.as_str() {
        "list" => no_args(tail, Command::NodeList),
        "join" => {
            let invite = tail
                .first()
                .cloned()
                .ok_or(ParseError::MissingArgument("invite code"))?;
            if let Some(extra) = tail.get(1) {
                return Err(ParseError::UnexpectedArgument(extra.clone()));
            }
            Ok(Command::NodeJoin { invite })
        }
        "-h" | "--help" => Ok(Command::Help {
            topic: Some("node".to_string()),
        }),
        other => Err(ParseError::UnknownCommand(format!("node {other}"))),
    }
}

fn parse_backup(rest: &[String]) -> Result<Command, ParseError> {
    if rest.first().is_some_and(|a| a == "-h" || a == "--help") {
        return Ok(Command::Help {
            topic: Some("backup".to_string()),
        });
    }
    let dest = rest.first().cloned();
    if let Some(extra) = rest.get(1) {
        return Err(ParseError::UnexpectedArgument(extra.clone()));
    }
    Ok(Command::Backup { dest })
}

fn parse_restore(rest: &[String]) -> Result<Command, ParseError> {
    if rest.first().is_some_and(|a| a == "-h" || a == "--help") {
        return Ok(Command::Help {
            topic: Some("restore".to_string()),
        });
    }
    let src = rest
        .first()
        .cloned()
        .ok_or(ParseError::MissingArgument("backup file"))?;
    if let Some(extra) = rest.get(1) {
        return Err(ParseError::UnexpectedArgument(extra.clone()));
    }
    Ok(Command::Restore { src })
}

/// Consume the next iterator item as the value for `flag`, or error.
fn take(it: &mut std::slice::Iter<'_, String>, flag: &str) -> Result<String, ParseError> {
    it.next()
        .cloned()
        .ok_or_else(|| ParseError::MissingValue(flag.to_string()))
}

/// Grandma-friendly help text. `topic` selects a per-command page; `None` is
/// the top-level overview. Pure: returns the string, never prints.
#[must_use]
pub fn help_text(topic: Option<&str>) -> String {
    match topic {
        Some("run") => "\
cave-home run — start your home

  This keeps your home running: lights, automations, cameras, and the
  dashboard. Leave it running.

  Options:
    --name <name>        a friendly name for this home (letters, numbers, dashes)
    --role <role>        hub, secondary, or ml (the kind of node this is)
    --data-dir <path>    where to keep your home's data
    --bind <address>     the network address to listen on
    --port <number>      the network port to listen on
    --log-level <level>  how much detail to record (error, warn, info, debug, trace)
"
        .to_string(),
        Some("status") => "\
cave-home status — see how your home is doing

  Shows a short, plain summary: everything fine, something needs a look,
  or a part is down.
"
        .to_string(),
        Some("config") => "\
cave-home config — work with your home's settings

  config check   make sure the settings are valid (changes nothing)
  config show    print the settings the way your home actually sees them
"
        .to_string(),
        Some("node") => "\
cave-home node — work with the parts of your home

  node list           show the parts that make up your home
  node join <invite>  add this part to an existing home using an invite
"
        .to_string(),
        Some("backup") => "\
cave-home backup [folder] — save a copy of your home's settings and history

  If you do not say where, cave-home picks a sensible place for you.
"
        .to_string(),
        Some("restore") => "\
cave-home restore <file> — put your home back from a saved copy
"
        .to_string(),
        Some("version") => "\
cave-home version — show which build of cave-home you are running
"
        .to_string(),
        _ => "\
cave-home — your home, in one place

  Usage: cave-home <what-to-do> [options]

  What you can do:
    run             start your home and keep it running
    status          see how your home is doing
    config check    make sure your settings are valid
    config show     print your settings
    node list       show the parts that make up your home
    node join       add this part to an existing home
    backup          save a copy of your settings and history
    restore         put your home back from a saved copy
    version         show which build you are running
    help <topic>    explain one of the commands above

  Everything runs as one program on this node — one thing to install,
  one thing to update.
"
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn empty_argv_is_no_command() {
        assert_eq!(parse(&[]).unwrap_err(), ParseError::NoCommand);
    }

    #[test]
    fn run_with_no_flags_parses() {
        let cmd = parse(&argv(&["run"])).expect("run parses");
        match cmd {
            Command::Run { flags } => assert_eq!(*flags, ConfigLayer::empty(Layer::Flags)),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn server_boots_the_control_plane_role() {
        assert!(matches!(
            parse(&argv(&["server"])).unwrap(),
            Command::Serve { role: ServeRole::Server, .. }
        ));
    }

    #[test]
    fn serve_is_an_alias_for_server() {
        assert!(matches!(
            parse(&argv(&["serve"])).unwrap(),
            Command::Serve { role: ServeRole::Server, .. }
        ));
    }

    #[test]
    fn agent_and_etcd_roles_parse() {
        assert!(matches!(
            parse(&argv(&["agent"])).unwrap(),
            Command::Serve { role: ServeRole::Agent, .. }
        ));
        assert!(matches!(
            parse(&argv(&["etcd"])).unwrap(),
            Command::Serve { role: ServeRole::Etcd, .. }
        ));
    }

    #[test]
    fn server_accepts_the_shared_flags() {
        let cmd = parse(&argv(&["server", "--port", "6443", "--bind", "0.0.0.0"])).expect("parses");
        let Command::Serve { role, flags } = cmd else {
            panic!("expected Serve");
        };
        assert_eq!(role, ServeRole::Server);
        assert_eq!(flags.bind_port, Some(6443));
        assert_eq!(flags.bind_addr.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn server_bad_port_errors() {
        assert!(parse(&argv(&["server", "--port", "not-a-port"])).is_err());
    }

    #[test]
    fn run_parses_all_flags() {
        let cmd = parse(&argv(&[
            "run",
            "--name",
            "home1",
            "--role",
            "secondary",
            "--data-dir",
            "/srv/ch",
            "--bind",
            "127.0.0.1",
            "--port",
            "9001",
            "--log-level",
            "debug",
        ]))
        .expect("parses");
        let Command::Run { flags } = cmd else {
            panic!("expected run");
        };
        assert_eq!(flags.node_name.as_deref(), Some("home1"));
        assert_eq!(flags.role, Some(NodeRole::Secondary));
        assert_eq!(flags.data_dir.as_deref(), Some("/srv/ch"));
        assert_eq!(flags.bind_addr.as_deref(), Some("127.0.0.1"));
        assert_eq!(flags.bind_port, Some(9001));
        assert_eq!(flags.log_level, Some(LogLevel::Debug));
    }

    #[test]
    fn run_missing_flag_value_errors() {
        let err = parse(&argv(&["run", "--name"])).unwrap_err();
        assert_eq!(err, ParseError::MissingValue("--name".to_string()));
    }

    #[test]
    fn run_bad_port_errors() {
        let err = parse(&argv(&["run", "--port", "notaport"])).unwrap_err();
        assert_eq!(
            err,
            ParseError::BadValue {
                flag: "--port".to_string(),
                value: "notaport".to_string()
            }
        );
    }

    #[test]
    fn run_bad_role_errors() {
        let err = parse(&argv(&["run", "--role", "worker"])).unwrap_err();
        assert!(matches!(err, ParseError::BadValue { .. }));
    }

    #[test]
    fn run_unknown_flag_errors() {
        let err = parse(&argv(&["run", "--turbo"])).unwrap_err();
        assert_eq!(err, ParseError::UnknownFlag("--turbo".to_string()));
    }

    #[test]
    fn status_and_version_parse() {
        assert_eq!(parse(&argv(&["status"])).unwrap(), Command::Status);
        assert_eq!(parse(&argv(&["version"])).unwrap(), Command::Version);
    }

    #[test]
    fn status_rejects_extra_argument() {
        let err = parse(&argv(&["status", "now"])).unwrap_err();
        assert_eq!(err, ParseError::UnexpectedArgument("now".to_string()));
    }

    #[test]
    fn config_check_and_show_parse() {
        assert_eq!(
            parse(&argv(&["config", "check"])).unwrap(),
            Command::ConfigCheck
        );
        assert_eq!(
            parse(&argv(&["config", "show"])).unwrap(),
            Command::ConfigShow
        );
    }

    #[test]
    fn config_without_subcommand_errors() {
        assert!(matches!(
            parse(&argv(&["config"])).unwrap_err(),
            ParseError::MissingArgument(_)
        ));
    }

    #[test]
    fn config_unknown_subcommand_errors() {
        assert!(matches!(
            parse(&argv(&["config", "edit"])).unwrap_err(),
            ParseError::UnknownCommand(_)
        ));
    }

    #[test]
    fn node_list_and_join_parse() {
        assert_eq!(parse(&argv(&["node", "list"])).unwrap(), Command::NodeList);
        assert_eq!(
            parse(&argv(&["node", "join", "INVITE-123"])).unwrap(),
            Command::NodeJoin {
                invite: "INVITE-123".to_string()
            }
        );
    }

    #[test]
    fn node_join_without_invite_errors() {
        assert!(matches!(
            parse(&argv(&["node", "join"])).unwrap_err(),
            ParseError::MissingArgument(_)
        ));
    }

    #[test]
    fn node_join_extra_argument_errors() {
        let err = parse(&argv(&["node", "join", "a", "b"])).unwrap_err();
        assert_eq!(err, ParseError::UnexpectedArgument("b".to_string()));
    }

    #[test]
    fn backup_with_and_without_dest() {
        assert_eq!(parse(&argv(&["backup"])).unwrap(), Command::Backup { dest: None });
        assert_eq!(
            parse(&argv(&["backup", "/tmp/b"])).unwrap(),
            Command::Backup {
                dest: Some("/tmp/b".to_string())
            }
        );
    }

    #[test]
    fn restore_requires_source() {
        assert!(matches!(
            parse(&argv(&["restore"])).unwrap_err(),
            ParseError::MissingArgument(_)
        ));
        assert_eq!(
            parse(&argv(&["restore", "/tmp/b"])).unwrap(),
            Command::Restore {
                src: "/tmp/b".to_string()
            }
        );
    }

    #[test]
    fn unknown_command_errors() {
        let err = parse(&argv(&["frobnicate"])).unwrap_err();
        assert_eq!(err, ParseError::UnknownCommand("frobnicate".to_string()));
    }

    #[test]
    fn top_level_help_and_version_flags() {
        assert_eq!(
            parse(&argv(&["--help"])).unwrap(),
            Command::Help { topic: None }
        );
        assert_eq!(parse(&argv(&["-h"])).unwrap(), Command::Help { topic: None });
        assert_eq!(parse(&argv(&["--version"])).unwrap(), Command::Version);
        assert_eq!(parse(&argv(&["-V"])).unwrap(), Command::Version);
    }

    #[test]
    fn help_with_topic_parses() {
        assert_eq!(
            parse(&argv(&["help", "run"])).unwrap(),
            Command::Help {
                topic: Some("run".to_string())
            }
        );
        assert_eq!(parse(&argv(&["help"])).unwrap(), Command::Help { topic: None });
    }

    #[test]
    fn per_command_help_flag_routes_to_help() {
        assert_eq!(
            parse(&argv(&["run", "--help"])).unwrap(),
            Command::Help {
                topic: Some("run".to_string())
            }
        );
    }

    #[test]
    fn help_text_is_grandma_friendly() {
        // Top-level and every topic must avoid implementation jargon (§6.3).
        let banned = [
            "pod", "kubelet", "etcd", "namespace", "rbac", "mqtt", "k3s",
            "container", "manifest", "yaml",
        ];
        let topics = [
            None,
            Some("run"),
            Some("status"),
            Some("config"),
            Some("node"),
            Some("backup"),
            Some("restore"),
            Some("version"),
        ];
        for t in topics {
            let text = help_text(t).to_ascii_lowercase();
            for b in banned {
                assert!(!text.contains(b), "help topic {t:?} leaks jargon `{b}`");
            }
        }
    }

    #[test]
    fn unknown_help_topic_falls_back_to_overview() {
        let overview = help_text(None);
        assert_eq!(help_text(Some("nonsense")), overview);
    }
}
