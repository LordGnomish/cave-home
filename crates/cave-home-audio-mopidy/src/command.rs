//! MPD-protocol command parsing.
//!
//! The MPD protocol (implemented here from the *public, documented* line-based
//! text protocol — no GPL MPD server source was read) is a sequence of
//! request lines. Each line is a command name followed by space-separated
//! arguments; an argument containing whitespace is wrapped in double quotes and
//! may escape `"` and `\` with a backslash.
//!
//! [`parse`] turns one request line into a typed [`Command`], or a
//! [`ParseError`] describing why the line could not be understood. Parsing never
//! panics and never allocates beyond the argument strings it returns.

use core::fmt;

/// A parsed client request.
///
/// Each variant carries exactly the arguments that command's MPD form takes.
/// Toggle commands (`random`, `repeat`, `single`, `consume`) carry the boolean
/// they request; numeric commands carry validated-shape (but not yet
/// range-checked) numbers — range validation belongs to the engine that owns
/// the queue and volume.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `play [POS]` — start playback, optionally at a queue position.
    Play(Option<usize>),
    /// `playid [ID]` — start playback at a stable song id.
    PlayId(Option<u64>),
    /// `pause [0|1]` — toggle when no arg, else set paused-state explicitly.
    Pause(Option<bool>),
    Stop,
    Next,
    Previous,
    /// `setvol VOL` — set volume (range-checked by the engine).
    SetVol(i64),
    Status,
    CurrentSong,
    /// `add URI` — append a song to the queue.
    Add(String),
    /// `delete POS` — remove the song at a queue position.
    Delete(usize),
    /// `deleteid ID` — remove the song with a stable id.
    DeleteId(u64),
    /// `move FROM TO` — move a queued song from one position to another.
    Move { from: usize, to: usize },
    Clear,
    /// `playlistinfo [POS]` — list the whole queue, or one entry.
    PlaylistInfo(Option<usize>),
    /// `seek POS TIME` — seek within the song at a queue position (seconds).
    Seek { pos: usize, seconds: f64 },
    /// `seekcur TIME` — seek within the current song (seconds).
    SeekCur(f64),
    Random(bool),
    Repeat(bool),
    Single(bool),
    Consume(bool),
}

/// Why a request line could not be parsed into a [`Command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The line was empty or only whitespace.
    Empty,
    /// The command name is not one this engine understands.
    UnknownCommand(String),
    /// The command needs more arguments than were supplied.
    MissingArgument(&'static str),
    /// An argument was present but not of the expected shape.
    BadArgument { arg: &'static str, value: String },
    /// A quoted argument was never closed.
    UnterminatedQuote,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty command line"),
            Self::UnknownCommand(c) => write!(f, "unknown command: {c}"),
            Self::MissingArgument(a) => write!(f, "missing argument: {a}"),
            Self::BadArgument { arg, value } => {
                write!(f, "bad value for {arg}: {value}")
            }
            Self::UnterminatedQuote => f.write_str("unterminated quoted argument"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Split a request line into tokens following MPD's quoting rules.
///
/// Whitespace separates tokens; a double-quoted token may contain spaces and
/// uses `\` to escape `"` and `\`. Returns [`ParseError::UnterminatedQuote`] if
/// a quote is opened and never closed.
fn tokenize(line: &str) -> Result<Vec<String>, ParseError> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        // Skip leading whitespace between tokens.
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            let _ = chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        let mut token = String::new();
        if first == '"' {
            let _ = chars.next(); // consume opening quote
            let mut closed = false;
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        // Escape the next char verbatim; a trailing backslash is
                        // treated as a literal backslash.
                        if let Some(next) = chars.next() {
                            token.push(next);
                        } else {
                            token.push('\\');
                        }
                    }
                    '"' => {
                        closed = true;
                        break;
                    }
                    other => token.push(other),
                }
            }
            if !closed {
                return Err(ParseError::UnterminatedQuote);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                token.push(c);
                let _ = chars.next();
            }
        }
        tokens.push(token);
    }
    Ok(tokens)
}

fn parse_bool(arg: &'static str, raw: &str) -> Result<bool, ParseError> {
    match raw {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(ParseError::BadArgument {
            arg,
            value: other.to_owned(),
        }),
    }
}

fn parse_usize(arg: &'static str, raw: &str) -> Result<usize, ParseError> {
    raw.parse::<usize>().map_err(|_| ParseError::BadArgument {
        arg,
        value: raw.to_owned(),
    })
}

