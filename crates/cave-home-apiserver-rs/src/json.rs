// SPDX-License-Identifier: Apache-2.0
//! A small, std-only JSON value tree.
//!
//! This crate is the Kubernetes apiserver *decision core* and deliberately
//! avoids pulling in a serialization framework: the REST/patch/selector logic
//! operates on an in-memory value tree, not on the wire. `Value` models the
//! object graph an apiserver manipulates (merge-patch, field selectors, label
//! maps) without any external dependency. A real HTTP/etcd wire codec is
//! deferred (see `parity.manifest.toml`).
//!
//! Behavioural reference: the JSON object model used throughout the Kubernetes
//! API conventions and RFC 7396 (JSON Merge Patch) / RFC 6902 (JSON Patch).

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

/// A JSON value. Objects use a `BTreeMap` so iteration/serialization is
/// deterministic (important for stable test assertions and stable diffs).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    /// JSON `null`.
    #[default]
    Null,
    /// JSON boolean.
    Bool(bool),
    /// JSON number. Kubernetes never needs more than an `f64` of range for the
    /// fields the decision core inspects; integers round-trip exactly up to
    /// 2^53 which covers every count/port/version we model.
    Number(f64),
    /// JSON string.
    String(String),
    /// JSON array.
    Array(Vec<Value>),
    /// JSON object (sorted keys).
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// Build an empty object.
    #[must_use]
    pub fn object() -> Self {
        Value::Object(BTreeMap::new())
    }

    /// Borrow as object map.
    #[must_use]
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    /// Mutably borrow as object map.
    #[must_use]
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    /// Borrow as string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Borrow as array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    /// Borrow as bool.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// True for JSON null.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Look up a key on an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|m| m.get(key))
    }

    /// Resolve a dotted path (`a.b.c`) against nested objects. Returns `None`
    /// at the first non-object/missing segment. Used by field selectors.
    #[must_use]
    pub fn pointer(&self, path: &str) -> Option<&Value> {
        let mut cur = self;
        for seg in path.split('.') {
            cur = cur.get(seg)?;
        }
        Some(cur)
    }

    /// Set `key` to `value` on an object, creating object-ness if needed is
    /// **not** done here — caller must ensure self is an object.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        if let Value::Object(m) = self {
            m.insert(key.into(), value);
        }
    }

    /// Render to a canonical, compact JSON string (sorted keys). This is for
    /// diagnostics, stable diffing and tests — not a negotiated wire codec.
    #[must_use]
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        // write! into a String is infallible; ignore the formatting Result.
        let _ = self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) -> fmt::Result {
        match self {
            Value::Null => out.write_str("null"),
            Value::Bool(true) => out.write_str("true"),
            Value::Bool(false) => out.write_str("false"),
            Value::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() && n.abs() < 9e15 {
                    write!(out, "{}", *n as i64)
                } else if n.is_finite() {
                    write!(out, "{n}")
                } else {
                    // JSON has no NaN/Inf; emit null to stay valid.
                    out.write_str("null")
                }
            }
            Value::String(s) => write_json_string(s, out),
            Value::Array(a) => {
                out.write_char('[')?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.write_char(',')?;
                    }
                    v.write_json(out)?;
                }
                out.write_char(']')
            }
            Value::Object(m) => {
                out.write_char('{')?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.write_char(',')?;
                    }
                    write_json_string(k, out)?;
                    out.write_char(':')?;
                    v.write_json(out)?;
                }
                out.write_char('}')
            }
        }
    }
}

fn write_json_string(s: &str, out: &mut String) -> fmt::Result {
    out.write_char('"')?;
    for c in s.chars() {
        match c {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32)?,
            c => out.write_char(c)?,
        }
    }
    out.write_char('"')
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Number(n as f64)
    }
}
impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

