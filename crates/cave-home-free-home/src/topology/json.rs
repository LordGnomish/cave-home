// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! A small, tolerant JSON reader.
//!
//! Phase 1 is std-only with no external crate, so this carries just enough JSON
//! to read the System Access Point's get-all configuration response: objects,
//! arrays, strings, numbers, booleans and null. Numbers are kept as their raw
//! text (the device tree never needs them as floats — pairing IDs and values
//! are read back out as strings/integers by the caller). It is recursion-bounded
//! to avoid unbounded stack growth on hostile input.

use std::collections::BTreeMap;

/// A parsed JSON value. Object keys are ordered so iteration is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    /// A number or string scalar, kept as text.
    Str(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl Json {
    /// Parse a complete JSON document.
    ///
    /// # Errors
    /// Returns a human description if the input is not valid JSON in the subset
    /// supported here, or if trailing non-whitespace remains.
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut p = Parser {
            bytes: input.as_bytes(),
            pos: 0,
        };
        p.skip_ws();
        let value = p.value(0)?;
        p.skip_ws();
        if p.pos != p.bytes.len() {
            return Err(format!("trailing data at byte {}", p.pos));
        }
        Ok(value)
    }

    /// Borrow as an object, if it is one.
    #[must_use]
    pub const fn as_object(&self) -> Option<&BTreeMap<String, Self>> {
        match self {
            Self::Object(m) => Some(m),
            _ => None,
        }
    }

    /// Borrow as a string/number scalar, if it is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 64;

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

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nesting too deep".to_string());
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b) if b == b'-' || b.is_ascii_digit() => Ok(Json::Str(self.number())),
            Some(b) => Err(format!("unexpected byte {b:#x} at {}", self.pos)),
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn literal(&mut self, word: &str, val: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(val)
        } else {
            Err(format!("invalid literal at {}", self.pos))
        }
    }

    fn number(&mut self) -> String {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit()
                || b == b'-'
                || b == b'+'
                || b == b'.'
                || b == b'e'
                || b == b'E'
            {
                self.pos += 1;
            } else {
                break;
            }
        }
        // start..pos is ASCII (digits + number punctuation), so this is valid UTF-8.
        String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned()
    }

    fn string(&mut self) -> Result<String, String> {
        // Consume opening quote.
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".to_string()),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'/') => out.push('/'),
                        Some(b'n') => out.push('\n'),
                        Some(b't') => out.push('\t'),
                        Some(b'r') => out.push('\r'),
                        Some(b'b') => out.push('\u{0008}'),
                        Some(b'f') => out.push('\u{000C}'),
                        Some(b'u') => {
                            let cp = self.unicode_escape()?;
                            // Skip the 4 hex digits handled inside unicode_escape.
                            match char::from_u32(cp) {
                                Some(c) => out.push(c),
                                None => out.push('\u{FFFD}'),
                            }
                            continue;
                        }
                        _ => return Err("invalid escape".to_string()),
                    }
                    self.pos += 1;
                }
                Some(_) => {
                    // Copy one UTF-8 char from the source.
                    let rest = &self.bytes[self.pos..];
                    let s = match std::str::from_utf8(rest) {
                        Ok(s) => s,
                        Err(e) if e.valid_up_to() > 0 => {
                            // Safe: valid_up_to bytes are valid UTF-8.
                            match std::str::from_utf8(&rest[..e.valid_up_to()]) {
                                Ok(s) => s,
                                Err(_) => return Err("invalid utf-8 in string".to_string()),
                            }
                        }
                        Err(_) => return Err("invalid utf-8 in string".to_string()),
                    };
                    let Some(ch) = s.chars().next() else {
                        return Err("unterminated string".to_string());
                    };
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<u32, String> {
        // self.pos points at 'u'; consume it then 4 hex digits.
        self.pos += 1;
        let end = self.pos + 4;
        if end > self.bytes.len() {
            return Err("truncated \\u escape".to_string());
        }
        let Ok(hex) = std::str::from_utf8(&self.bytes[self.pos..end]) else {
            return Err("invalid \\u escape".to_string());
        };
        let cp = u32::from_str_radix(hex, 16).map_err(|_| "invalid \\u escape".to_string())?;
        self.pos = end;
        Ok(cp)
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            let v = self.value(depth + 1)?;
            items.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected ',' or ']' at {}", self.pos)),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.pos += 1; // '{'
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(format!("expected object key at {}", self.pos));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(format!("expected ':' at {}", self.pos));
            }
            self.pos += 1;
            let value = self.value(depth + 1)?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(map));
                }
                _ => return Err(format!("expected ',' or '}}' at {}", self.pos)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_object() {
        let j = Json::parse(r#"{"a":{"b":"c"},"d":[1,2,3]}"#).expect("ok");
        let o = j.as_object().expect("object");
        assert_eq!(
            o.get("a").and_then(Json::as_object).and_then(|m| m.get("b")),
            Some(&Json::Str("c".into()))
        );
        assert!(matches!(o.get("d"), Some(Json::Array(v)) if v.len() == 3));
    }

    #[test]
    fn numbers_kept_as_text() {
        let j = Json::parse(r#"{"n":256,"f":-1.5e2}"#).expect("ok");
        let o = j.as_object().expect("object");
        assert_eq!(o.get("n").and_then(Json::as_str), Some("256"));
        assert_eq!(o.get("f").and_then(Json::as_str), Some("-1.5e2"));
    }

    #[test]
    fn booleans_and_null() {
        let j = Json::parse(r#"{"t":true,"f":false,"z":null}"#).expect("ok");
        let o = j.as_object().expect("object");
        assert_eq!(o.get("t"), Some(&Json::Bool(true)));
        assert_eq!(o.get("z"), Some(&Json::Null));
    }

    #[test]
    fn string_escapes_and_unicode() {
        let j = Json::parse(r#"{"k":"a\tbç\n"}"#).expect("ok");
        assert_eq!(
            j.as_object().and_then(|m| m.get("k")).and_then(Json::as_str),
            Some("a\tbç\n")
        );
    }

    #[test]
    fn empty_containers() {
        assert_eq!(Json::parse("{}"), Ok(Json::Object(BTreeMap::new())));
        assert_eq!(Json::parse("[]"), Ok(Json::Array(Vec::new())));
    }

    #[test]
    fn rejects_trailing_and_malformed() {
        assert!(Json::parse(r#"{"a":1}x"#).is_err());
        assert!(Json::parse("{").is_err());
        assert!(Json::parse(r#"{"a"}"#).is_err());
        assert!(Json::parse(r#"{"a":}"#).is_err());
    }

    #[test]
    fn rejects_deep_nesting() {
        let deep = "[".repeat(MAX_DEPTH + 5);
        assert!(Json::parse(&deep).is_err());
    }
}
