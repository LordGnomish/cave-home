//! Sentence-template grammar — parse and compile spoken-sentence patterns.
//!
//! cave-home recognises commands the same way Rhasspy and Home Assistant's
//! "Assist" do: a household authors *sentence templates* — short patterns that
//! describe how a person might phrase a request — and the engine compiles them
//! into a matcher. This module is the grammar front-end. It is hand-rolled
//! std-only (no regex crate, per the Charter dependency-minimalism rule).
//!
//! # Grammar
//!
//! A template is plain text with three constructs:
//!
//! - **Optional** `[the]` — the enclosed words may or may not be spoken.
//! - **Alternatives** `(what is|what's)` — exactly one of the `|`-separated
//!   branches is spoken.
//! - **Slot** `{name}` — a placeholder that captures whatever the speaker
//!   said there, to be resolved later by a [`crate::slot`] definition.
//!
//! Optionals and alternatives may themselves contain words, slots, more
//! alternatives and more optionals — they nest. Everything else is a literal
//! word.
//!
//! ```
//! use cave_home_voice::template::Template;
//!
//! let t = Template::parse("turn [the] {name} on").expect("valid template");
//! // The template exposes the slot names it will capture.
//! assert_eq!(t.slot_names(), vec!["name".to_string()]);
//! ```

use core::fmt;

/// A compiled sentence template — a sequence of [`Element`]s the matcher walks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    elements: Vec<Element>,
}

/// One node in a compiled template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    /// A literal word that must be spoken (already lower-cased + punctuation
    /// stripped so it compares directly against normalised input tokens).
    Word(String),
    /// A `{slot}` capture; the inner string is the slot name.
    Slot(String),
    /// An `[optional]` group — the inner sequence may be absent.
    Optional(Vec<Element>),
    /// An `(a|b|c)` group — exactly one branch is spoken.
    Alternatives(Vec<Vec<Element>>),
}

/// Why a template string could not be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// A `[`, `(` or `{` was opened but never closed.
    Unclosed(char),
    /// A `]`, `)` or `}` appeared with no matching opener.
    Unexpected(char),
    /// A `{slot}` had an empty or whitespace-only name.
    EmptySlotName,
    /// The whole template (or a required branch) compiled to nothing.
    Empty,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemplateError::Unclosed(c) => write!(f, "unclosed `{c}` group in template"),
            TemplateError::Unexpected(c) => write!(f, "unexpected `{c}` in template"),
            TemplateError::EmptySlotName => write!(f, "a {{slot}} had no name"),
            TemplateError::Empty => write!(f, "template is empty"),
        }
    }
}

impl std::error::Error for TemplateError {}

impl Template {
    /// Compile a template string into matchable [`Element`]s.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError`] when brackets are unbalanced, a slot name is
    /// empty, or the template compiles to nothing.
    pub fn parse(src: &str) -> Result<Template, TemplateError> {
        let chars: Vec<char> = src.chars().collect();
        let mut cursor = 0;
        let elements = parse_seq(&chars, &mut cursor, None)?;
        if cursor != chars.len() {
            // parse_seq stopped early on a stray closer.
            return Err(TemplateError::Unexpected(chars[cursor]));
        }
        if elements.is_empty() {
            return Err(TemplateError::Empty);
        }
        Ok(Template { elements })
    }

    /// The compiled elements, in order.
    #[must_use]
    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    /// All slot names the template can capture, in first-seen order, without
    /// duplicates. Useful for wiring slot definitions and for tests.
    #[must_use]
    pub fn slot_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_slot_names(&self.elements, &mut out);
        out
    }
}

