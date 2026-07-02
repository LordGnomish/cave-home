// SPDX-License-Identifier: Apache-2.0
//! The typed workload object model and the in-memory apiserver the workload
//! controllers reconcile against.
//!
//! Behavioural reimplementation of the documented Kubernetes API shapes the
//! core controllers read and write — a minimal, `std`-only subset of the
//! `core/v1`, `apps/v1` and `batch/v1` group APIs (only the fields the pure
//! reconcile logic actually consumes).
//!
//! ## The in-memory apiserver ([`client`])
//!
//! [`client::Api`] is a *real* in-memory implementation of the
//! create/get/update/delete/list contract — the analogue of client-go's
//! `testing.ObjectTracker` / `fake.Clientset` that every upstream controller
//! test runs against, **not** a stub. It assigns UIDs on create, keeps the
//! informer indices consistent, and supports owner/selector queries. The
//! *networked* REST client (`client-go/kubernetes`) and the watch reflector
//! that would fill the cache over the wire remain deferred (see
//! `parity.manifest.toml`); the decision logic is identical either way.
//!
//! [`client::Cluster`] bundles one [`client::Api`] per kind, so a test (or a
//! future real run loop) has a single handle the controllers share.

use std::collections::BTreeMap;

pub mod apps;
pub mod batch;
pub mod client;
pub mod core;

pub use apps::{
    DaemonSet, DaemonSetSpec, DaemonSetStatus, Deployment, DeploymentSpec, DeploymentStatus,
    DeploymentStrategy,
    PodManagementPolicy, ReplicaSet, ReplicaSetSpec, ReplicaSetStatus, StatefulSet,
    StatefulSetSpec, StatefulSetStatus,
};
pub use batch::{ConcurrencyPolicy, CronJob, CronJobSpec, Job, JobSpec, JobStatus};
pub use client::{Api, Cluster};
pub use core::{
    Endpoints, Namespace, Node, NodeCondition, PersistentVolumeClaim, Pod, PodPhase, PodStatus,
    PodTemplateSpec, Service, ServiceAccount,
};

/// A label selector: a key/value map with AND semantics (apimachinery
/// `LabelSelector.matchLabels`).
///
/// An empty selector matches nothing for controllers (a controller with an
/// empty selector would adopt every pod), so callers treat the empty case
/// explicitly.
pub type LabelSelector = BTreeMap<String, String>;

/// `true` if `labels` satisfies every entry of `selector` (AND semantics).
///
/// Mirrors `labels.Selector.Matches`: an **empty** selector matches everything
/// (the caller, not this function, decides whether an empty selector is legal).
#[must_use]
pub fn selector_matches(selector: &LabelSelector, labels: &BTreeMap<String, String>) -> bool {
    selector.iter().all(|(k, v)| labels.get(k) == Some(v))
}

/// A deterministic FNV-1a hash of a pod template, rendered as a short suffix.
///
/// Upstream computes a `pod-template-hash` over the template to name and select
/// the `ReplicaSet` a Deployment owns. We need the same property — equal
/// templates hash equally, different templates differ — and it must be stable
/// across processes (so it is **not** the std `DefaultHasher`, whose seed is
/// randomised per run). FNV-1a over the template's canonical rendering gives a
/// deterministic, collision-resilient-enough tag for this purpose.
#[must_use]
pub fn template_hash(template: &PodTemplateSpec) -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    // Canonical rendering: sorted "k=v;" pairs of the template labels.
    for (k, v) in &template.labels {
        mix(k.as_bytes());
        mix(b"=");
        mix(v.as_bytes());
        mix(b";");
    }
    // Render to a base36-ish short alphanumeric suffix, building the String
    // directly from ASCII chars so no fallible UTF-8 conversion is needed.
    let mut n = h;
    let mut out = String::with_capacity(13);
    if n == 0 {
        out.push('0');
    }
    while n > 0 {
        out.push(ALPHABET[(n % 36) as usize] as char);
        n /= 36;
    }
    out
}
