// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! Polling upstream repositories.
//!
//! All git access goes through the [`GitRunner`] trait so the polling logic in
//! [`poll`] is unit-testable with a [`MockGit`], while production uses
//! [`ShellGit`] which shells out to the real `git` binary (`git clone
//! --depth 1`, `git fetch`, `git describe`).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::TrackerConfig;

/// Abstraction over the git operations the tracker needs.
pub trait GitRunner {
    /// Shallow-clone `repo` into `dest`, or fetch if `dest` already exists.
    /// When `tag` is set, that ref is checked out.
    ///
    /// # Errors
    /// Returns an error if the underlying git invocation fails.
    fn sync(&self, repo: &str, dest: &Path, tag: Option<&str>) -> crate::Result<()>;

    /// Resolve the current `HEAD` commit hash in `dest`.
    ///
    /// # Errors
    /// Returns an error if `git rev-parse` fails.
    fn head_commit(&self, dest: &Path) -> crate::Result<String>;

    /// The most recent tag reachable from `HEAD`, if any.
    ///
    /// # Errors
    /// Returns an error only on an unexpected git failure; a repo with no tags
    /// yields `Ok(None)`.
    fn latest_tag(&self, dest: &Path) -> crate::Result<Option<String>>;
}

/// Outcome of polling one upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollResult {
    /// Upstream name from the config.
    pub name: String,
    /// Clone directory.
    pub dest: PathBuf,
    /// Resolved `HEAD` commit.
    pub head_commit: String,
    /// Latest tag, if any.
    pub latest_tag: Option<String>,
}

/// Poll a single upstream by name.
///
/// # Errors
/// Returns [`TrackerError::NotFound`](crate::TrackerError::NotFound) for an
/// unknown name, or propagates git errors.
pub fn poll_one(cfg: &TrackerConfig, git: &dyn GitRunner, name: &str) -> crate::Result<PollResult> {
    let upstream = cfg
        .upstream(name)
        .ok_or_else(|| crate::TrackerError::NotFound(format!("upstream `{name}`")))?;
    let dest = cfg.clone_dir(name);
    git.sync(&upstream.repo, &dest, upstream.tag.as_deref())?;
    let head_commit = git.head_commit(&dest)?;
    let latest_tag = git.latest_tag(&dest)?;
    Ok(PollResult {
        name: name.to_owned(),
        dest,
        head_commit,
        latest_tag,
    })
}

/// Poll every upstream in the config. Errors on individual upstreams are
/// returned alongside successes so one bad repo does not abort the run.
#[must_use]
pub fn poll_all(
    cfg: &TrackerConfig,
    git: &dyn GitRunner,
) -> Vec<(String, crate::Result<PollResult>)> {
    cfg.upstreams
        .iter()
        .map(|u| (u.name.clone(), poll_one(cfg, git, &u.name)))
        .collect()
}

/// Real git, via the `git` CLI.
#[derive(Debug, Clone, Default)]
pub struct ShellGit;

