// SPDX-License-Identifier: Apache-2.0
//! WWW-Authenticate Bearer-challenge parsing.
//!
//! Line-by-line port of containerd's
//! `core/remotes/docker/auth/parse.go` (v2.3.0). We only need the
//! Bearer-scheme path — Basic and Digest are Phase 1b.

use std::collections::HashMap;

/// A parsed `WWW-Authenticate` challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// Lower-case auth scheme — `"bearer"`, `"basic"`, …
    pub scheme: String,
    /// `realm`, `service`, `scope`, …
    pub parameters: HashMap<String, String>,
}

/// Parses a single `WWW-Authenticate` header value. Returns `None`
/// for unrecognised schemes (Phase 1 only handles Bearer).
///
/// Mirrors `parseValueAndParams` from upstream parse.go.
#[must_use]
pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let (scheme, rest) = expect_token(header.trim_start());
    if scheme.is_empty() {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let mut params: HashMap<String, String> = HashMap::new();
    let mut s = rest;
    loop {
        s = skip_space(s);
        let (pkey, after_key) = expect_token(s);
        if pkey.is_empty() {
            break;
        }
        if !after_key.starts_with('=') {
            break;
        }
        let (pvalue, after_val) = expect_token_or_quoted(&after_key[1..]);
        params.insert(pkey.to_ascii_lowercase(), pvalue);
        s = skip_space(after_val);
        if !s.starts_with(',') {
            break;
        }
        s = &s[1..];
    }
    Some(Challenge { scheme, parameters: params })
}

fn skip_space(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    &s[i..]
}

fn is_token_byte(b: u8) -> bool {
    // RFC 2616 token: any CHAR except CTLs or separators
    if b > 127 || b < 33 {
        return false;
    }
    !matches!(
        b,
        b'(' | b')'
            | b'<'
            | b'>'
            | b'@'
            | b','
            | b';'
            | b':'
            | b'\\'
            | b'"'
            | b'/'
            | b'['
            | b']'
            | b'?'
            | b'='
            | b'{'
            | b'}'
            | b' '
            | b'\t'
    )
}

fn expect_token(s: &str) -> (String, &str) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_token_byte(bytes[i]) {
        i += 1;
    }
    (s[..i].to_owned(), &s[i..])
}

fn expect_token_or_quoted(s: &str) -> (String, &str) {
    if !s.starts_with('"') {
        let (t, rest) = expect_token(s);
        return (t, rest);
    }
    let body = &s[1..];
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return (String::from_utf8_lossy(&out).into_owned(), &body[i + 1..]),
            b'\\' if i + 1 < bytes.len() => {
                out.push(bytes[i + 1]);
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    (String::new(), "")
}

/// Picks the first Bearer challenge from a list of `WWW-Authenticate`
/// header values.
#[must_use]
pub fn first_bearer<I, S>(headers: I) -> Option<Challenge>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for h in headers {
        if let Some(c) = parse_challenge(h.as_ref()) {
            if c.scheme == "bearer" {
                return Some(c);
            }
        }
    }
    None
}
