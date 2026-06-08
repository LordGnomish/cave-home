// SPDX-License-Identifier: Apache-2.0
//! Traefik routing-rule parser (hand-rolled, no regex dependency).
//!
//! Parses the documented Traefik v3 HTTP router rule grammar into an AST that
//! [`crate::matcher`] evaluates. Supported matchers:
//!
//! * `Host(`backtick value, …`)`
//! * `Path(`…`)` / `PathPrefix(`…`)`
//! * `Header(`name`, `value`)`
//! * `Method(`…`)`
//!
//! and the boolean combinators `&&`, `||`, `!`, with parenthesised grouping.
//! Backtick-quoted arguments are used (matching Traefik's own syntax), and a
//! matcher may take several comma-separated values which OR together (e.g.
//! `` Host(`a.com`, `b.com`) ``), per the public Traefik rules documentation.
//!
//! Spec basis: <https://doc.traefik.io/traefik/routing/routers/> (rule syntax).
//! This is a behavioural reimplementation from that documentation, not a port
//! of Traefik's own ANTLR-derived matcher engine.

use std::fmt;

/// A parsed routing rule expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    /// `Host(...)` — match the request host against any of the listed hosts.
    Host(Vec<String>),
    /// `Path(...)` — match the request path exactly against any listed path.
    Path(Vec<String>),
    /// `PathPrefix(...)` — match if the path starts with any listed prefix.
    PathPrefix(Vec<String>),
    /// `Header(name, value)` — match if the named header equals `value`.
    Header { name: String, value: String },
    /// `Method(...)` — match the request method against any listed method.
    Method(Vec<String>),
    /// Logical AND of two sub-rules (`&&`).
    And(Box<Self>, Box<Self>),
    /// Logical OR of two sub-rules (`||`).
    Or(Box<Self>, Box<Self>),
    /// Logical NOT of a sub-rule (`!`).
    Not(Box<Self>),
}

/// An error produced while parsing a rule string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The matcher name is not one this crate implements.
    UnknownMatcher(String),
    /// A token was expected but the input ended.
    UnexpectedEnd,
    /// An unexpected character/token was found, with a human-debug note.
    Unexpected(String),
    /// A matcher requires arguments but none were supplied.
    EmptyArguments(String),
    /// `Header()` requires exactly two arguments (name, value).
    HeaderArity,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMatcher(m) => write!(f, "unknown matcher: {m}"),
            Self::UnexpectedEnd => write!(f, "unexpected end of rule"),
            Self::Unexpected(s) => write!(f, "unexpected token: {s}"),
            Self::EmptyArguments(m) => write!(f, "matcher {m} has no arguments"),
            Self::HeaderArity => write!(f, "Header() takes exactly (name, value)"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a Traefik rule string into a [`Rule`] AST.
///
/// # Errors
/// Returns [`ParseError`] for unknown matchers, malformed argument lists,
/// unbalanced parentheses, or trailing input.
///
/// # Examples
/// ```
/// use cave_home_traefik_rs::rule::{parse, Rule};
/// let r = parse("Host(`example.com`) && PathPrefix(`/api`)").unwrap();
/// assert!(matches!(r, Rule::And(_, _)));
/// ```
pub fn parse(input: &str) -> Result<Rule, ParseError> {
    let tokens = lex(input)?;
    let mut p = Parser { tokens, pos: 0 };
    let rule = p.parse_or()?;
    if p.pos != p.tokens.len() {
        return Err(ParseError::Unexpected(format!("{:?}", p.tokens[p.pos])));
    }
    Ok(rule)
}

// --- lexer ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    /// A backtick-quoted string literal.
    Str(String),
    LParen,
    RParen,
    Comma,
    And,
    Or,
    Not,
}