fn collect_slot_names(elements: &[Element], out: &mut Vec<String>) {
    for el in elements {
        match el {
            Element::Word(_) => {}
            Element::Slot(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Element::Optional(inner) => collect_slot_names(inner, out),
            Element::Alternatives(branches) => {
                for b in branches {
                    collect_slot_names(b, out);
                }
            }
        }
    }
}

/// Parse a sequence of elements until end-of-input or an unescaped `stop`
/// character. On `stop`, the cursor is left pointing *at* the stop char.
fn parse_seq(
    chars: &[char],
    cursor: &mut usize,
    stop: Option<char>,
) -> Result<Vec<Element>, TemplateError> {
    let mut elements = Vec::new();
    let mut word = String::new();

    // Flush an accumulated literal word into the element list (normalising it).
    let flush = |word: &mut String, elements: &mut Vec<Element>| {
        if !word.is_empty() {
            let norm: String = word
                .chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            if !norm.is_empty() {
                elements.push(Element::Word(norm));
            }
            word.clear();
        }
    };

    while *cursor < chars.len() {
        let c = chars[*cursor];
        if Some(c) == stop {
            flush(&mut word, &mut elements);
            return Ok(elements);
        }
        match c {
            ' ' | '\t' | '\n' => {
                flush(&mut word, &mut elements);
                *cursor += 1;
            }
            '[' => {
                flush(&mut word, &mut elements);
                *cursor += 1;
                let inner = parse_seq(chars, cursor, Some(']'))?;
                expect_close(chars, cursor, ']', '[')?;
                elements.push(Element::Optional(inner));
            }
            '(' => {
                flush(&mut word, &mut elements);
                *cursor += 1;
                let branches = parse_alternatives(chars, cursor)?;
                expect_close(chars, cursor, ')', '(')?;
                elements.push(Element::Alternatives(branches));
            }
            '{' => {
                flush(&mut word, &mut elements);
                *cursor += 1;
                let name = parse_slot_name(chars, cursor)?;
                expect_close(chars, cursor, '}', '{')?;
                elements.push(Element::Slot(name));
            }
            ']' | ')' | '}' => {
                // A closer with no matching opener at this level.
                flush(&mut word, &mut elements);
                return Err(TemplateError::Unexpected(c));
            }
            '|' => {
                // `|` only has meaning inside `parse_alternatives`; at the top
                // level it is a stray separator.
                return Err(TemplateError::Unexpected('|'));
            }
            other => {
                word.push(other);
                *cursor += 1;
            }
        }
    }

    // Reached end of input.
    if stop.is_some() {
        // We were expecting a closer but ran out of input.
        return Err(TemplateError::Unclosed(match stop {
            Some(']') => '[',
            Some(')') => '(',
            Some('}') => '{',
            _ => '?',
        }));
    }
    flush(&mut word, &mut elements);
    Ok(elements)
}

/// Parse the `a|b|c` branches inside `(...)`. Stops at the closing `)` (cursor
/// left pointing at it).
fn parse_alternatives(
    chars: &[char],
    cursor: &mut usize,
) -> Result<Vec<Vec<Element>>, TemplateError> {
    let mut branches = Vec::new();
    loop {
        // Each branch parses until `|` or `)`.
        let mut branch = Vec::new();
        let mut word = String::new();
        loop {
            if *cursor >= chars.len() {
                return Err(TemplateError::Unclosed('('));
            }
            let c = chars[*cursor];
            match c {
                '|' | ')' => {
                    push_word(&mut word, &mut branch);
                    break;
                }
                ' ' | '\t' | '\n' => {
                    push_word(&mut word, &mut branch);
                    *cursor += 1;
                }
                '[' => {
                    push_word(&mut word, &mut branch);
                    *cursor += 1;
                    let inner = parse_seq(chars, cursor, Some(']'))?;
                    expect_close(chars, cursor, ']', '[')?;
                    branch.push(Element::Optional(inner));
                }
                '(' => {
                    push_word(&mut word, &mut branch);
                    *cursor += 1;
                    let inner = parse_alternatives(chars, cursor)?;
                    expect_close(chars, cursor, ')', '(')?;
                    branch.push(Element::Alternatives(inner));
                }
                '{' => {
                    push_word(&mut word, &mut branch);
                    *cursor += 1;
                    let name = parse_slot_name(chars, cursor)?;
                    expect_close(chars, cursor, '}', '{')?;
                    branch.push(Element::Slot(name));
                }
                ']' | '}' => return Err(TemplateError::Unexpected(c)),
                other => {
                    word.push(other);
                    *cursor += 1;
                }
            }
        }
        branches.push(branch);
        if chars[*cursor] == ')' {
            return Ok(branches);
        }
        // chars[*cursor] == '|' — consume it and parse the next branch.
        *cursor += 1;
    }
}

fn push_word(word: &mut String, branch: &mut Vec<Element>) {
    if !word.is_empty() {
        let norm: String = word
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if !norm.is_empty() {
            branch.push(Element::Word(norm));
        }
        word.clear();
    }
}

fn parse_slot_name(chars: &[char], cursor: &mut usize) -> Result<String, TemplateError> {
    let mut name = String::new();
    while *cursor < chars.len() && chars[*cursor] != '}' {
        let c = chars[*cursor];
        if c == '{' || c == '[' || c == '(' {
            return Err(TemplateError::Unexpected(c));
        }
        name.push(c);
        *cursor += 1;
    }
    if *cursor >= chars.len() {
        return Err(TemplateError::Unclosed('{'));
    }
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(TemplateError::EmptySlotName);
    }
    Ok(name)
}

