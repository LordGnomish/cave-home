//! Version-compatibility policy (Charter §7 always-latest, Charter §8
//! no-backcompat).
//!
//! cave-home runs one version across the whole cluster: a node only joins, and
//! peers only trust each other, when they are on a **compatible** version. The
//! policy here is deliberately strict to match the Charter:
//!
//! - Same `major.minor` → fully [`Compatibility::Compatible`]. Patch releases
//!   are always interchangeable (bug fixes only).
//! - Same major, peer one minor behind/ahead → [`Compatibility::Upgrade`]:
//!   they can interoperate for the moment but the lagging node should be
//!   rolled forward (always-latest). Surfaced as a gentle nudge, not an error.
//! - Different major, or a minor gap of more than one →
//!   [`Compatibility::Incompatible`]: refuse. There is no back-compat mode
//!   (Charter §8); the operator must bring both nodes to the current line.
//!
//! Versions are parsed from the `v` TXT key (semantic `major.minor.patch`).

/// A parsed semantic version (`major.minor.patch`). Pre-release / build
/// metadata suffixes are not modelled in Phase 1 (deferred).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// Why a version string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    /// The string was empty.
    Empty,
    /// A component was missing (need exactly `major.minor.patch`).
    WrongComponentCount(usize),
    /// A component was not a base-10 unsigned integer.
    NotNumeric(String),
}

impl core::fmt::Display for VersionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("version string is empty"),
            Self::WrongComponentCount(n) => {
                write!(f, "version needs 3 components (major.minor.patch), got {n}")
            }
            Self::NotNumeric(s) => write!(f, "version component {s:?} is not a number"),
        }
    }
}

impl std::error::Error for VersionError {}

impl Version {
    /// Parse a `major.minor.patch` string (e.g. `"1.4.0"`).
    ///
    /// A leading `v` is tolerated (`"v1.4.0"`).
    ///
    /// # Errors
    /// [`VersionError`] if empty, not exactly three dot-separated components,
    /// or any component is non-numeric.
    pub fn parse(s: &str) -> Result<Self, VersionError> {
        let s = s.strip_prefix('v').unwrap_or(s);
        if s.is_empty() {
            return Err(VersionError::Empty);
        }
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(VersionError::WrongComponentCount(parts.len()));
        }
        let parse_one = |p: &str| {
            p.parse::<u32>()
                .map_err(|_| VersionError::NotNumeric(p.to_owned()))
        };
        Ok(Self {
            major: parse_one(parts[0])?,
            minor: parse_one(parts[1])?,
            patch: parse_one(parts[2])?,
        })
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The verdict of comparing this node's version against a peer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    /// Same `major.minor` — interchangeable.
    Compatible,
    /// One minor version apart (same major) — works now, but one side should
    /// roll forward to keep the cluster on the latest line (Charter §7).
    Upgrade,
    /// Different major, or more than one minor apart — refuse (Charter §8 has
    /// no back-compat mode).
    Incompatible,
}

impl Compatibility {
    /// Whether peers in this state may interoperate at all.
    #[must_use]
    pub const fn may_interoperate(self) -> bool {
        matches!(self, Self::Compatible | Self::Upgrade)
    }
}

/// Compare `ours` against a `peer` version and apply the cluster policy.
#[must_use]
pub fn compatibility(ours: Version, peer: Version) -> Compatibility {
    if ours.major != peer.major {
        return Compatibility::Incompatible;
    }
    let gap = ours.minor.abs_diff(peer.minor);
    match gap {
        0 => Compatibility::Compatible,
        1 => Compatibility::Upgrade,
        _ => Compatibility::Incompatible,
    }
}

/// Convenience: may these two versions interoperate at all?
#[must_use]
pub fn version_is_compatible(ours: Version, peer: Version) -> bool {
    compatibility(ours, peer).may_interoperate()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).expect("valid test version")
    }

    #[test]
    fn parses_basic_version() {
        assert_eq!(v("1.4.0"), Version { major: 1, minor: 4, patch: 0 });
    }

    #[test]
    fn parses_v_prefix() {
        assert_eq!(v("v2.10.3"), Version { major: 2, minor: 10, patch: 3 });
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(Version::parse(""), Err(VersionError::Empty));
        assert_eq!(Version::parse("v"), Err(VersionError::Empty));
    }

    #[test]
    fn parse_rejects_wrong_component_count() {
        assert_eq!(
            Version::parse("1.4"),
            Err(VersionError::WrongComponentCount(2))
        );
        assert_eq!(
            Version::parse("1.4.0.5"),
            Err(VersionError::WrongComponentCount(4))
        );
    }

    #[test]
    fn parse_rejects_non_numeric() {
        assert_eq!(
            Version::parse("1.x.0"),
            Err(VersionError::NotNumeric("x".to_owned()))
        );
    }

    #[test]
    fn same_minor_is_compatible_regardless_of_patch() {
        assert_eq!(compatibility(v("1.4.0"), v("1.4.9")), Compatibility::Compatible);
        assert!(version_is_compatible(v("1.4.0"), v("1.4.9")));
    }

    #[test]
    fn one_minor_gap_is_upgrade_nudge() {
        assert_eq!(compatibility(v("1.5.0"), v("1.4.2")), Compatibility::Upgrade);
        assert_eq!(compatibility(v("1.4.2"), v("1.5.0")), Compatibility::Upgrade);
        assert!(Compatibility::Upgrade.may_interoperate());
    }

    #[test]
    fn two_minor_gap_is_incompatible() {
        assert_eq!(
            compatibility(v("1.6.0"), v("1.4.0")),
            Compatibility::Incompatible
        );
        assert!(!version_is_compatible(v("1.6.0"), v("1.4.0")));
    }

    #[test]
    fn different_major_is_incompatible() {
        assert_eq!(
            compatibility(v("2.0.0"), v("1.99.0")),
            Compatibility::Incompatible
        );
        assert!(!Compatibility::Incompatible.may_interoperate());
    }
}