fn lex(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'(' => {
                out.push(Token::LParen);
                i += 1;
            }
            b')' => {
                out.push(Token::RParen);
                i += 1;
            }
            b',' => {
                out.push(Token::Comma);
                i += 1;
            }
            b'`' => {
                // Backtick string: read until the closing backtick.
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'`' {
                    j += 1;
                }
                if j >= bytes.len() {
                    return Err(ParseError::Unexpected("unterminated `string`".to_string()));
                }
                let s = input
                    .get(start..j)
                    .ok_or_else(|| ParseError::Unexpected("bad string slice".to_string()))?;
                out.push(Token::Str(s.to_string()));
                i = j + 1;
            }
            b'&' => {
                if bytes.get(i + 1) == Some(&b'&') {
                    out.push(Token::And);
                    i += 2;
                } else {
                    return Err(ParseError::Unexpected("&".to_string()));
                }
            }
            b'|' => {
                if bytes.get(i + 1) == Some(&b'|') {
                    out.push(Token::Or);
                    i += 2;
                } else {
                    return Err(ParseError::Unexpected("|".to_string()));
                }
            }
            b'!' => {
                out.push(Token::Not);
                i += 1;
            }
            _ if c.is_ascii_alphabetic() => {
                let start = i;
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                let s = input
                    .get(start..j)
                    .ok_or_else(|| ParseError::Unexpected("bad ident slice".to_string()))?;
                out.push(Token::Ident(s.to_string()));
                i = j;
            }
            other => {
                return Err(ParseError::Unexpected((other as char).to_string()));
            }
        }
    }
    Ok(out)
}

// --- parser (recursive descent, precedence: OR < AND < NOT < primary) -------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, want: &Token) -> Result<(), ParseError> {
        match self.bump() {
            Some(ref t) if t == want => Ok(()),
            Some(t) => Err(ParseError::Unexpected(format!("{t:?}"))),
            None => Err(ParseError::UnexpectedEnd),
        }
    }

    /// or := and ( "||" and )*
    fn parse_or(&mut self) -> Result<Rule, ParseError> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.pos += 1;
            let rhs = self.parse_and()?;
            lhs = Rule::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    /// and := not ( "&&" not )*
    fn parse_and(&mut self) -> Result<Rule, ParseError> {
        let mut lhs = self.parse_not()?;
        while matches!(self.peek(), Some(Token::And)) {
            self.pos += 1;
            let rhs = self.parse_not()?;
            lhs = Rule::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    /// not := "!" not | primary
    fn parse_not(&mut self) -> Result<Rule, ParseError> {
        if matches!(self.peek(), Some(Token::Not)) {
            self.pos += 1;
            let inner = self.parse_not()?;
            Ok(Rule::Not(Box::new(inner)))
        } else {
            self.parse_primary()
        }
    }

    /// primary := "(" or ")" | matcher
    fn parse_primary(&mut self) -> Result<Rule, ParseError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.pos += 1;
                let inner = self.parse_or()?;
                self.expect(&Token::RParen)?;
                Ok(inner)
            }
            Some(Token::Ident(_)) => self.parse_matcher(),
            Some(other) => Err(ParseError::Unexpected(format!("{other:?}"))),
            None => Err(ParseError::UnexpectedEnd),
        }
    }

    fn parse_matcher(&mut self) -> Result<Rule, ParseError> {
        let name = match self.bump() {
            Some(Token::Ident(n)) => n,
            Some(t) => return Err(ParseError::Unexpected(format!("{t:?}"))),
            None => return Err(ParseError::UnexpectedEnd),
        };
        self.expect(&Token::LParen)?;
        let args = self.parse_args();
        self.expect(&Token::RParen)?;

        match name.as_str() {
            "Host" => {
                if args.is_empty() {
                    return Err(ParseError::EmptyArguments(name));
                }
                Ok(Rule::Host(args.into_iter().map(|h| h.to_ascii_lowercase()).collect()))
            }
            "Path" => {
                if args.is_empty() {
                    return Err(ParseError::EmptyArguments(name));
                }
                Ok(Rule::Path(args))
            }
            "PathPrefix" => {
                if args.is_empty() {
                    return Err(ParseError::EmptyArguments(name));
                }
                Ok(Rule::PathPrefix(args))
            }
            "Method" => {
                if args.is_empty() {
                    return Err(ParseError::EmptyArguments(name));
                }
                Ok(Rule::Method(
                    args.into_iter().map(|m| m.to_ascii_uppercase()).collect(),
                ))
            }
            "Header" => {
                if args.len() != 2 {
                    return Err(ParseError::HeaderArity);
                }
                let mut it = args.into_iter();
                let nm = it.next().ok_or(ParseError::HeaderArity)?;
                let val = it.next().ok_or(ParseError::HeaderArity)?;
                Ok(Rule::Header { name: nm, value: val })
            }
            other => Err(ParseError::UnknownMatcher(other.to_string())),
        }
    }

    /// args := ( STRING ( "," STRING )* )?
    fn parse_args(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(Token::Str(s)) = self.peek() {
            out.push(s.clone());
            self.pos += 1;
            if matches!(self.peek(), Some(Token::Comma)) {
                self.pos += 1;
            } else {
                break;
            }
        }
        out
    }
}