/// A JSON parse failure: a human-readable reason plus the byte offset at which
/// it was detected. Kept deliberately small (std-only) — the transport layer
/// maps it to a `BadRequest` status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonError {
    /// What went wrong.
    pub message: String,
    /// Byte offset into the input where the error was detected.
    pub offset: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JSON at offset {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for JsonError {}

/// Parse a JSON document into a [`Value`].
///
/// A recursive-descent parser over the documented JSON grammar (RFC 8259), with
/// no external dependency — the apiserver decision core deserializes request
/// bodies into the same value tree the REST/patch/selector logic operates on.
/// Trailing non-whitespace content after the top-level value is rejected.
///
/// # Errors
/// Returns [`JsonError`] for any malformed input (bad token, unterminated
/// string, trailing garbage, empty input, …).
pub fn parse(input: &str) -> std::result::Result<Value, JsonError> {
    let mut p = Parser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(p.err("unexpected trailing characters"));
    }
    Ok(v)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> JsonError {
        JsonError {
            message: message.to_string(),
            offset: self.pos,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> std::result::Result<Value, JsonError> {
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(Value::String(self.parse_string()?)),
            Some(b't' | b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err(self.err("expected a JSON value")),
        }
    }

    fn expect_lit(&mut self, lit: &str, value: Value) -> std::result::Result<Value, JsonError> {
        let end = self.pos + lit.len();
        if self.bytes.get(self.pos..end) == Some(lit.as_bytes()) {
            self.pos = end;
            Ok(value)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn parse_null(&mut self) -> std::result::Result<Value, JsonError> {
        self.expect_lit("null", Value::Null)
    }

    fn parse_bool(&mut self) -> std::result::Result<Value, JsonError> {
        if self.peek() == Some(b't') {
            self.expect_lit("true", Value::Bool(true))
        } else {
            self.expect_lit("false", Value::Bool(false))
        }
    }

    fn parse_number(&mut self) -> std::result::Result<Value, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let slice = &self.bytes[start..self.pos];
        if slice.is_empty() || slice == b"-" {
            return Err(self.err("invalid number"));
        }
        // The slice is ASCII digits/sign/dot/exp by construction → valid UTF-8.
        let text = std::str::from_utf8(slice).map_err(|_| self.err("invalid number"))?;
        let n: f64 = text.parse().map_err(|_| JsonError {
            message: "number out of range".to_string(),
            offset: start,
        })?;
        Ok(Value::Number(n))
    }

    fn parse_string(&mut self) -> std::result::Result<String, JsonError> {
        // Opening quote.
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated string")),
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
                            let cp = self.parse_unicode_escape()?;
                            out.push(cp);
                            continue;
                        }
                        _ => return Err(self.err("invalid escape")),
                    }
                    self.pos += 1;
                }
                Some(_) => {
                    // Copy one UTF-8 scalar starting at pos.
                    let rest = &self.bytes[self.pos..];
                    let s = std::str::from_utf8(rest).map_err(|_| self.err("invalid UTF-8"))?;
                    let ch = s.chars().next().ok_or_else(|| self.err("invalid UTF-8"))?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> std::result::Result<u32, JsonError> {
        // pos is at the 'u'; advance past it then read 4 hex digits.
        self.pos += 1;
        let end = self.pos + 4;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| self.err("truncated \\u escape"))?;
        let text = std::str::from_utf8(slice).map_err(|_| self.err("bad \\u escape"))?;
        let cp = u32::from_str_radix(text, 16).map_err(|_| self.err("bad \\u hex"))?;
        self.pos = end;
        Ok(cp)
    }

    fn parse_unicode_escape(&mut self) -> std::result::Result<char, JsonError> {
        let hi = self.parse_hex4()?;
        // Surrogate pair: high surrogate followed by \uXXXX low surrogate.
        if (0xD800..=0xDBFF).contains(&hi) {
            if self.peek() == Some(b'\\') && self.bytes.get(self.pos + 1) == Some(&b'u') {
                self.pos += 1; // consume backslash; parse_hex4 consumes the 'u'
                let lo = self.parse_hex4()?;
                if (0xDC00..=0xDFFF).contains(&lo) {
                    let c = 0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                    return char::from_u32(c).ok_or_else(|| self.err("invalid surrogate pair"));
                }
            }
            return Err(self.err("unpaired high surrogate"));
        }
        char::from_u32(hi).ok_or_else(|| self.err("invalid \\u code point"))
    }

    fn parse_array(&mut self) -> std::result::Result<Value, JsonError> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.err("expected ',' or ']' in array")),
            }
        }
    }

    fn parse_object(&mut self) -> std::result::Result<Value, JsonError> {
        self.pos += 1; // '{'
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected string key in object"));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':' after key"));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(map));
                }
                _ => return Err(self.err("expected ',' or '}' in object")),
            }
        }
    }
}