fn parse_u64(arg: &'static str, raw: &str) -> Result<u64, ParseError> {
    raw.parse::<u64>().map_err(|_| ParseError::BadArgument {
        arg,
        value: raw.to_owned(),
    })
}

fn parse_i64(arg: &'static str, raw: &str) -> Result<i64, ParseError> {
    raw.parse::<i64>().map_err(|_| ParseError::BadArgument {
        arg,
        value: raw.to_owned(),
    })
}

fn parse_seconds(arg: &'static str, raw: &str) -> Result<f64, ParseError> {
    let value = raw.parse::<f64>().map_err(|_| ParseError::BadArgument {
        arg,
        value: raw.to_owned(),
    })?;
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(ParseError::BadArgument {
            arg,
            value: raw.to_owned(),
        })
    }
}

/// Parse a single MPD request line into a [`Command`].
///
/// # Errors
/// Returns a [`ParseError`] for empty lines, unknown commands, missing or
/// malformed arguments, and unterminated quotes. Range-checking of values
/// (e.g. that a volume is 0..=100) is the engine's job, not the parser's.
pub fn parse(line: &str) -> Result<Command, ParseError> {
    let tokens = tokenize(line)?;
    let Some((name, args)) = tokens.split_first() else {
        return Err(ParseError::Empty);
    };
    let opt_first = || args.first().map(String::as_str);
    match name.as_str() {
        "play" => match opt_first() {
            None => Ok(Command::Play(None)),
            Some(p) => Ok(Command::Play(Some(parse_usize("position", p)?))),
        },
        "playid" => match opt_first() {
            None => Ok(Command::PlayId(None)),
            Some(p) => Ok(Command::PlayId(Some(parse_u64("songid", p)?))),
        },
        "pause" => match opt_first() {
            None => Ok(Command::Pause(None)),
            Some(b) => Ok(Command::Pause(Some(parse_bool("pause", b)?))),
        },
        "stop" => Ok(Command::Stop),
        "next" => Ok(Command::Next),
        "previous" => Ok(Command::Previous),
        "setvol" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("volume"))?;
            Ok(Command::SetVol(parse_i64("volume", raw)?))
        }
        "status" => Ok(Command::Status),
        "currentsong" => Ok(Command::CurrentSong),
        "add" => {
            let uri = opt_first().ok_or(ParseError::MissingArgument("song"))?;
            Ok(Command::Add(uri.to_owned()))
        }
        "delete" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("position"))?;
            Ok(Command::Delete(parse_usize("position", raw)?))
        }
        "deleteid" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("songid"))?;
            Ok(Command::DeleteId(parse_u64("songid", raw)?))
        }
        "move" => {
            let from = args.first().ok_or(ParseError::MissingArgument("from"))?;
            let to = args.get(1).ok_or(ParseError::MissingArgument("to"))?;
            Ok(Command::Move {
                from: parse_usize("from", from)?,
                to: parse_usize("to", to)?,
            })
        }
        "clear" => Ok(Command::Clear),
        "playlistinfo" => match opt_first() {
            None => Ok(Command::PlaylistInfo(None)),
            Some(p) => Ok(Command::PlaylistInfo(Some(parse_usize("position", p)?))),
        },
        "seek" => {
            let pos = args.first().ok_or(ParseError::MissingArgument("position"))?;
            let time = args.get(1).ok_or(ParseError::MissingArgument("time"))?;
            Ok(Command::Seek {
                pos: parse_usize("position", pos)?,
                seconds: parse_seconds("time", time)?,
            })
        }
        "seekcur" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("time"))?;
            Ok(Command::SeekCur(parse_seconds("time", raw)?))
        }
        "random" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("random"))?;
            Ok(Command::Random(parse_bool("random", raw)?))
        }
        "repeat" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("repeat"))?;
            Ok(Command::Repeat(parse_bool("repeat", raw)?))
        }
        "single" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("single"))?;
            Ok(Command::Single(parse_bool("single", raw)?))
        }
        "consume" => {
            let raw = opt_first().ok_or(ParseError::MissingArgument("consume"))?;
            Ok(Command::Consume(parse_bool("consume", raw)?))
        }
        other => Err(ParseError::UnknownCommand(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_verbs() {
        assert_eq!(parse("stop"), Ok(Command::Stop));
        assert_eq!(parse("next"), Ok(Command::Next));
        assert_eq!(parse("previous"), Ok(Command::Previous));
        assert_eq!(parse("clear"), Ok(Command::Clear));
        assert_eq!(parse("status"), Ok(Command::Status));
        assert_eq!(parse("currentsong"), Ok(Command::CurrentSong));
    }

    #[test]
    fn parses_optional_and_required_numeric_args() {
        assert_eq!(parse("play"), Ok(Command::Play(None)));
        assert_eq!(parse("play 3"), Ok(Command::Play(Some(3))));
        assert_eq!(parse("setvol 75"), Ok(Command::SetVol(75)));
        assert_eq!(parse("delete 2"), Ok(Command::Delete(2)));
        assert_eq!(parse("deleteid 42"), Ok(Command::DeleteId(42)));
        assert_eq!(parse("playid 7"), Ok(Command::PlayId(Some(7))));
    }

    #[test]
    fn parses_pause_toggle_and_explicit() {
        assert_eq!(parse("pause"), Ok(Command::Pause(None)));
        assert_eq!(parse("pause 1"), Ok(Command::Pause(Some(true))));
        assert_eq!(parse("pause 0"), Ok(Command::Pause(Some(false))));
    }

    #[test]
    fn parses_move_and_seek() {
        assert_eq!(parse("move 0 3"), Ok(Command::Move { from: 0, to: 3 }));
        assert_eq!(parse("seek 1 30"), Ok(Command::Seek { pos: 1, seconds: 30.0 }));
        assert_eq!(parse("seekcur 12.5"), Ok(Command::SeekCur(12.5)));
    }

    #[test]
    fn parses_toggle_modes() {
        assert_eq!(parse("random 1"), Ok(Command::Random(true)));
        assert_eq!(parse("repeat 0"), Ok(Command::Repeat(false)));
        assert_eq!(parse("single 1"), Ok(Command::Single(true)));
        assert_eq!(parse("consume 1"), Ok(Command::Consume(true)));
    }

    #[test]
    fn add_takes_a_quoted_uri_with_spaces() {
        assert_eq!(
            parse("add \"local:track:My Song.mp3\""),
            Ok(Command::Add("local:track:My Song.mp3".to_owned()))
        );
    }

    #[test]
    fn quoted_argument_honours_escapes() {
        // A backslash escapes the following quote/backslash.
        assert_eq!(
            parse("add \"a \\\"quoted\\\" name\""),
            Ok(Command::Add("a \"quoted\" name".to_owned()))
        );
    }

    #[test]
    fn unterminated_quote_is_an_error_not_a_panic() {
        assert_eq!(parse("add \"open ended"), Err(ParseError::UnterminatedQuote));
    }

    #[test]
    fn empty_line_is_empty_error() {
        assert_eq!(parse(""), Err(ParseError::Empty));
        assert_eq!(parse("    "), Err(ParseError::Empty));
    }

    #[test]
    fn unknown_command_is_reported_with_its_name() {
        assert_eq!(
            parse("frobnicate 1"),
            Err(ParseError::UnknownCommand("frobnicate".to_owned()))
        );
    }

    #[test]
    fn missing_required_argument_is_an_error() {
        assert_eq!(parse("setvol"), Err(ParseError::MissingArgument("volume")));
        assert_eq!(parse("add"), Err(ParseError::MissingArgument("song")));
        assert_eq!(parse("move 0"), Err(ParseError::MissingArgument("to")));
    }

    #[test]
    fn malformed_numeric_argument_is_bad_argument() {
        assert_eq!(
            parse("setvol loud"),
            Err(ParseError::BadArgument {
                arg: "volume",
                value: "loud".to_owned()
            })
        );
        assert_eq!(
            parse("delete -1"),
            Err(ParseError::BadArgument {
                arg: "position",
                value: "-1".to_owned()
            })
        );
    }

    #[test]
    fn negative_or_nonfinite_seek_is_rejected() {
        assert!(matches!(parse("seekcur -5"), Err(ParseError::BadArgument { .. })));
        assert!(matches!(parse("seekcur nan"), Err(ParseError::BadArgument { .. })));
    }

    #[test]
    fn leading_and_repeated_whitespace_is_tolerated() {
        assert_eq!(parse("  play   4 "), Ok(Command::Play(Some(4))));
    }
}