impl Rule {
    /// Traefik's default router priority is the length of the rule string.
    /// We expose the canonical computation here so the router can derive a
    /// default when no explicit priority is configured.
    ///
    /// Spec basis: Traefik computes a default priority equal to the length of
    /// the rule (`len(rule)`), so longer (more specific) rules win.
    #[must_use]
    pub const fn default_priority(rule_text: &str) -> usize {
        rule_text.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_host() {
        assert_eq!(
            parse("Host(`example.com`)").unwrap(),
            Rule::Host(vec!["example.com".to_string()])
        );
    }

    #[test]
    fn host_is_lowercased() {
        assert_eq!(
            parse("Host(`EXAMPLE.com`)").unwrap(),
            Rule::Host(vec!["example.com".to_string()])
        );
    }

    #[test]
    fn parses_multi_value_host_as_or_list() {
        assert_eq!(
            parse("Host(`a.com`, `b.com`)").unwrap(),
            Rule::Host(vec!["a.com".to_string(), "b.com".to_string()])
        );
    }

    #[test]
    fn parses_path_and_pathprefix() {
        assert_eq!(parse("Path(`/x`)").unwrap(), Rule::Path(vec!["/x".to_string()]));
        assert_eq!(
            parse("PathPrefix(`/api`)").unwrap(),
            Rule::PathPrefix(vec!["/api".to_string()])
        );
    }

    #[test]
    fn parses_header() {
        assert_eq!(
            parse("Header(`X-Env`, `prod`)").unwrap(),
            Rule::Header { name: "X-Env".to_string(), value: "prod".to_string() }
        );
    }

    #[test]
    fn parses_method_uppercased() {
        assert_eq!(
            parse("Method(`get`, `post`)").unwrap(),
            Rule::Method(vec!["GET".to_string(), "POST".to_string()])
        );
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // a || b && c  ==  a || (b && c)
        let r = parse("Host(`a`) || Host(`b`) && Host(`c`)").unwrap();
        match r {
            Rule::Or(l, right) => {
                assert_eq!(*l, Rule::Host(vec!["a".to_string()]));
                assert!(matches!(*right, Rule::And(_, _)));
            }
            other => panic!("expected Or at top, got {other:?}"),
        }
    }

    #[test]
    fn parentheses_override_precedence() {
        // (a || b) && c  ==  And(Or(a,b), c)
        let r = parse("(Host(`a`) || Host(`b`)) && Host(`c`)").unwrap();
        match r {
            Rule::And(l, rgt) => {
                assert!(matches!(*l, Rule::Or(_, _)));
                assert_eq!(*rgt, Rule::Host(vec!["c".to_string()]));
            }
            other => panic!("expected And at top, got {other:?}"),
        }
    }

    #[test]
    fn not_binds_to_primary() {
        let r = parse("!Host(`a`)").unwrap();
        assert_eq!(r, Rule::Not(Box::new(Rule::Host(vec!["a".to_string()]))));
    }

    #[test]
    fn double_not() {
        let r = parse("!!Method(`GET`)").unwrap();
        assert_eq!(
            r,
            Rule::Not(Box::new(Rule::Not(Box::new(Rule::Method(vec!["GET".to_string()])))))
        );
    }

    #[test]
    fn rejects_unknown_matcher() {
        assert_eq!(
            parse("Query(`a`)"),
            Err(ParseError::UnknownMatcher("Query".to_string()))
        );
    }

    #[test]
    fn rejects_header_wrong_arity() {
        assert_eq!(parse("Header(`only-one`)"), Err(ParseError::HeaderArity));
    }

    #[test]
    fn rejects_empty_arguments() {
        assert_eq!(parse("Host()"), Err(ParseError::EmptyArguments("Host".to_string())));
    }

    #[test]
    fn rejects_unbalanced_parens() {
        assert!(parse("(Host(`a`)").is_err());
    }

    #[test]
    fn rejects_unterminated_string() {
        assert!(parse("Host(`a)").is_err());
    }

    #[test]
    fn rejects_trailing_garbage() {
        assert!(parse("Host(`a`) Host(`b`)").is_err());
    }

    #[test]
    fn rejects_single_ampersand() {
        assert!(parse("Host(`a`) & Host(`b`)").is_err());
    }

    #[test]
    fn default_priority_is_rule_length() {
        assert_eq!(Rule::default_priority("Host(`a`)"), 9);
    }
}