impl ShellGit {
    /// Construct the real git runner.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

fn run_git(args: &[&str]) -> crate::Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| crate::TrackerError::io(PathBuf::from("git"), e))?;
    if !output.status.success() {
        return Err(crate::TrackerError::command(
            format!("git {}", args.join(" ")),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl GitRunner for ShellGit {
    fn sync(&self, repo: &str, dest: &Path, tag: Option<&str>) -> crate::Result<()> {
        let dest_str = dest.to_string_lossy().into_owned();
        if dest.join(".git").is_dir() {
            // Existing clone: fetch the latest shallow snapshot and reset.
            run_git(&[
                "-C", &dest_str, "fetch", "--depth", "1", "--tags", "--force", "origin",
            ])?;
            if let Some(tag) = tag {
                run_git(&["-C", &dest_str, "checkout", "--force", tag])?;
            } else {
                // Move to the freshly fetched tip.
                run_git(&["-C", &dest_str, "reset", "--hard", "FETCH_HEAD"])?;
            }
            return Ok(());
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::TrackerError::io(parent, e))?;
        }
        let mut args = vec!["clone", "--depth", "1"];
        if let Some(tag) = tag {
            args.push("--branch");
            args.push(tag);
        }
        args.push(repo);
        args.push(&dest_str);
        run_git(&args)?;
        Ok(())
    }

    fn head_commit(&self, dest: &Path) -> crate::Result<String> {
        run_git(&["-C", &dest.to_string_lossy(), "rev-parse", "HEAD"])
    }

    fn latest_tag(&self, dest: &Path) -> crate::Result<Option<String>> {
        let dest_str = dest.to_string_lossy().into_owned();
        match run_git(&["-C", &dest_str, "describe", "--tags", "--abbrev=0"]) {
            Ok(tag) if !tag.is_empty() => Ok(Some(tag)),
            // Empty output, or `describe` exiting non-zero because no tag is
            // reachable: both mean "no tag", not an error.
            Ok(_) | Err(crate::TrackerError::Command { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct MockGit {
        synced: RefCell<Vec<(String, PathBuf, Option<String>)>>,
    }

    impl GitRunner for MockGit {
        fn sync(&self, repo: &str, dest: &Path, tag: Option<&str>) -> crate::Result<()> {
            self.synced.borrow_mut().push((
                repo.to_owned(),
                dest.to_path_buf(),
                tag.map(str::to_owned),
            ));
            Ok(())
        }
        fn head_commit(&self, _dest: &Path) -> crate::Result<String> {
            Ok("deadbeef".to_owned())
        }
        fn latest_tag(&self, _dest: &Path) -> crate::Result<Option<String>> {
            Ok(Some("v1.2.3".to_owned()))
        }
    }

    fn cfg() -> TrackerConfig {
        TrackerConfig::from_yaml_str(
            r"
project: t
work_dir: /tmp/tracker-test
upstreams:
  - name: k3s
    repo: https://example.invalid/k3s
    languages: [go]
  - name: pinned
    repo: https://example.invalid/pinned
    tag: v9.9.9
subsystems: []
",
        )
        .unwrap()
    }

    #[test]
    fn poll_one_records_sync_and_resolves() {
        let cfg = cfg();
        let git = MockGit::default();
        let res = poll_one(&cfg, &git, "k3s").unwrap();
        assert_eq!(res.head_commit, "deadbeef");
        assert_eq!(res.latest_tag.as_deref(), Some("v1.2.3"));
        let synced = git.synced.borrow();
        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].0, "https://example.invalid/k3s");
        assert_eq!(synced[0].2, None);
    }

    #[test]
    fn poll_one_passes_tag() {
        let cfg = cfg();
        let git = MockGit::default();
        poll_one(&cfg, &git, "pinned").unwrap();
        assert_eq!(git.synced.borrow()[0].2.as_deref(), Some("v9.9.9"));
    }

    #[test]
    fn poll_unknown_is_not_found() {
        let cfg = cfg();
        let git = MockGit::default();
        assert!(matches!(
            poll_one(&cfg, &git, "ghost"),
            Err(crate::TrackerError::NotFound(_))
        ));
    }

    #[test]
    fn poll_all_visits_every_upstream() {
        let cfg = cfg();
        let git = MockGit::default();
        let results = poll_all(&cfg, &git);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, r)| r.is_ok()));
    }

    /// Real `git`: clone shallowly from a local source repo (no network).
    #[test]
    fn shell_git_clones_real_local_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let s = src.to_string_lossy().into_owned();
        // Build a tiny source repo with one tagged commit.
        for args in [
            vec!["-C", &s, "init", "-q"],
            vec!["-C", &s, "config", "user.email", "t@t"],
            vec!["-C", &s, "config", "user.name", "t"],
        ] {
            run_git(&args).unwrap();
        }
        std::fs::write(src.join("f.go"), "package main\nfunc main() {}\n").unwrap();
        run_git(&["-C", &s, "add", "."]).unwrap();
        run_git(&["-C", &s, "commit", "-q", "-m", "init"]).unwrap();
        run_git(&["-C", &s, "tag", "v0.1.0"]).unwrap();

        let dest = tmp.path().join("clone");
        let git = ShellGit::new();
        let url = format!("file://{s}");
        git.sync(&url, &dest, None).unwrap();
        assert!(dest.join("f.go").exists(), "file was cloned");
        let head = git.head_commit(&dest).unwrap();
        assert_eq!(head.len(), 40, "full sha resolved");
        // Tag presence depends on shallow fetch semantics; just assert no error.
        let _ = git.latest_tag(&dest).unwrap();

        // Second sync on the existing clone must succeed (fetch path).
        git.sync(&url, &dest, None).unwrap();
    }
}
