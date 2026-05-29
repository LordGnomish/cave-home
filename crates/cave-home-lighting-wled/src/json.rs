//! A tiny, std-only JSON value model + parser + serializer.
//!
//! The WLED JSON API is small and well-shaped, and cave-home is a single-binary
//! project that avoids pulling external crates where std suffices (Charter §5).
//! Rather than depend on `serde_json`, this module hand-rolls just enough JSON
//! to round-trip the documented WLED `state` object: objects, arrays, strings,
//! integers, and booleans. It is deliberately minimal — no floats, no scientific
//! notation — because the WLED state shape only needs those types.
//!
//! Every entry point is total: malformed input yields an `Err`, never a panic.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// A whole number. WLED state values are all small integers.
    Int(i64),
    /// A UTF-8 string (no surrogate-pair escapes — not needed by WLED state).
    Str(String),
    /// An ordered array.
    Arr(Vec<Self>),
    /// An object. `BTreeMap` keeps key order deterministic for stable output.
    Obj(BTreeMap<String, Self>),
}

impl Json {
    /// Borrow a child of an object by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Obj(m) => m.get(key),
            _ => None,
        }
    }

    /// Read this value as an integer, if it is one.
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Read this value as a bool, if it is one.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Read this value as a slice of array elements, if it is an array.
    #[must_use]
    pub fn as_arr(&self) -> Option<&[Self]> {
        match self {
            Self::Arr(a) => Some(a),
            _ => None,
        }
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Int(i) => {
                // write! to a String never fails; ignore the Result without unwrap.
                let _ = write!(out, "{i}");
            }
            Self::Str(s) => write_escaped(s, out),
            Self::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Self::Obj(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    /// Parse a JSON document. Trailing non-whitespace is an error.
    ///
    /// # Errors
    /// Returns a human-readable message describing the first parse failure.
    pub fn parse(input: &str) -> Result<Self, String> {
        let bytes = input.as_bytes();
        let mut p = Parser { bytes, pos: 0 };
        p.skip_ws();
        let v = p.value()?;
        p.skip_ws();
        if p.pos != bytes.len() {
            return Err(format!("trailing data at byte {}", p.pos));
        }
        Ok(v)
    }
}

impl std::fmt::Display for Json {
    /// Serialize to a compact JSON string (also gives `.to_string()`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        self.write(&mut out);
        f.write_str(&out)
    }
}

fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while let Some(&b) = self.bytes.get(self.pos) {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't' | b'f') => self.boolean(),
            Some(b'n') => self.null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected byte {:?} at {}", c as char, self.pos)),
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at byte {}", b as char, self.pos))
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            let val = self.value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(map));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(arr));
        }
        loop {
            let val = self.value()?;
            arr.push(val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(arr));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".to_string()),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(s);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b'r') => s.push('\r'),
                        Some(b't') => s.push('\t'),
                        Some(b'b') => s.push('\u{0008}'),
                        Some(b'f') => s.push('\u{000C}'),
                        Some(b'u') => {
                            let cp = self.unicode_escape()?;
                            match char::from_u32(cp) {
                                Some(c) => s.push(c),
                                None => return Err("invalid unicode escape".to_string()),
                            }
                            continue;
                        }
                        _ => return Err(format!("bad escape at byte {}", self.pos)),
                    }
                    self.pos += 1;
                }
                Some(_) => {
                    // Copy one full UTF-8 char from the source.
                    let start = self.pos;
                    let rest = &self.bytes[start..];
                    let len = utf8_len(rest[0]);
                    let end = start + len;
                    if end > self.bytes.len() {
                        return Err("truncated UTF-8 in string".to_string());
                    }
                    match std::str::from_utf8(&self.bytes[start..end]) {
                        Ok(chunk) => s.push_str(chunk),
                        Err(_) => return Err("invalid UTF-8 in string".to_string()),
                    }
                    self.pos = end;
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<u32, String> {
        // self.pos is at 'u'.
        self.pos += 1;
        let end = self.pos + 4;
        if end > self.bytes.len() {
            return Err("short \\u escape".to_string());
        }
        let Ok(hex) = std::str::from_utf8(&self.bytes[self.pos..end]) else {
            return Err("bad \\u escape".to_string());
        };
        let cp = u32::from_str_radix(hex, 16).map_err(|_| "bad \\u hex".to_string())?;
        self.pos = end;
        Ok(cp)
    }

    fn boolean(&mut self) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(Json::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(Json::Bool(false))
        } else {
            Err(format!("invalid literal at byte {}", self.pos))
        }
    }

    fn null(&mut self) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(Json::Null)
        } else {
            Err(format!("invalid literal at byte {}", self.pos))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut saw_digit = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
                saw_digit = true;
            } else {
                break;
            }
        }
        if !saw_digit {
            return Err(format!("malformed number at byte {start}"));
        }
        let Ok(slice) = std::str::from_utf8(&self.bytes[start..self.pos]) else {
            return Err("non-utf8 number".to_string());
        };
        slice
            .parse::<i64>()
            .map(Json::Int)
            .map_err(|_| format!("number out of range at byte {start}"))
    }
}

/// Length in bytes of the UTF-8 sequence whose lead byte is `b`.
const fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    // Tests legitimately use expect/unwrap on known-good inputs and the
    // `let mut s = Default; s.field = ..` setup shape; these patterns are fine
    // in test scaffolding even though clippy::pedantic flags them in shipped code.
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::field_reassign_with_default,
        clippy::uninlined_format_args,
        clippy::float_cmp
    )]
    use super::*;

    #[test]
    fn parses_scalars() {
        assert_eq!(Json::parse("null"), Ok(Json::Null));
        assert_eq!(Json::parse("true"), Ok(Json::Bool(true)));
        assert_eq!(Json::parse("false"), Ok(Json::Bool(false)));
        assert_eq!(Json::parse("42"), Ok(Json::Int(42)));
        assert_eq!(Json::parse("-7"), Ok(Json::Int(-7)));
        assert_eq!(Json::parse("\"hi\""), Ok(Json::Str("hi".to_string())));
    }

    #[test]
    fn parses_nested_and_whitespace() {
        let v = Json::parse(" { \"on\" : true , \"seg\" : [ 1 , 2 ] } ").expect("parse");
        assert_eq!(v.get("on").and_then(Json::as_bool), Some(true));
        assert_eq!(v.get("seg").and_then(Json::as_arr).map(<[_]>::len), Some(2));
    }

    #[test]
    fn rejects_malformed() {
        assert!(Json::parse("").is_err());
        assert!(Json::parse("{").is_err());
        assert!(Json::parse("[1,]").is_err());
        assert!(Json::parse("tru").is_err());
        assert!(Json::parse("12 34").is_err()); // trailing data
        assert!(Json::parse("\"unterminated").is_err());
        assert!(Json::parse("-").is_err());
    }

    #[test]
    fn round_trips_object_deterministically() {
        let src = "{\"bri\":128,\"on\":true,\"tt\":7}";
        let v = Json::parse(src).expect("parse");
        // BTreeMap orders keys: bri, on, tt — matches src here.
        assert_eq!(v.to_string(), src);
    }

    #[test]
    fn escapes_round_trip() {
        let v = Json::Str("a\"b\\c\nd".to_string());
        let s = v.to_string();
        assert_eq!(Json::parse(&s), Ok(v));
    }

    #[test]
    fn unicode_escape_decodes() {
        assert_eq!(Json::parse("\"\\u00e7\""), Ok(Json::Str("ç".to_string())));
    }
}
