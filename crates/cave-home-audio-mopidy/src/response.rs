//! MPD-protocol response formatting.
//!
//! An MPD reply is a sequence of `key: value` lines followed by a terminator:
//! `OK\n` on success, or `ACK [error@cmd_index] {command} message\n` on failure.
//! This module formats both — built from the *public protocol documentation*,
//! not from any GPL MPD server source.
//!
//! The [`AckError`] codes mirror MPD's published numeric error set so a stock
//! MPD client understands them; they are wire-level and never shown to the
//! end-user (the grandma-friendly layer lives in [`crate::label`]).

use core::fmt::Write as _;

/// MPD's published `ACK` error codes (subset this engine emits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckError {
    /// `ACK_ERROR_ARG` (2) — a malformed or out-of-range argument.
    Arg,
    /// `ACK_ERROR_NO_EXIST` (50) — referenced song/position does not exist.
    NoExist,
    /// `ACK_ERROR_UNKNOWN` (5) — unknown command.
    UnknownCommand,
}

impl AckError {
    /// The numeric code carried in the `ACK [code@index]` prefix.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::Arg => 2,
            Self::UnknownCommand => 5,
            Self::NoExist => 50,
        }
    }
}

/// A `key: value` response line builder.
///
/// Accumulates lines and emits them with the chosen terminator. Values are
/// written verbatim (MPD values are not quoted on the response side).
#[derive(Debug, Default)]
pub struct Response {
    body: String,
}

impl Response {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a `key: value` line. Chainable.
    pub fn field(&mut self, key: &str, value: impl core::fmt::Display) -> &mut Self {
        // Writing to a String is infallible; ignore the formatter Result.
        let _ = writeln!(self.body, "{key}: {value}");
        self
    }

    /// Finish with a success `OK` terminator.
    #[must_use]
    pub fn ok(&self) -> String {
        format!("{}OK\n", self.body)
    }

    /// Finish with an `ACK` error terminator.
    ///
    /// `cmd_index` is the 0-based position of the failing command within a
    /// command list (0 for a standalone command); `command` is the verb that
    /// failed; `message` is a short human-readable cause.
    #[must_use]
    pub fn ack(&self, err: AckError, cmd_index: usize, command: &str, message: &str) -> String {
        format!(
            "{}ACK [{}@{}] {{{}}} {}\n",
            self.body,
            err.code(),
            cmd_index,
            command,
            message
        )
    }
}

/// A bare success reply with no fields (`OK\n`).
#[must_use]
pub fn ok() -> String {
    "OK\n".to_owned()
}

/// A bare error reply with no preceding fields.
#[must_use]
pub fn ack(err: AckError, cmd_index: usize, command: &str, message: &str) -> String {
    Response::new().ack(err, cmd_index, command, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_ok_is_terminator_only() {
        assert_eq!(ok(), "OK\n");
    }

    #[test]
    fn fields_then_ok() {
        let mut r = Response::new();
        r.field("volume", 75).field("state", "play");
        assert_eq!(r.ok(), "volume: 75\nstate: play\nOK\n");
    }

    #[test]
    fn ack_carries_code_index_command_and_message() {
        let r = ack(AckError::NoExist, 0, "play", "No such song");
        assert_eq!(r, "ACK [50@0] {play} No such song\n");
    }

    #[test]
    fn ack_codes_match_the_published_set() {
        assert_eq!(AckError::Arg.code(), 2);
        assert_eq!(AckError::UnknownCommand.code(), 5);
        assert_eq!(AckError::NoExist.code(), 50);
    }

    #[test]
    fn ack_after_fields_includes_the_fields() {
        let mut r = Response::new();
        r.field("partition", "default");
        let out = r.ack(AckError::Arg, 1, "setvol", "Integer expected");
        assert_eq!(out, "partition: default\nACK [2@1] {setvol} Integer expected\n");
    }
}
