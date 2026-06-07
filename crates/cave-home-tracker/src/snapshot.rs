// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! The measured state of a project at a point in time.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::honest;
use crate::stubs::StubCount;

/// One subsystem's measured metrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubsystemMetric {
    /// Subsystem name (matches the config).
    pub name: String,
    /// Group it rolls up into (e.g. `"k3s"`, `"smart-home"`).
    pub group: String,
    /// Upstream source LOC being ported.
    pub upstream_loc: u64,
    /// cave-home port source LOC (Rust).
    pub port_loc: u64,
    /// `port_loc / upstream_loc`, capped at 1.0.
    pub ratio: f64,
    /// Port tests that passed.
    pub tests_passed: u64,
    /// Port tests that failed.
    pub tests_failed: u64,
    /// Port tests that were ignored.
    pub tests_ignored: u64,
    /// Fraction of run tests that passed.
    pub test_pass_rate: f64,
    /// Stub markers found in the port.
    pub stubs: StubCount,
    /// Honest completion percentage in `[0, 100]`.
    pub real_pct: f64,
}

impl SubsystemMetric {
    /// Build a metric from raw measurements, deriving ratio / pass-rate /
    /// real-% via the [`honest`] formula so callers cannot fudge them.
    #[must_use]
    #[allow(clippy::too_many_arguments)] // these are the raw measured inputs
    pub fn derive(
        name: impl Into<String>,
        group: impl Into<String>,
        upstream_loc: u64,
        port_loc: u64,
        has_upstream: bool,
        tests_passed: u64,
        tests_failed: u64,
        tests_ignored: u64,
        stubs: StubCount,
    ) -> Self {
        let ratio = honest::coverage(upstream_loc, port_loc, has_upstream);
        let test_pass_rate = honest::quality(tests_passed, tests_failed);
        let real_pct = honest::real_pct(honest::CompletionInputs {
            upstream_loc,
            port_loc,
            has_upstream,
            tests_passed,
            tests_failed,
            stubs,
        });
        Self {
            name: name.into(),
            group: group.into(),
            upstream_loc,
            port_loc,
            ratio,
            tests_passed,
            tests_failed,
            tests_ignored,
            test_pass_rate,
            stubs,
            real_pct,
        }
    }

    /// Total tests run (passed + failed; ignored excluded).
    #[must_use]
    pub const fn tests_run(&self) -> u64 {
        self.tests_passed + self.tests_failed
    }
}

/// All subsystem metrics measured in a single `measure` run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Project name (`cave-home`, `cave-runtime`, …).
    pub project: String,
    /// Calendar date `YYYY-MM-DD` the snapshot represents.
    pub date: String,
    /// RFC3339 timestamp the snapshot was generated.
    pub generated_at: String,
    /// Per-subsystem metrics.
    pub subsystems: Vec<SubsystemMetric>,
}

impl Snapshot {
    /// Weighted overall completion across every subsystem.
    #[must_use]
    pub fn overall_real_pct(&self) -> f64 {
        honest::aggregate_real_pct(self.subsystems.iter())
    }

    /// Weighted completion for one group only.
    #[must_use]
    pub fn group_real_pct(&self, group: &str) -> f64 {
        honest::aggregate_real_pct(self.subsystems.iter().filter(|m| m.group == group))
    }

    /// Weighted completion for everything *not* in `group` (e.g. the
    /// smart-home rollup is "everything that is not k3s").
    #[must_use]
    pub fn complement_real_pct(&self, group: &str) -> f64 {
        honest::aggregate_real_pct(self.subsystems.iter().filter(|m| m.group != group))
    }

    /// Distinct group names, in first-seen order.
    #[must_use]
    pub fn groups(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for m in &self.subsystems {
            if !seen.contains(&m.group) {
                seen.push(m.group.clone());
            }
        }
        seen
    }

    /// Look up a subsystem by name.
    #[must_use]
    pub fn subsystem(&self, name: &str) -> Option<&SubsystemMetric> {
        self.subsystems.iter().find(|m| m.name == name)
    }

    /// Serialise to pretty JSON.
    ///
    /// # Errors
    /// Propagates any `serde_json` serialisation error.
    pub fn to_json(&self) -> crate::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parse from JSON.
    ///
    /// # Errors
    /// Propagates any `serde_json` deserialisation error.
    pub fn from_json(s: &str) -> crate::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// File name a snapshot is stored under in the snapshots directory.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("snapshot-{}.json", self.date)
    }
}

