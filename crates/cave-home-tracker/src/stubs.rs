// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Counts unfinished-work markers in Rust port code.
//!
//! The cave-home golden rule is **no stubs**. A port that compiles only because
//! it is littered with `todo!()`, `unimplemented!()` or `panic!("not yet")` is
//! not done, however good the LOC ratio looks. [`count_stubs`] surfaces those
//! markers so the honest-completion formula can penalise them.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Tally of unfinished-work markers found in a Rust source tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StubCount {
    /// `todo!` invocations.
    pub todo: u64,
    /// `unimplemented!` invocations.
    pub unimplemented: u64,
    /// `panic!` invocations (a panic in port code is a stand-in for real error
    /// handling).
    pub panic: u64,
}

impl StubCount {
    /// Total markers of any kind.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.todo + self.unimplemented + self.panic
    }

    /// Fold another tally into this one.
    pub const fn accumulate(&mut self, other: Self) {
        self.todo += other.todo;
        self.unimplemented += other.unimplemented;
        self.panic += other.panic;
    }
}

/// Count stub markers in a single Rust source string.
///
/// Lines whose first non-whitespace characters are `//` are treated as comments
/// and ignored, so doc-comments mentioning `todo!` do not inflate the count.
#[must_use]
pub fn count_in_source(src: &str) -> StubCount {
    let mut c = StubCount::default();
    for raw in src.lines() {
        let line = raw.trim_start();
        if line.starts_with("//") {
            continue;
        }
        c.todo += occurrences(line, "todo!");
        c.unimplemented += occurrences(line, "unimplemented!");
        c.panic += occurrences(line, "panic!");
    }
    c
}

/// Count non-overlapping occurrences of `needle` in `hay`.
fn occurrences(hay: &str, needle: &str) -> u64 {
    let mut n = 0;
    let mut rest = hay;
    while let Some(i) = rest.find(needle) {
        n += 1;
        rest = &rest[i + needle.len()..];
    }
    n
}

/// Recursively count stub markers across every `.rs` file under `root`.
///
/// # Errors
/// Returns an error only if `root` itself cannot be read.
pub fn count_stubs(root: &Path) -> crate::Result<StubCount> {
    let mut total = StubCount::default();
    walk(root, &mut total)?;
    Ok(total)
}

fn walk(dir: &Path, total: &mut StubCount) -> crate::Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| crate::TrackerError::io(dir, e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let name = entry.file_name();
            if matches!(name.to_string_lossy().as_ref(), ".git" | "target") {
                continue;
            }
            let _ = walk(&path, total);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(src) = std::fs::read_to_string(&path) {
                total.accumulate(count_in_source(&src));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_each_marker_kind() {
        let src =
            "fn f() { todo!() }\nfn g() { unimplemented!(\"x\") }\nfn h() { panic!(\"no\") }\n";
        let c = count_in_source(src);
        assert_eq!(c.todo, 1);
        assert_eq!(c.unimplemented, 1);
        assert_eq!(c.panic, 1);
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn ignores_comment_lines() {
        let src = "// this is fine: todo! later\n/// docs panic! mention\nlet x = 1;\n";
        assert_eq!(count_in_source(src).total(), 0);
    }

    #[test]
    fn counts_multiple_on_one_line() {
        assert_eq!(
            count_in_source("if a { panic!() } else { panic!() }").panic,
            2
        );
    }

    #[test]
    fn walks_directory_skipping_target() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.rs"), "fn a() { todo!() }\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/b.rs"), "fn b() { todo!() }\n").unwrap();
        let c = count_stubs(root).unwrap();
        assert_eq!(c.todo, 1, "target/ is ignored");
    }
}