/// Verify the char at the cursor is `close`, then step past it.
fn expect_close(
    chars: &[char],
    cursor: &mut usize,
    close: char,
    open: char,
) -> Result<(), TemplateError> {
    if *cursor >= chars.len() {
        return Err(TemplateError::Unclosed(open));
    }
    if chars[*cursor] != close {
        return Err(TemplateError::Unexpected(chars[*cursor]));
    }
    *cursor += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &str) -> Element {
        Element::Word(w.to_string())
    }

    #[test]
    fn parses_plain_words_normalised() {
        let t = Template::parse("Turn On").expect("valid");
        assert_eq!(t.elements(), &[word("turn"), word("on")]);
    }

    #[test]
    fn collapses_extra_whitespace_and_strips_punctuation() {
        let t = Template::parse("  what's   the   weather?  ").expect("valid");
        assert_eq!(
            t.elements(),
            &[word("whats"), word("the"), word("weather")]
        );
    }

    #[test]
    fn parses_optional_group() {
        let t = Template::parse("turn [the] light on").expect("valid");
        assert_eq!(
            t.elements(),
            &[
                word("turn"),
                Element::Optional(vec![word("the")]),
                word("light"),
                word("on"),
            ]
        );
    }

    #[test]
    fn parses_alternatives() {
        let t = Template::parse("(what is|whats) it").expect("valid");
        assert_eq!(
            t.elements(),
            &[
                Element::Alternatives(vec![
                    vec![word("what"), word("is")],
                    vec![word("whats")],
                ]),
                word("it"),
            ]
        );
    }

    #[test]
    fn parses_slot() {
        let t = Template::parse("turn {name} on").expect("valid");
        assert_eq!(
            t.elements(),
            &[word("turn"), Element::Slot("name".to_string()), word("on")]
        );
    }

    #[test]
    fn slot_names_are_collected_uniquely_in_order() {
        let t = Template::parse("set {name} to {brightness} [for {name}]").expect("valid");
        assert_eq!(
            t.slot_names(),
            vec!["name".to_string(), "brightness".to_string()]
        );
    }

    #[test]
    fn nests_groups() {
        let t = Template::parse("turn [(the|a)] {name} on").expect("valid");
        // Just assert it compiled and has the slot.
        assert_eq!(t.slot_names(), vec!["name".to_string()]);
        assert!(matches!(t.elements()[1], Element::Optional(_)));
    }

    #[test]
    fn rejects_unclosed_optional() {
        assert_eq!(
            Template::parse("turn [the light on"),
            Err(TemplateError::Unclosed('['))
        );
    }

    #[test]
    fn rejects_unclosed_alternatives_and_slot() {
        assert_eq!(
            Template::parse("(a|b"),
            Err(TemplateError::Unclosed('('))
        );
        assert_eq!(
            Template::parse("turn {name on"),
            Err(TemplateError::Unclosed('{'))
        );
    }

    #[test]
    fn rejects_unexpected_closer() {
        assert_eq!(
            Template::parse("turn the light on]"),
            Err(TemplateError::Unexpected(']'))
        );
    }

    #[test]
    fn rejects_empty_slot_name() {
        assert_eq!(
            Template::parse("turn {  } on"),
            Err(TemplateError::EmptySlotName)
        );
    }

    #[test]
    fn rejects_empty_template() {
        assert_eq!(Template::parse("   "), Err(TemplateError::Empty));
    }
}