/// Build a `Value::Object` from key/value pairs.
#[must_use]
pub fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_resolves_nested() {
        let v = obj([("a", obj([("b", Value::from("x"))]))]);
        assert_eq!(v.pointer("a.b").and_then(Value::as_str), Some("x"));
        assert!(v.pointer("a.c").is_none());
        assert!(v.pointer("z").is_none());
    }

    #[test]
    fn to_json_string_sorts_keys_and_quotes() {
        let v = obj([("b", Value::from(2_i64)), ("a", Value::from("hi"))]);
        assert_eq!(v.to_json_string(), r#"{"a":"hi","b":2}"#);
    }

    #[test]
    fn integers_render_without_decimal() {
        assert_eq!(Value::from(42_i64).to_json_string(), "42");
        assert_eq!(Value::from(1.5_f64).to_json_string(), "1.5");
    }

    #[test]
    fn string_escaping() {
        assert_eq!(
            Value::from("a\"b\nc").to_json_string(),
            r#""a\"b\nc""#
        );
    }

    // --- parser (request-body decode) ---------------------------------------

    #[test]
    fn parse_literals() {
        assert_eq!(parse("null").expect("null"), Value::Null);
        assert_eq!(parse("true").expect("true"), Value::Bool(true));
        assert_eq!(parse("false").expect("false"), Value::Bool(false));
    }

    #[test]
    fn parse_numbers() {
        assert_eq!(parse("42").expect("int"), Value::from(42_i64));
        assert_eq!(parse("-7").expect("neg"), Value::from(-7_i64));
        assert_eq!(parse("1.5").expect("float"), Value::from(1.5_f64));
        assert_eq!(parse("1e3").expect("exp"), Value::from(1000.0_f64));
        assert_eq!(parse("-2.5e-1").expect("exp2"), Value::from(-0.25_f64));
    }

    #[test]
    fn parse_strings_with_escapes() {
        assert_eq!(parse(r#""hi""#).expect("s"), Value::from("hi"));
        assert_eq!(parse(r#""a\"b\nc""#).expect("esc"), Value::from("a\"b\nc"));
        assert_eq!(parse(r#""A""#).expect("u"), Value::from("A"));
        assert_eq!(parse(r#""tab\there""#).expect("t"), Value::from("tab\there"));
    }

    #[test]
    fn parse_array_and_object_nested() {
        let v = parse(r#"{"a":[1,2,{"b":true}],"c":null}"#).expect("nested");
        assert_eq!(v.pointer("a").and_then(Value::as_array).map(<[_]>::len), Some(3));
        assert_eq!(v.pointer("c"), Some(&Value::Null));
        // index 2 of array is an object {b:true}
        let third = &v.pointer("a").and_then(Value::as_array).unwrap()[2];
        assert_eq!(third.pointer("b"), Some(&Value::Bool(true)));
    }

    #[test]
    fn parse_ignores_surrounding_whitespace() {
        assert_eq!(parse("  \n\t {\r\n }  ").expect("ws"), Value::object());
    }

    #[test]
    fn parse_round_trips_to_json_string() {
        for raw in [
            r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx","namespace":"default"},"spec":{"replicas":3}}"#,
            r#"[1,2,3]"#,
            r#"{"nested":{"deep":{"x":[true,false,null]}}}"#,
        ] {
            let v = parse(raw).expect("parse");
            let s = v.to_json_string();
            let v2 = parse(&s).expect("reparse");
            assert_eq!(v, v2, "round-trip for {raw}");
        }
    }

    #[test]
    fn parse_rejects_trailing_garbage() {
        assert!(parse("{} junk").is_err());
        assert!(parse("123 456").is_err());
    }

    #[test]
    fn parse_rejects_unterminated_and_malformed() {
        assert!(parse(r#""no end"#).is_err());
        assert!(parse("{\"a\":}").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("").is_err());
        assert!(parse("tru").is_err());
    }
}
