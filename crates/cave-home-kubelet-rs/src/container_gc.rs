// SPDX-License-Identifier: Apache-2.0
//! Dead-container garbage-collection *selection*.
//!
//! Behavioural reimplementation of the documented kubelet runtime-GC decision
//! (`pkg/kubelet/kuberuntime/kuberuntime_gc.go`): given the set of containers
//! the runtime knows about and a [`GcPolicy`], decide *which* dead containers
//! to reap. The actual `RemoveContainer` CRI calls are the deferred I/O layer;
//! this is the pure selection logic.
//!
//! The documented algorithm:
//!
//! 1. **Candidates** — a container is evictable only if it is **not running**
//!    and its age (`now - created_at`) is at least [`GcPolicy::min_age_secs`].
//!    Running containers and too-young dead ones are never reaped.
//! 2. **Per evict-unit cap** — candidates are grouped into *evict units* keyed
//!    by `(pod_uid, container_name)`. When [`GcPolicy::max_per_pod_container`]
//!    is non-negative each unit keeps its newest `N` and removes the rest.
//! 3. **Global cap** — when [`GcPolicy::max_containers`] is non-negative and the
//!    surviving candidate count still exceeds it, every unit is re-capped to
//!    `max_containers / units` (floored to 1); if that is still over the limit
//!    the globally-oldest survivors are removed until it fits.
//!
//! A negative cap means "unlimited"; with both caps unlimited nothing is
//! removed regardless of how many dead containers exist.
//!
//! Pure, `std`-only.

use std::collections::BTreeMap;

/// One container known to the runtime, as the GC selector sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerRecord {
    /// The container id.
    pub id: String,
    /// The owning pod's uid.
    pub pod_uid: String,
    /// The container name within the pod (the evict-unit key, with `pod_uid`).
    pub name: String,
    /// Creation time, in unix seconds.
    pub created_at: u64,
    /// Whether the container is currently running (running containers are
    /// never garbage-collected).
    pub running: bool,
}

/// The kubelet container-GC policy (`GCPolicy`).
///
/// `max_per_pod_container` / `max_containers` use `-1` for "unlimited"
/// (matching the kubelet flag semantics), hence the signed type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GcPolicy {
    /// Minimum age, in seconds, before a dead container may be reaped
    /// (`MinAge`).
    pub min_age_secs: u64,
    /// Max dead containers to keep per `(pod, name)` evict-unit
    /// (`MaxPerPodContainer`); `-1` is unlimited.
    pub max_per_pod_container: i64,
    /// Max dead containers to keep node-wide (`MaxContainers`); `-1` is
    /// unlimited.
    pub max_containers: i64,
}

/// Removes the oldest entries of an already-newest-first `unit` until at most
/// `cap` remain, recording each removed id.
fn cap_unit(unit: &mut Vec<&ContainerRecord>, cap: usize, removed: &mut Vec<String>) {
    while unit.len() > cap {
        if let Some(c) = unit.pop() {
            removed.push(c.id.clone());
        }
    }
}

/// Selects the container ids to garbage-collect under `policy`, given every
/// container the runtime knows about and the current time `now` (unix seconds).
///
/// The returned ids are sorted and de-duplicated for a deterministic result;
/// the order carries no semantic meaning.
#[must_use]
pub fn select_containers_to_remove(
    policy: &GcPolicy,
    containers: &[ContainerRecord],
    now: u64,
) -> Vec<String> {
    // Candidates grouped into evict units, each sorted newest-first.
    let mut units: BTreeMap<(&str, &str), Vec<&ContainerRecord>> = BTreeMap::new();
    for c in containers {
        if c.running || now.saturating_sub(c.created_at) < policy.min_age_secs {
            continue;
        }
        units
            .entry((c.pod_uid.as_str(), c.name.as_str()))
            .or_default()
            .push(c);
    }
    for unit in units.values_mut() {
        unit.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    let num_units = units.len();
    let mut removed: Vec<String> = Vec::new();

    // Per evict-unit cap.
    if policy.max_per_pod_container >= 0 {
        let cap = usize::try_from(policy.max_per_pod_container).unwrap_or(0);
        for unit in units.values_mut() {
            cap_unit(unit, cap, &mut removed);
        }
    }

    // Global cap.
    if policy.max_containers >= 0 {
        let max = usize::try_from(policy.max_containers).unwrap_or(0);
        let total: usize = units.values().map(Vec::len).sum();
        if total > max {
            // Re-cap each unit to an even share (at least one), then, if still
            // over, drop the globally-oldest survivors.
            let per_unit = (max / num_units).max(1);
            for unit in units.values_mut() {
                cap_unit(unit, per_unit, &mut removed);
            }
            let remaining: usize = units.values().map(Vec::len).sum();
            if remaining > max {
                let mut survivors: Vec<&ContainerRecord> =
                    units.values().flatten().copied().collect();
                survivors.sort_by(|a, b| {
                    a.created_at
                        .cmp(&b.created_at)
                        .then_with(|| a.id.cmp(&b.id))
                });
                for c in survivors.into_iter().take(remaining - max) {
                    removed.push(c.id.clone());
                }
            }
        }
    }

    removed.sort();
    removed.dedup();
    removed
}