/// On-disk store of dated snapshots under `dir`.
#[derive(Debug, Clone)]
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    /// Open (creating if needed) a store rooted at `dir`.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be created.
    pub fn open(dir: impl Into<PathBuf>) -> crate::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(|e| crate::TrackerError::io(&dir, e))?;
        Ok(Self { dir })
    }

    /// Write a snapshot, named by its date.
    ///
    /// # Errors
    /// Returns an error if serialisation or the write fails.
    pub fn save(&self, snap: &Snapshot) -> crate::Result<PathBuf> {
        let path = self.dir.join(snap.file_name());
        let json = snap.to_json()?;
        std::fs::write(&path, json).map_err(|e| crate::TrackerError::io(&path, e))?;
        Ok(path)
    }

    /// Load every snapshot, sorted ascending by date.
    ///
    /// # Errors
    /// Returns an error if the store directory cannot be listed.
    pub fn load_all(&self) -> crate::Result<Vec<Snapshot>> {
        let mut out = Vec::new();
        let entries =
            std::fs::read_dir(&self.dir).map_err(|e| crate::TrackerError::io(&self.dir, e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let named_snapshot = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("snapshot-"));
            let is_json = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"));
            if !(named_snapshot && is_json) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(snap) = Snapshot::from_json(&text) {
                    out.push(snap);
                }
            }
        }
        out.sort_by(|a, b| a.date.cmp(&b.date));
        Ok(out)
    }

    /// The most recent snapshot strictly before `date`, if any.
    ///
    /// # Errors
    /// Propagates errors from [`SnapshotStore::load_all`].
    pub fn previous(&self, date: &str) -> crate::Result<Option<Snapshot>> {
        let all = self.load_all()?;
        Ok(all.into_iter().rfind(|s| s.date.as_str() < date))
    }

    /// Load a snapshot for an exact date.
    ///
    /// # Errors
    /// Propagates read/parse errors; returns `Ok(None)` if absent.
    pub fn load(&self, date: &str) -> crate::Result<Option<Snapshot>> {
        let path = self.dir.join(format!("snapshot-{date}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path).map_err(|e| crate::TrackerError::io(&path, e))?;
        Ok(Some(Snapshot::from_json(&text)?))
    }

    /// The latest snapshot on disk, if any.
    ///
    /// # Errors
    /// Propagates errors from [`SnapshotStore::load_all`].
    pub fn latest(&self) -> crate::Result<Option<Snapshot>> {
        Ok(self.load_all()?.pop())
    }

    /// The directory this store writes to.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(name: &str, group: &str, real: f64, upstream: u64) -> SubsystemMetric {
        SubsystemMetric {
            real_pct: real,
            upstream_loc: upstream,
            group: group.into(),
            name: name.into(),
            ..SubsystemMetric::default()
        }
    }

    #[test]
    fn derive_computes_fields() {
        let m = SubsystemMetric::derive(
            "kine",
            "k3s",
            1000,
            500,
            true,
            8,
            2,
            1,
            StubCount::default(),
        );
        assert!((m.ratio - 0.5).abs() < 1e-9);
        assert!((m.test_pass_rate - 0.8).abs() < 1e-9);
        assert!((m.real_pct - 40.0).abs() < 1e-9);
        assert_eq!(m.tests_run(), 10);
    }

    #[test]
    fn group_and_overall_aggregates() {
        let snap = Snapshot {
            project: "cave-home".into(),
            date: "2026-06-07".into(),
            generated_at: "2026-06-07T06:00:00Z".into(),
            subsystems: vec![
                metric("kine", "k3s", 80.0, 1000),
                metric("apiserver", "k3s", 40.0, 1000),
                metric("hue", "smart-home", 100.0, 500),
            ],
        };
        assert!((snap.group_real_pct("k3s") - 60.0).abs() < 1e-9);
        assert!((snap.complement_real_pct("k3s") - 100.0).abs() < 1e-9);
        assert_eq!(
            snap.groups(),
            vec!["k3s".to_owned(), "smart-home".to_owned()]
        );
        assert!(snap.subsystem("hue").is_some());
    }

    #[test]
    fn json_round_trip() {
        let snap = Snapshot {
            project: "cave-home".into(),
            date: "2026-06-07".into(),
            generated_at: "2026-06-07T06:00:00Z".into(),
            subsystems: vec![metric("kine", "k3s", 50.0, 1000)],
        };
        let back = Snapshot::from_json(&snap.to_json().unwrap()).unwrap();
        assert_eq!(snap, back);
    }

    #[test]
    fn store_save_load_previous_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SnapshotStore::open(tmp.path()).unwrap();
        let mk = |date: &str, real: f64| Snapshot {
            project: "cave-home".into(),
            date: date.into(),
            generated_at: format!("{date}T06:00:00Z"),
            subsystems: vec![metric("kine", "k3s", real, 1000)],
        };
        store.save(&mk("2026-06-05", 10.0)).unwrap();
        store.save(&mk("2026-06-06", 20.0)).unwrap();
        store.save(&mk("2026-06-07", 30.0)).unwrap();

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].date, "2026-06-05");
        assert_eq!(store.latest().unwrap().unwrap().date, "2026-06-07");
        assert_eq!(
            store.previous("2026-06-07").unwrap().unwrap().date,
            "2026-06-06"
        );
        assert!(store.previous("2026-06-05").unwrap().is_none());
        assert_eq!(
            store.load("2026-06-06").unwrap().unwrap().date,
            "2026-06-06"
        );
        assert!(store.load("2099-01-01").unwrap().is_none());
    }
}
