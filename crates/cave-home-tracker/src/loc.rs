// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! A small, honest source-line counter.
//!
//! The tracker needs to compare the size of an upstream project (Go, Python,
//! JavaScript, …) against the size of its cave-home port (Rust). We could shell
//! out to `tokei`, but that is not always installed and not in the offline
//! build cache, so we implement our own counter. It is deliberately simple:
//!
//! * files are classified into a [`Language`] by extension;
//! * each file is split into lines and every line is classified as *code*,
//!   *comment* or *blank* using the language's comment syntax;
//! * results are aggregated per language in a [`LocReport`].
//!
//! The counter is line-based and uses small heuristics for block comments. It
//! is not a full parser — but it is consistent, which is what matters for
//! tracking a ratio over time.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Per-language line counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangStat {
    /// Number of files counted.
    pub files: u64,
    /// Lines containing code.
    pub code: u64,
    /// Lines that are purely comments.
    pub comment: u64,
    /// Blank lines.
    pub blank: u64,
}

impl LangStat {
    const fn add(&mut self, other: Self) {
        self.files += other.files;
        self.code += other.code;
        self.comment += other.comment;
        self.blank += other.blank;
    }
}

/// Aggregated line counts for a directory tree, keyed by language name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocReport {
    /// Per-language statistics, keyed by [`Language::name`].
    pub per_lang: BTreeMap<String, LangStat>,
}

impl LocReport {
    /// Total code lines across every language.
    #[must_use]
    pub fn total_code(&self) -> u64 {
        self.per_lang.values().map(|s| s.code).sum()
    }

    /// Code lines belonging only to the named languages.
    #[must_use]
    pub fn code_for(&self, langs: &[&str]) -> u64 {
        langs
            .iter()
            .filter_map(|l| self.per_lang.get(*l))
            .map(|s| s.code)
            .sum()
    }

    /// Total files counted.
    #[must_use]
    pub fn total_files(&self) -> u64 {
        self.per_lang.values().map(|s| s.files).sum()
    }

    fn merge(&mut self, lang: &str, stat: LangStat) {
        self.per_lang.entry(lang.to_owned()).or_default().add(stat);
    }
}

/// A source language the counter understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    name: &'static str,
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
}

impl Language {
    /// Canonical lower-case language name (e.g. `"rust"`, `"go"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Resolve a language from a file extension (without the dot), case-folded.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        let c_like = Some(("/*", "*/"));
        let lang = |name, line: &'static [&'static str], block| Self { name, line, block };
        let l = match ext.to_ascii_lowercase().as_str() {
            "rs" => lang("rust", &["//"], c_like),
            "go" => lang("go", &["//"], c_like),
            "py" | "pyi" => lang("python", &["#"], None),
            "js" | "mjs" | "cjs" | "jsx" => lang("javascript", &["//"], c_like),
            "ts" | "tsx" => lang("typescript", &["//"], c_like),
            "c" | "h" => lang("c", &["//"], c_like),
            "cc" | "cpp" | "cxx" | "hpp" | "hh" => lang("cpp", &["//"], c_like),
            "java" => lang("java", &["//"], c_like),
            "sh" | "bash" => lang("shell", &["#"], None),
            "yaml" | "yml" => lang("yaml", &["#"], None),
            "toml" => lang("toml", &["#"], None),
            "json" => lang("json", &[], None),
            "md" | "markdown" => lang("markdown", &[], None),
            "proto" => lang("protobuf", &["//"], c_like),
            _ => return None,
        };
        Some(l)
    }

    /// Classify the lines of `content` for this language.
    #[must_use]
    pub fn count(self, content: &str) -> LangStat {
        let mut stat = LangStat {
            files: 1,
            ..LangStat::default()
        };
        let mut in_block = false;
        for raw in content.lines() {
            let line = raw.trim();
            if in_block {
                stat.comment += 1;
                if let Some((_, end)) = self.block {
                    if line.contains(end) {
                        in_block = false;
                    }
                }
                continue;
            }
            if line.is_empty() {
                stat.blank += 1;
                continue;
            }
            if self.line.iter().any(|tok| line.starts_with(tok)) {
                stat.comment += 1;
                continue;
            }
            if let Some((start, end)) = self.block {
                if let Some(after) = line.strip_prefix(start) {
                    stat.comment += 1;
                    // single-line `/* ... */` closes immediately; otherwise we
                    // are now inside a multi-line block.
                    if !after.contains(end) {
                        in_block = true;
                    }
                    continue;
                }
                // code line that opens (but does not close) a trailing block.
                if let Some(open) = line.rfind(start) {
                    if !line[open + start.len()..].contains(end) {
                        in_block = true;
                    }
                }
            }
            stat.code += 1;
        }
        stat
    }
}

/// Directory names that are never descended into.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "vendor",
    ".cargo",
    "dist",
    "_output",
    ".idea",
    ".vscode",
];

/// Count every recognised source file under `root`, recursively.
///
/// Unreadable files and unrecognised extensions are silently skipped; the
/// counter never fails on a single bad file, which keeps a `poll` of a large
/// upstream robust.
///
/// # Errors
/// Returns [`TrackerError::Io`](crate::TrackerError::Io) only if `root` itself
/// cannot be read as a directory.
pub fn count_dir(root: &Path) -> crate::Result<LocReport> {
    let mut report = LocReport::default();
    count_into(root, &mut report)?;
    Ok(report)
}

