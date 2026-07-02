// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! The `tracker.yaml` configuration model.
//!
//! Everything project-specific lives here, which is what makes one binary able
//! to track cave-home *and* cave-runtime: you point `--config` at a different
//! file. A config declares the **upstreams** to clone and the **subsystems**
//! (cave-home crates) to measure, each subsystem referencing the upstream
//! directories it ports.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A remote project we shallow-clone and measure against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upstream {
    /// Short handle referenced by subsystems (e.g. `"k3s"`, `"kubernetes"`).
    pub name: String,
    /// Clone URL.
    pub repo: String,
    /// Optional tag/branch to pin; `None` clones the default branch shallowly.
    #[serde(default)]
    pub tag: Option<String>,
    /// Languages that count toward this upstream's LOC (e.g. `["go"]`).
    #[serde(default)]
    pub languages: Vec<String>,
    /// Optional human note (e.g. "community-maintained spec, no code port").
    #[serde(default)]
    pub note: Option<String>,
}

/// A subsystem's reference into one upstream: which sub-directories it ports,
/// and (optionally) which languages to count there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamRef {
    /// Name of the [`Upstream`] this refers to.
    pub name: String,
    /// Sub-directories within the upstream clone to measure; empty = whole repo.
    #[serde(default)]
    pub subpaths: Vec<String>,
    /// Override languages for this reference; falls back to the upstream's.
    #[serde(default)]
    pub languages: Option<Vec<String>>,
}

/// A cave-home subsystem (one or more crates) and the upstream surface it ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subsystem {
    /// Subsystem name, also used as the snapshot/metric key.
    pub name: String,
    /// Rollup group: `"k3s"`, `"smart-home"`, … (free-form).
    pub group: String,
    /// Port crate directories, relative to the project `root`.
    pub port_crates: Vec<String>,
    /// Upstream surfaces this subsystem ports (may be empty for first-party).
    #[serde(default)]
    pub upstreams: Vec<UpstreamRef>,
}

/// A whole `tracker.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerConfig {
    /// Project name, stamped into every snapshot.
    pub project: String,
    /// Project source root (where `port_crates` are resolved). Default `.`.
    #[serde(default = "default_root")]
    pub root: String,
    /// Where clones, snapshots and reports live. `~` is expanded.
    pub work_dir: String,
    /// Tracked upstreams.
    pub upstreams: Vec<Upstream>,
    /// Tracked subsystems.
    pub subsystems: Vec<Subsystem>,
}

fn default_root() -> String {
    ".".to_owned()
}

impl TrackerConfig {
    /// Parse a config from YAML text.
    ///
    /// # Errors
    /// Returns [`TrackerError::Config`](crate::TrackerError::Config) on invalid
    /// YAML, or [`TrackerError::NotFound`](crate::TrackerError::NotFound) if a
    /// subsystem references an unknown upstream.
    pub fn from_yaml_str(s: &str) -> crate::Result<Self> {
        let cfg: Self = serde_yaml::from_str(s)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load and parse a config from a file.
    ///
    /// # Errors
    /// Propagates I/O and parse errors.
    pub fn from_path(path: &Path) -> crate::Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| crate::TrackerError::io(path, e))?;
        Self::from_yaml_str(&text)
    }

    /// Validate cross-references: every subsystem upstream must exist.
    fn validate(&self) -> crate::Result<()> {
        let known: Vec<&str> = self.upstreams.iter().map(|u| u.name.as_str()).collect();
        for sub in &self.subsystems {
            for r in &sub.upstreams {
                if !known.contains(&r.name.as_str()) {
                    return Err(crate::TrackerError::NotFound(format!(
                        "subsystem `{}` references unknown upstream `{}`",
                        sub.name, r.name
                    )));
                }
            }
        }
        Ok(())
    }

    /// Look up an upstream by name.
    #[must_use]
    pub fn upstream(&self, name: &str) -> Option<&Upstream> {
        self.upstreams.iter().find(|u| u.name == name)
    }

    /// Expanded work directory (resolves a leading `~`).
    #[must_use]
    pub fn work_dir_path(&self) -> PathBuf {
        expand_tilde(&self.work_dir)
    }

    /// Project root, expanded.
    #[must_use]
    pub fn root_path(&self) -> PathBuf {
        expand_tilde(&self.root)
    }

    /// Directory upstream clones live in.
    #[must_use]
    pub fn clones_dir(&self) -> PathBuf {
        self.work_dir_path().join("upstreams")
    }

    /// Directory snapshots live in.
    #[must_use]
    pub fn snapshots_dir(&self) -> PathBuf {
        self.work_dir_path().join("snapshots")
    }

    /// Clone directory for a specific upstream.
    #[must_use]
    pub fn clone_dir(&self, upstream: &str) -> PathBuf {
        self.clones_dir().join(upstream)
    }

    /// Effective languages for an upstream reference (ref override, else the
    /// upstream's declared languages).
    #[must_use]
    pub fn ref_languages(&self, r: &UpstreamRef) -> Vec<String> {
        if let Some(langs) = &r.languages {
            return langs.clone();
        }
        self.upstream(&r.name)
            .map(|u| u.languages.clone())
            .unwrap_or_default()
    }

    /// Map of upstream name -> resolved clone directory, for `poll`.
    #[must_use]
    pub fn clone_targets(&self) -> HashMap<String, PathBuf> {
        self.upstreams
            .iter()
            .map(|u| (u.name.clone(), self.clone_dir(&u.name)))
            .collect()
    }
}

