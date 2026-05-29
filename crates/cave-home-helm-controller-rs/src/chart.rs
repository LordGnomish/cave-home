// SPDX-License-Identifier: Apache-2.0
//! The `HelmChart` / `HelmChartConfig` CRD model, validation, and version/repo
//! resolution.
//!
//! Behavioural reimplementation of the k3s-io/helm-controller `HelmChart` CRD
//! (group `helm.cattle.io/v1`) from its **public CRD reference**. We model the
//! spec fields the reconcile core needs and the status fields it writes back.
//! The cluster-side watch/informer and apply are deferred (Phase 1b).
//!
//! Spec sources (public, Apache-2.0-compatible documentation):
//! * k3s-io/helm-controller public CRD docs (`HelmChart`, `HelmChartConfig`).
//! * Helm chart-repository spec (`index.yaml`, repo URL shapes, OCI `oci://`).
//! * Charter §7 (always-latest): an unset / `latest` version resolves to the
//!   newest available chart.

use std::collections::BTreeMap;

use crate::hash::{fnv1a64, short_hex};
use crate::values::Value;

/// Why a [`HelmChart`] spec was rejected. No panics — validation is total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    EmptyChart,
    EmptyTargetNamespace,
    BadVersion(String),
    BadRepo(String),
    EmptyJobImage,
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyChart => write!(f, "chart name must not be empty"),
            Self::EmptyTargetNamespace => write!(f, "targetNamespace must not be empty"),
            Self::BadVersion(v) => write!(f, "invalid chart version: {v}"),
            Self::BadRepo(r) => write!(f, "invalid repo URL: {r}"),
            Self::EmptyJobImage => write!(f, "jobImage must not be empty"),
        }
    }
}

impl std::error::Error for SpecError {}

/// How a chart version is pinned. Charter §7 favours always-latest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionPolicy {
    /// Resolve to the newest version the repo offers.
    Latest,
    /// Pin to an exact SemVer-shaped version string.
    Pinned(String),
}

impl VersionPolicy {
    /// Parse a spec `version` field. Empty or `"latest"` → [`VersionPolicy::Latest`].
    /// Any other value must look like a `SemVer` (`MAJOR.MINOR.PATCH`, optional
    /// leading `v`, optional pre-release/build suffix) or it is rejected.
    ///
    /// # Errors
    /// Returns [`SpecError::BadVersion`] for a non-empty, non-`latest` string
    /// that is not a plausible `SemVer`.
    pub fn parse(raw: &str) -> Result<Self, SpecError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("latest") {
            return Ok(Self::Latest);
        }
        let core = trimmed.strip_prefix('v').unwrap_or(trimmed);
        if is_semver_core(core) {
            Ok(Self::Pinned(core.to_string()))
        } else {
            Err(SpecError::BadVersion(raw.to_string()))
        }
    }

    #[must_use]
    pub const fn is_latest(&self) -> bool {
        matches!(self, Self::Latest)
    }
}

/// Validate a SemVer-ish core: `N.N.N` with optional `-pre` and `+build`.
fn is_semver_core(core: &str) -> bool {
    // Split off build metadata (+...) then pre-release (-...).
    let no_build = core.split('+').next().unwrap_or(core);
    let main = no_build.split('-').next().unwrap_or(no_build);
    let parts: Vec<&str> = main.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Validate and classify a chart `repo` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoKind {
    /// Classic HTTP(S) chart repository (`index.yaml` based).
    Http,
    /// OCI registry reference (`oci://...`).
    Oci,
}

/// A validated repo URL plus its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub url: String,
    pub kind: RepoKind,
}

impl Repo {
    /// Parse and validate a repo URL.
    ///
    /// Accepts `http://`, `https://` and `oci://`. An empty repo is allowed at
    /// the CRD level (a chart may be referenced by an absolute tarball URL in
    /// the `chart` field), so empties are represented as `None` by the caller;
    /// this constructor only runs on a non-empty string.
    ///
    /// # Errors
    /// Returns [`SpecError::BadRepo`] for an unsupported scheme or a URL with
    /// no host.
    pub fn parse(raw: &str) -> Result<Self, SpecError> {
        let raw = raw.trim();
        let (kind, rest) = if let Some(r) = raw.strip_prefix("https://") {
            (RepoKind::Http, r)
        } else if let Some(r) = raw.strip_prefix("http://") {
            (RepoKind::Http, r)
        } else if let Some(r) = raw.strip_prefix("oci://") {
            (RepoKind::Oci, r)
        } else {
            return Err(SpecError::BadRepo(raw.to_string()));
        };
        // Require a host component: at least one char before any `/`, and no
        // whitespace anywhere.
        let host = rest.split('/').next().unwrap_or("");
        if host.is_empty() || rest.contains(char::is_whitespace) {
            return Err(SpecError::BadRepo(raw.to_string()));
        }
        Ok(Self {
            url: raw.to_string(),
            kind,
        })
    }
}

