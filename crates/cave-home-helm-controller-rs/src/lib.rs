// SPDX-License-Identifier: Apache-2.0
//! `cave-home-helm-controller-rs` — the decision core of K3s's
//! **helm-controller** (ADR-004, Orchestration Phase 4).
//!
//! helm-controller reconciles `HelmChart` custom resources into Helm releases:
//! it installs a chart when its CR appears, upgrades the release when the spec
//! or values change, leaves it alone when nothing changed, rolls back a failed
//! upgrade, and uninstalls when the CR is deleted. The heavy lifting is done by
//! running `helm` inside a Kubernetes `Job`.
//!
//! This crate is a **behavioural reimplementation** (HONEST port) of the
//! documented helm-controller behaviour, built std-only from public Helm and
//! helm-controller CRD documentation (all Apache-2.0-compatible). It does
//! **not** claim verbatim line-by-line parity. It ships the parts that are pure
//! logic and fully testable without a cluster:
//!
//! * [`chart`] — the `HelmChart` / `HelmChartConfig` CRD model, spec
//!   validation, and version/repo resolution.
//! * [`values`] — a small nested value tree and Helm's deep-merge with the
//!   documented layer precedence and `--set` null-removal.
//! * [`hash`] — a deterministic content hash driving change detection.
//! * [`reconcile`] — the install/upgrade/no-op/rollback/uninstall decision
//!   plus retry classification and backoff.
//! * [`job`] — pure construction of the helm container args a Job would run.
//!
//! The actual Helm SDK/exec, the in-cluster Job runner, the CRD watch/informer,
//! cluster apply, repo fetch/auth, and OCI registry support are deferred to
//! Phase 1b (see `parity.manifest.toml`).
//!
//! Per ADR-004 §6.3, helm-controller lives entirely under the hood: this crate
//! produces no end-user-facing strings and carries no i18n.
//!
//! # Example
//!
//! ```
//! use cave_home_helm_controller_rs::chart::{HelmChart, HelmChartSpec, VersionPolicy};
//! use cave_home_helm_controller_rs::reconcile::{decide, Action, ReleaseState};
//! use std::collections::BTreeMap;
//!
//! let chart = HelmChart {
//!     name: "traefik".into(),
//!     spec: HelmChartSpec {
//!         chart: "traefik".into(),
//!         repo: Some("https://helm.traefik.io/traefik".into()),
//!         version: VersionPolicy::parse("v1.2.3").unwrap(),
//!         target_namespace: "kube-system".into(),
//!         values_content: None,
//!         set: BTreeMap::new(),
//!         bootstrap: false,
//!         job_image: "rancher/klipper-helm:v0.8.0".into(),
//!     },
//!     status: cave_home_helm_controller_rs::chart::HelmChartStatus::default(),
//! };
//! assert!(chart.spec.validate().is_ok());
//!
//! // No release yet → install.
//! assert_eq!(
//!     decide(&chart, &ReleaseState::Absent, false, None, None),
//!     Action::Install,
//! );
//! ```

pub mod chart;
pub mod hash;
pub mod job;
pub mod reconcile;
pub mod values;

pub use chart::{
    HelmChart, HelmChartConfig, HelmChartSpec, HelmChartStatus, Repo, RepoKind, SpecError,
    VersionPolicy,
};
pub use hash::{fnv1a64, short_hex};
pub use job::{build_args, Operation};
pub use reconcile::{
    backoff_secs, decide, Action, FailureClass, FailureReason, ReleaseState,
};
pub use values::{merge_layers, Value};
