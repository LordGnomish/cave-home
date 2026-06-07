// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Turning a polled checkout into a [`Snapshot`].
//!
//! For each subsystem the measurer:
//! * counts upstream LOC over the referenced clone sub-directories;
//! * counts port LOC (Rust) over the subsystem's crates;
//! * counts stub markers in those crates;
//! * runs the crates' tests via a [`TestRunner`] seam;
//! * folds it all into a [`SubsystemMetric`] via the honest formula.
//!
//! The [`TestRunner`] seam keeps the orchestration unit-testable; production
//! uses [`CargoTestRunner`] which shells out to `cargo test`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::TrackerConfig;
use crate::snapshot::{Snapshot, SubsystemMetric};
use crate::stubs::{self, StubCount};
use crate::{TrackerError, loc};

/// The result of running a crate's test suite.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestOutcome {
    /// Tests that passed.
    pub passed: u64,
    /// Tests that failed.
    pub failed: u64,
    /// Tests marked ignored.
    pub ignored: u64,
}

impl TestOutcome {
    const fn add(&mut self, other: Self) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.ignored += other.ignored;
    }
}

/// Abstraction over running a crate's tests.
pub trait TestRunner {
    /// Run the tests for the crate rooted at `crate_dir`.
    ///
    /// # Errors
    /// Returns an error if the test process cannot be launched. A *failing*
    /// suite is not an error — it is reported via [`TestOutcome::failed`].
    fn test(&self, crate_dir: &Path) -> crate::Result<TestOutcome>;
}

/// A [`TestRunner`] that always reports zero tests — useful for a fast
/// `measure` that only wants LOC/stub numbers.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopTestRunner;

impl TestRunner for NoopTestRunner {
    fn test(&self, _crate_dir: &Path) -> crate::Result<TestOutcome> {
        Ok(TestOutcome::default())
    }
}

/// Parse the `test result:` summary lines emitted by `cargo test` / libtest,
/// summing across every test binary in the output.
#[must_use]
pub fn parse_cargo_test(output: &str) -> TestOutcome {
    let mut total = TestOutcome::default();
    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("test result:") else {
            continue;
        };
        // e.g. "ok. 12 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out"
        total.add(TestOutcome {
            passed: extract_before(rest, "passed"),
            failed: extract_before(rest, "failed"),
            ignored: extract_before(rest, "ignored"),
        });
    }
    total
}

/// Pull the integer that immediately precedes `label` in a libtest summary.
fn extract_before(s: &str, label: &str) -> u64 {
    let Some(idx) = s.find(label) else { return 0 };
    s[..idx]
        .split_whitespace()
        .last()
        .and_then(|tok| tok.parse().ok())
        .unwrap_or(0)
}

/// Real test runner: `cargo test` for the crate at `crate_dir`.
#[derive(Debug, Clone)]
pub struct CargoTestRunner {
    offline: bool,
}

impl Default for CargoTestRunner {
    fn default() -> Self {
        Self { offline: true }
    }
}

impl CargoTestRunner {
    /// New runner; `offline` adds `--offline` to honour the cached registry.
    #[must_use]
    pub const fn new(offline: bool) -> Self {
        Self { offline }
    }
}

impl TestRunner for CargoTestRunner {
    fn test(&self, crate_dir: &Path) -> crate::Result<TestOutcome> {
        let manifest = crate_dir.join("Cargo.toml");
        let mut cmd = Command::new("cargo");
        cmd.arg("test")
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--lib");
        if self.offline {
            cmd.arg("--offline");
        }
        let output = cmd
            .output()
            .map_err(|e| TrackerError::io(PathBuf::from("cargo"), e))?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(parse_cargo_test(&combined))
    }
}

/// Measures a project into snapshots.
pub struct Measurer<'a> {
    cfg: &'a TrackerConfig,
    tests: &'a dyn TestRunner,
}

impl<'a> Measurer<'a> {
    /// Create a measurer over `cfg`, running tests through `tests`.
    #[must_use]
    pub fn new(cfg: &'a TrackerConfig, tests: &'a dyn TestRunner) -> Self {
        Self { cfg, tests }
    }

    /// Measure one subsystem into a [`SubsystemMetric`].
    ///
    /// # Errors
    /// Propagates LOC / stub / test errors.
    pub fn measure_subsystem(&self, name: &str) -> crate::Result<SubsystemMetric> {
        let sub = self
            .cfg
            .subsystems
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| TrackerError::NotFound(format!("subsystem `{name}`")))?;

        // Upstream LOC across referenced clone sub-directories.
        let mut upstream_loc = 0u64;
        for r in &sub.upstreams {
            let clone = self.cfg.clone_dir(&r.name);
            if !clone.exists() {
                continue; // not polled yet; counts as zero, honestly
            }
            let report = loc::count_subpaths(&clone, &r.subpaths)?;
            let langs = self.cfg.ref_languages(r);
            let lang_refs: Vec<&str> = langs.iter().map(String::as_str).collect();
            upstream_loc += if lang_refs.is_empty() {
                report.total_code()
            } else {
                report.code_for(&lang_refs)
            };
        }

        // Port LOC + stubs + tests across the subsystem's crates.
        let root = self.cfg.root_path();
        let mut port_loc = 0u64;
        let mut stubs = StubCount::default();
        let mut outcome = TestOutcome::default();
        for crate_rel in &sub.port_crates {
            let crate_dir = root.join(crate_rel);
            if !crate_dir.exists() {
                continue;
            }
            port_loc += loc::count_dir(&crate_dir)?.code_for(&["rust"]);
            stubs.accumulate(stubs::count_stubs(&crate_dir)?);
            outcome.add(self.tests.test(&crate_dir)?);
        }