/// `HelmChartConfig` — a values overlay keyed to a `HelmChart` of the same name.
///
/// helm-controller merges this *between* the chart's inline `valuesContent`
/// and the explicit `set` map (see [`crate::values::merge_layers`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelmChartConfig {
    /// Overlay values (parsed from the config's own `valuesContent`).
    pub values: Option<Value>,
}

/// The `HelmChart` spec — the fields the reconcile core consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelmChartSpec {
    /// Chart name (or absolute chart URL). Required.
    pub chart: String,
    /// Optional chart repository URL.
    pub repo: Option<String>,
    /// Version pin / policy.
    pub version: VersionPolicy,
    /// Namespace the release is installed into. Required.
    pub target_namespace: String,
    /// Inline values YAML, modelled directly as a [`Value`] tree.
    pub values_content: Option<Value>,
    /// Explicit `--set` overrides (highest precedence).
    pub set: BTreeMap<String, Value>,
    /// If true, the chart is part of cluster bootstrap (applied before the
    /// apiserver is fully ready). Affects only ordering, not the merge.
    pub bootstrap: bool,
    /// Image used for the helm job that performs install/upgrade/delete.
    pub job_image: String,
}

impl HelmChartSpec {
    /// Build the `set` layer as a single [`Value`] object (sorted keys).
    #[must_use]
    pub fn set_as_value(&self) -> Option<Value> {
        if self.set.is_empty() {
            return None;
        }
        let mut obj = BTreeMap::new();
        for (k, v) in &self.set {
            obj.insert(k.clone(), v.clone());
        }
        Some(Value::Object(obj))
    }

    /// Validate the spec. Total — never panics.
    ///
    /// # Errors
    /// Returns the first [`SpecError`] encountered.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.chart.trim().is_empty() {
            return Err(SpecError::EmptyChart);
        }
        if self.target_namespace.trim().is_empty() {
            return Err(SpecError::EmptyTargetNamespace);
        }
        if self.job_image.trim().is_empty() {
            return Err(SpecError::EmptyJobImage);
        }
        if let Some(repo) = &self.repo {
            if !repo.trim().is_empty() {
                Repo::parse(repo)?;
            }
        }
        Ok(())
    }
}

/// `HelmChart` — spec + status. Status mirrors what helm-controller writes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelmChart {
    pub name: String,
    pub spec: HelmChartSpec,
    pub status: HelmChartStatus,
}

/// The status subresource helm-controller maintains on a `HelmChart`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelmChartStatus {
    /// Name of the job last created to apply this chart.
    pub job_name: Option<String>,
    /// Hash of the spec+values last *successfully* applied.
    pub last_applied_hash: Option<String>,
    /// Helm release revision counter of the last applied release.
    pub last_revision: Option<u32>,
}