fn count_into(dir: &Path, report: &mut LocReport) -> crate::Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| crate::TrackerError::io(dir, e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            // A nested unreadable directory should not abort the whole walk.
            let _ = count_into(&path, report);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(lang) = Language::from_extension(ext) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        report.merge(lang.name(), lang.count(&content));
    }
    Ok(())
}

/// Count only the given `subpaths` (relative to `root`); empty means all of
/// `root`. Used to restrict an upstream's LOC to the directories that the
/// corresponding cave-home subsystem actually ports.
///
/// # Errors
/// Returns an error only if `root` exists but cannot be read.
pub fn count_subpaths(root: &Path, subpaths: &[String]) -> crate::Result<LocReport> {
    if subpaths.is_empty() {
        return count_dir(root);
    }
    let mut report = LocReport::default();
    for sub in subpaths {
        let target = root.join(sub);
        if target.is_dir() {
            count_into(&target, &mut report)?;
        } else if target.is_file() {
            if let Some(lang) = target
                .extension()
                .and_then(|e| e.to_str())
                .and_then(Language::from_extension)
            {
                if let Ok(content) = std::fs::read_to_string(&target) {
                    report.merge(lang.name(), lang.count(&content));
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_extensions() {
        assert_eq!(Language::from_extension("rs").unwrap().name(), "rust");
        assert_eq!(Language::from_extension("GO").unwrap().name(), "go");
        assert_eq!(Language::from_extension("Py").unwrap().name(), "python");
        assert!(Language::from_extension("xyz").is_none());
    }

    #[test]
    fn counts_rust_code_comment_blank() {
        let src = "fn main() {\n    // a comment\n    let x = 1;\n\n}\n";
        let rust = Language::from_extension("rs").unwrap();
        let stat = rust.count(src);
        assert_eq!(stat.code, 3, "fn/let/closing-brace are code");
        assert_eq!(stat.comment, 1);
        assert_eq!(stat.blank, 1);
        assert_eq!(stat.files, 1);
    }

    #[test]
    fn handles_block_comments() {
        let src = "let a = 1;\n/* multi\n line\n comment */\nlet b = 2;\n";
        let rust = Language::from_extension("rs").unwrap();
        let stat = rust.count(src);
        assert_eq!(stat.code, 2);
        assert_eq!(stat.comment, 3);
    }

    #[test]
    fn single_line_block_comment_is_one_comment() {
        let rust = Language::from_extension("rs").unwrap();
        let stat = rust.count("/* one liner */\ncode();\n");
        assert_eq!(stat.comment, 1);
        assert_eq!(stat.code, 1);
    }

    #[test]
    fn trailing_block_open_enters_block_state() {
        let rust = Language::from_extension("rs").unwrap();
        // code line that opens a block at its end
        let stat = rust.count("let x = 1; /* note\nstill comment */\ndone();\n");
        assert_eq!(stat.code, 2, "first and last lines are code");
        assert_eq!(stat.comment, 1, "the middle line closes the block");
    }

    #[test]
    fn python_uses_hash_comments() {
        let py = Language::from_extension("py").unwrap();
        let stat = py.count("# header\nx = 1\n\n");
        assert_eq!(stat.comment, 1);
        assert_eq!(stat.code, 1);
        assert_eq!(stat.blank, 1);
    }

    #[test]
    fn report_aggregation_and_selectors() {
        let mut r = LocReport::default();
        r.merge(
            "rust",
            LangStat {
                files: 1,
                code: 10,
                comment: 2,
                blank: 1,
            },
        );
        r.merge(
            "rust",
            LangStat {
                files: 1,
                code: 5,
                comment: 0,
                blank: 0,
            },
        );
        r.merge(
            "go",
            LangStat {
                files: 1,
                code: 100,
                comment: 0,
                blank: 0,
            },
        );
        assert_eq!(r.total_code(), 115);
        assert_eq!(r.code_for(&["rust"]), 15);
        assert_eq!(r.code_for(&["go"]), 100);
        assert_eq!(r.code_for(&["rust", "go"]), 115);
        assert_eq!(r.total_files(), 3);
    }

    #[test]
    fn count_dir_walks_and_skips_target() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.go"), "package main\nfunc b() {}\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/junk.rs"), "fn junk() {}\n").unwrap();

        let report = count_dir(root).unwrap();
        assert_eq!(report.code_for(&["rust"]), 1, "only a.rs, target skipped");
        assert_eq!(report.code_for(&["go"]), 2);
    }

    #[test]
    fn count_subpaths_restricts_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("pkg/kine")).unwrap();
        std::fs::create_dir_all(root.join("pkg/other")).unwrap();
        std::fs::write(root.join("pkg/kine/k.go"), "package kine\nfunc K() {}\n").unwrap();
        std::fs::write(
            root.join("pkg/other/o.go"),
            "package other\nfunc O() {}\nfunc P(){}\n",
        )
        .unwrap();

        let scoped = count_subpaths(root, &["pkg/kine".to_owned()]).unwrap();
        assert_eq!(scoped.code_for(&["go"]), 2);
        let all = count_subpaths(root, &[]).unwrap();
        assert_eq!(all.code_for(&["go"]), 5);
    }
}