/// Expand a leading `~` or `~/` to the user's home directory (from `$HOME`).
#[must_use]
pub fn expand_tilde(path: &str) -> PathBuf {
    expand_tilde_with(path, home_dir().as_deref())
}

/// Pure tilde expansion against an explicit `home` (testable without mutating
/// process environment, which is `unsafe` under edition 2024).
#[must_use]
pub fn expand_tilde_with(path: &str, home: Option<&Path>) -> PathBuf {
    match (path, home) {
        ("~", Some(home)) => home.to_path_buf(),
        (p, Some(home)) if p.starts_with("~/") => home.join(&p[2..]),
        _ => PathBuf::from(path),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r"
project: cave-home
root: .
work_dir: ~/.cache/cave-home-tracker
upstreams:
  - name: k3s
    repo: https://github.com/k3s-io/k3s
    languages: [go]
  - name: kubernetes
    repo: https://github.com/kubernetes/kubernetes
    tag: v1.30.0
    languages: [go]
subsystems:
  - name: kine
    group: k3s
    port_crates: [crates/cave-home-kine-rs]
    upstreams:
      - name: kubernetes
        subpaths: [staging/src/k8s.io/apiserver]
        languages: [go]
  - name: apiserver
    group: k3s
    port_crates: [crates/cave-home-apiserver-rs]
    upstreams:
      - name: kubernetes
";

    #[test]
    fn parses_sample() {
        let cfg = TrackerConfig::from_yaml_str(SAMPLE).unwrap();
        assert_eq!(cfg.project, "cave-home");
        assert_eq!(cfg.upstreams.len(), 2);
        assert_eq!(cfg.subsystems.len(), 2);
        assert_eq!(
            cfg.upstream("kubernetes").unwrap().tag.as_deref(),
            Some("v1.30.0")
        );
    }

    #[test]
    fn rejects_unknown_upstream() {
        let bad = r"
project: x
work_dir: /tmp/x
upstreams: []
subsystems:
  - name: kine
    group: k3s
    port_crates: [crates/cave-home-kine-rs]
    upstreams:
      - name: ghost
";
        let err = TrackerConfig::from_yaml_str(bad).unwrap_err();
        assert!(matches!(err, crate::TrackerError::NotFound(_)));
    }

    #[test]
    fn ref_languages_falls_back_to_upstream() {
        let cfg = TrackerConfig::from_yaml_str(SAMPLE).unwrap();
        let apiserver = &cfg.subsystems[1];
        let r = &apiserver.upstreams[0];
        assert_eq!(cfg.ref_languages(r), vec!["go".to_owned()]);
    }

    #[test]
    fn tilde_expansion() {
        let home = Path::new("/home/tester");
        assert_eq!(
            expand_tilde_with("~", Some(home)),
            PathBuf::from("/home/tester")
        );
        assert_eq!(
            expand_tilde_with("~/.cache/x", Some(home)),
            PathBuf::from("/home/tester/.cache/x")
        );
        assert_eq!(
            expand_tilde_with("/abs/path", Some(home)),
            PathBuf::from("/abs/path")
        );
        // No home available: leave the path untouched.
        assert_eq!(expand_tilde_with("~/x", None), PathBuf::from("~/x"));
    }

    #[test]
    fn derived_dirs() {
        // Absolute work_dir needs no tilde expansion, so the assertion is
        // independent of the test runner's $HOME.
        let cfg = TrackerConfig::from_yaml_str(
            r"
project: cave-home
work_dir: /var/lib/cave-home-tracker
upstreams: []
subsystems: []
",
        )
        .unwrap();
        assert_eq!(
            cfg.work_dir_path(),
            PathBuf::from("/var/lib/cave-home-tracker")
        );
        assert_eq!(
            cfg.clone_dir("k3s"),
            PathBuf::from("/var/lib/cave-home-tracker/upstreams/k3s")
        );
        assert!(cfg.snapshots_dir().ends_with("snapshots"));
    }
}