        Ok(SubsystemMetric::derive(
            sub.name.clone(),
            sub.group.clone(),
            upstream_loc,
            port_loc,
            !sub.upstreams.is_empty(),
            outcome.passed,
            outcome.failed,
            outcome.ignored,
            stubs,
        ))
    }

    /// Measure every subsystem into a full [`Snapshot`].
    ///
    /// `date` is `YYYY-MM-DD`; `generated_at` is an RFC3339 timestamp. They are
    /// passed in (rather than read from the clock) so the run is reproducible
    /// and testable.
    ///
    /// # Errors
    /// Propagates measurement errors from any subsystem.
    pub fn measure_all(&self, date: &str, generated_at: &str) -> crate::Result<Snapshot> {
        let mut subsystems = Vec::with_capacity(self.cfg.subsystems.len());
        for sub in &self.cfg.subsystems {
            subsystems.push(self.measure_subsystem(&sub.name)?);
        }
        Ok(Snapshot {
            project: self.cfg.project.clone(),
            date: date.to_owned(),
            generated_at: generated_at.to_owned(),
            subsystems,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_result_line() {
        let out = "test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out";
        let o = parse_cargo_test(out);
        assert_eq!(
            o,
            TestOutcome {
                passed: 12,
                failed: 0,
                ignored: 1
            }
        );
    }

    #[test]
    fn sums_multiple_result_lines() {
        let out = "\
running 3 tests
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
running 5 tests
test result: FAILED. 4 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out
";
        let o = parse_cargo_test(out);
        assert_eq!(
            o,
            TestOutcome {
                passed: 7,
                failed: 1,
                ignored: 2
            }
        );
    }

    #[test]
    fn parse_handles_no_results() {
        assert_eq!(parse_cargo_test("nothing here"), TestOutcome::default());
    }

    struct FixedTests(TestOutcome);
    impl TestRunner for FixedTests {
        fn test(&self, _dir: &Path) -> crate::Result<TestOutcome> {
            Ok(self.0)
        }
    }

    fn write_crate(root: &Path, rel: &str, rust: &str) {
        let dir = root.join(rel).join("src");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join(rel).join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(dir.join("lib.rs"), rust).unwrap();
    }

    #[test]
    fn measures_subsystem_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let work = root.join("work");

        // Fake upstream clone with 4 lines of Go.
        let clone = work.join("upstreams/kubernetes/pkg");
        std::fs::create_dir_all(&clone).unwrap();
        std::fs::write(
            clone.join("a.go"),
            "package a\nfunc A() {}\nfunc B() {}\nfunc C() {}\n",
        )
        .unwrap();

        // Port crate with 2 lines of Rust, one todo!.
        write_crate(
            root,
            "crates/port",
            "pub fn a() {}\npub fn b() { todo!() }\n",
        );

        let yaml = format!(
            r"
project: cave-home
root: {root}
work_dir: {work}
upstreams:
  - name: kubernetes
    repo: https://example.invalid/k
    languages: [go]
subsystems:
  - name: apiserver
    group: k3s
    port_crates: [crates/port]
    upstreams:
      - name: kubernetes
        subpaths: [pkg]
",
            root = root.display(),
            work = work.display(),
        );
        let cfg = TrackerConfig::from_yaml_str(&yaml).unwrap();
        let tests = FixedTests(TestOutcome {
            passed: 9,
            failed: 1,
            ignored: 0,
        });
        let m = Measurer::new(&cfg, &tests);
        let metric = m.measure_subsystem("apiserver").unwrap();

        assert_eq!(metric.upstream_loc, 4, "4 go code lines");
        assert_eq!(
            metric.port_loc, 2,
            "2 rust code lines (incl. the todo line)"
        );
        assert_eq!(metric.stubs.todo, 1);
        assert_eq!(metric.tests_passed, 9);
        assert_eq!(metric.tests_failed, 1);
        // real_pct must match the honest formula for exactly these measured
        // inputs (here the dense stub in a 2-line port drives integrity to 0).
        let expected = crate::honest::real_pct(crate::honest::CompletionInputs {
            upstream_loc: 4,
            port_loc: 2,
            has_upstream: true,
            tests_passed: 9,
            tests_failed: 1,
            stubs: StubCount {
                todo: 1,
                ..StubCount::default()
            },
        });
        assert!((metric.real_pct - expected).abs() < 1e-9);
    }

    #[test]
    fn measure_all_builds_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_crate(root, "crates/port", "pub fn a() {}\n");
        let yaml = format!(
            r"
project: cave-home
root: {root}
work_dir: {work}
upstreams: []
subsystems:
  - name: first-party
    group: smart-home
    port_crates: [crates/port]
",
            root = root.display(),
            work = root.join("w").display(),
        );
        let cfg = TrackerConfig::from_yaml_str(&yaml).unwrap();
        let tests = FixedTests(TestOutcome {
            passed: 5,
            failed: 0,
            ignored: 0,
        });
        let m = Measurer::new(&cfg, &tests);
        let snap = m.measure_all("2026-06-07", "2026-06-07T06:00:00Z").unwrap();
        assert_eq!(snap.project, "cave-home");
        assert_eq!(snap.subsystems.len(), 1);
        // first-party (no upstream) + has code + tests pass + no stubs => 100%.
        assert!((snap.subsystems[0].real_pct - 100.0).abs() < 1e-9);
    }
}