impl HelmChart {
    /// Compute the change-detection hash over the merged effective values plus
    /// the identity-affecting spec fields (chart, resolved version, repo,
    /// namespace, job image, bootstrap).
    ///
    /// `chart_defaults` and the optional `HelmChartConfig` overlay participate
    /// so that a change in any layer flips the hash.
    #[must_use]
    pub fn desired_hash(
        &self,
        chart_defaults: Option<Value>,
        config: Option<&HelmChartConfig>,
    ) -> String {
        let merged = crate::values::merge_layers(
            chart_defaults,
            self.spec.values_content.clone(),
            config.and_then(|c| c.values.clone()),
            self.spec.set_as_value(),
        );
        let version = match &self.spec.version {
            VersionPolicy::Latest => "latest".to_string(),
            VersionPolicy::Pinned(v) => v.clone(),
        };
        let repo = self.spec.repo.clone().unwrap_or_default();
        let identity = format!(
            "chart={}|repo={}|ver={}|ns={}|img={}|boot={}|vals={}",
            self.spec.chart,
            repo,
            version,
            self.spec.target_namespace,
            self.spec.job_image,
            self.spec.bootstrap,
            merged.canonical(),
        );
        short_hex(fnv1a64(&identity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> HelmChartSpec {
        HelmChartSpec {
            chart: "traefik".into(),
            repo: Some("https://helm.traefik.io/traefik".into()),
            version: VersionPolicy::Latest,
            target_namespace: "kube-system".into(),
            values_content: None,
            set: BTreeMap::new(),
            bootstrap: false,
            job_image: "rancher/klipper-helm:v0.8.0".into(),
        }
    }

    #[test]
    fn version_empty_is_latest() {
        assert!(VersionPolicy::parse("").unwrap().is_latest());
        assert!(VersionPolicy::parse("   ").unwrap().is_latest());
    }

    #[test]
    fn version_latest_keyword_case_insensitive() {
        assert!(VersionPolicy::parse("Latest").unwrap().is_latest());
        assert!(VersionPolicy::parse("LATEST").unwrap().is_latest());
    }

    #[test]
    fn version_pinned_with_and_without_v_prefix() {
        assert_eq!(
            VersionPolicy::parse("1.2.3").unwrap(),
            VersionPolicy::Pinned("1.2.3".into())
        );
        assert_eq!(
            VersionPolicy::parse("v1.2.3").unwrap(),
            VersionPolicy::Pinned("1.2.3".into())
        );
    }

    #[test]
    fn version_pinned_prerelease_ok() {
        assert_eq!(
            VersionPolicy::parse("1.2.3-rc.1").unwrap(),
            VersionPolicy::Pinned("1.2.3-rc.1".into())
        );
    }

    #[test]
    fn version_garbage_rejected() {
        assert!(matches!(
            VersionPolicy::parse("not-a-version"),
            Err(SpecError::BadVersion(_))
        ));
        assert!(matches!(
            VersionPolicy::parse("1.2"),
            Err(SpecError::BadVersion(_))
        ));
        assert!(matches!(
            VersionPolicy::parse("1.x.0"),
            Err(SpecError::BadVersion(_))
        ));
    }

    #[test]
    fn repo_https_ok() {
        let r = Repo::parse("https://charts.example.com/stable").unwrap();
        assert_eq!(r.kind, RepoKind::Http);
    }

    #[test]
    fn repo_oci_ok() {
        let r = Repo::parse("oci://registry.example.com/charts").unwrap();
        assert_eq!(r.kind, RepoKind::Oci);
    }

    #[test]
    fn repo_bad_scheme_rejected() {
        assert!(matches!(
            Repo::parse("ftp://x/y"),
            Err(SpecError::BadRepo(_))
        ));
        assert!(matches!(
            Repo::parse("charts.example.com"),
            Err(SpecError::BadRepo(_))
        ));
    }

    #[test]
    fn repo_no_host_rejected() {
        assert!(matches!(Repo::parse("https://"), Err(SpecError::BadRepo(_))));
        assert!(matches!(
            Repo::parse("https:// space/x"),
            Err(SpecError::BadRepo(_))
        ));
    }

    #[test]
    fn validate_accepts_good_spec() {
        assert!(spec().validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_chart() {
        let mut s = spec();
        s.chart = "  ".into();
        assert_eq!(s.validate(), Err(SpecError::EmptyChart));
    }

    #[test]
    fn validate_rejects_empty_namespace() {
        let mut s = spec();
        s.target_namespace = String::new();
        assert_eq!(s.validate(), Err(SpecError::EmptyTargetNamespace));
    }

    #[test]
    fn validate_rejects_empty_job_image() {
        let mut s = spec();
        s.job_image = String::new();
        assert_eq!(s.validate(), Err(SpecError::EmptyJobImage));
    }

    #[test]
    fn validate_rejects_bad_repo() {
        let mut s = spec();
        s.repo = Some("nonsense".into());
        assert!(matches!(s.validate(), Err(SpecError::BadRepo(_))));
    }

    #[test]
    fn empty_repo_string_is_allowed() {
        let mut s = spec();
        s.repo = Some(String::new());
        assert!(s.validate().is_ok());
    }
}
