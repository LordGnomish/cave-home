// SPDX-License-Identifier: Apache-2.0
//! Build / version information.
//!
//! Honest provenance (Charter §6): the git sha is read from the build
//! environment **iff** it was supplied, otherwise it is reported as the literal
//! string `"unknown"`. No sha is ever fabricated. The crate version comes from
//! Cargo's `CARGO_PKG_VERSION`; the build profile from `debug_assertions`.

use crate::Component;

/// Captured build/version facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    /// Semantic version of the binary (the workspace `version`).
    pub version: &'static str,
    /// Git commit sha if the build environment provided one, else `"unknown"`.
    pub git_sha: String,
    /// `"debug"` or `"release"`.
    pub profile: &'static str,
    /// The pillars this build is able to run, by stable key.
    pub supported_pillars: Vec<&'static str>,
}

/// The sentinel used when no git sha was available at build time. Never a fake.
pub const UNKNOWN_SHA: &str = "unknown";

impl BuildInfo {
    /// Gather build info for the running binary.
    ///
    /// The git sha is taken from the optional `CAVE_HOME_GIT_SHA` build-time
    /// environment variable (set by the build pipeline); when it is absent or
    /// blank, [`UNKNOWN_SHA`] is reported — we do not invent one.
    #[must_use]
    pub fn current() -> Self {
        let raw_sha = option_env!("CAVE_HOME_GIT_SHA");
        Self::new(
            env!("CARGO_PKG_VERSION"),
            raw_sha,
            cfg!(debug_assertions),
        )
    }

    /// Construct build info from explicit inputs. Factored out so the
    /// sha-honesty rule and pillar list are unit-testable without a rebuild.
    #[must_use]
    pub fn new(version: &'static str, raw_sha: Option<&str>, debug: bool) -> Self {
        let git_sha = match raw_sha {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => UNKNOWN_SHA.to_string(),
        };
        let profile = if debug { "debug" } else { "release" };
        let supported_pillars = Component::ALL.iter().map(|c| c.key()).collect();
        Self {
            version,
            git_sha,
            profile,
            supported_pillars,
        }
    }

    /// Whether the git sha is the honest "unknown" sentinel.
    #[must_use]
    pub fn sha_is_unknown(&self) -> bool {
        self.git_sha == UNKNOWN_SHA
    }

    /// A short human-readable, jargon-free version line.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "cave-home {} ({}, {} build)",
            self.version, self.git_sha, self.profile
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_sha_reports_unknown_not_fabricated() {
        let info = BuildInfo::new("1.2.3", None, true);
        assert_eq!(info.git_sha, "unknown");
        assert!(info.sha_is_unknown());
    }

    #[test]
    fn blank_sha_reports_unknown() {
        let info = BuildInfo::new("1.2.3", Some("   "), true);
        assert!(info.sha_is_unknown());
    }

    #[test]
    fn provided_sha_is_used_and_trimmed() {
        let info = BuildInfo::new("1.2.3", Some(" abc123 "), false);
        assert_eq!(info.git_sha, "abc123");
        assert!(!info.sha_is_unknown());
    }

    #[test]
    fn profile_reflects_debug_flag() {
        assert_eq!(BuildInfo::new("0.0.0", None, true).profile, "debug");
        assert_eq!(BuildInfo::new("0.0.0", None, false).profile, "release");
    }

    #[test]
    fn supported_pillars_lists_every_component() {
        let info = BuildInfo::new("0.0.0", None, false);
        assert_eq!(info.supported_pillars.len(), Component::ALL.len());
        for c in Component::ALL {
            assert!(info.supported_pillars.contains(&c.key()));
        }
    }

    #[test]
    fn current_does_not_fabricate_a_sha() {
        // In this build environment CAVE_HOME_GIT_SHA is unset, so the sha must
        // be exactly the honest sentinel — never a made-up hex string.
        let info = BuildInfo::current();
        if info.sha_is_unknown() {
            assert_eq!(info.git_sha, UNKNOWN_SHA);
        } else {
            // If a real sha was injected it must be non-empty and not the word.
            assert!(!info.git_sha.is_empty());
            assert_ne!(info.git_sha, UNKNOWN_SHA);
        }
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn version_line_is_readable() {
        let info = BuildInfo::new("9.9.9", Some("deadbeef"), false);
        let line = info.line();
        assert!(line.contains("9.9.9"));
        assert!(line.contains("deadbeef"));
        assert!(line.contains("release"));
    }
}
