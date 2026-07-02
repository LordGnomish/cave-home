// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::container_gc` — the dead-
//! container garbage-collection *selection* decision.
//!
//! Behavioural reference target (documented kubelet runtime GC):
//! - pkg/kubelet/kuberuntime/kuberuntime_gc.go::{evictableContainers,
//!   enforceMaxContainersPerEvictUnit, evictContainers}
//! - the GCPolicy {MinAge, MaxPerPodContainer, MaxContainers} contract.
//!
//! The rules under test:
//! - only **dead** (non-running) containers older than MinAge are candidates;
//! - removal only happens to satisfy a cap — with both caps unlimited (-1)
//!   nothing is removed however many dead containers exist;
//! - MaxPerPodContainer keeps the newest N dead containers per (pod, name)
//!   evict-unit and removes the rest;
//! - MaxContainers, when exceeded, re-caps every unit to MaxContainers/units
//!   (>=1) and, if still over, removes the globally-oldest until it fits.

use cave_home_kubelet_rs::container_gc::{ContainerRecord, GcPolicy, select_containers_to_remove};

fn dead(id: &str, pod: &str, name: &str, created_at: u64) -> ContainerRecord {
    ContainerRecord {
        id: id.to_string(),
        pod_uid: pod.to_string(),
        name: name.to_string(),
        created_at,
        running: false,
    }
}

fn running(id: &str, pod: &str, name: &str, created_at: u64) -> ContainerRecord {
    ContainerRecord {
        running: true,
        ..dead(id, pod, name, created_at)
    }
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

const NOW: u64 = 1000;

#[test]
fn nothing_removed_when_both_caps_unlimited() {
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: -1,
        max_containers: -1,
    };
    let cs = vec![dead("c1", "a", "x", 100), dead("c2", "a", "x", 200)];
    assert!(select_containers_to_remove(&policy, &cs, NOW).is_empty());
}

#[test]
fn running_containers_are_never_removed() {
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 0,
        max_containers: -1,
    };
    let cs = vec![running("c1", "a", "x", 100), running("c2", "a", "x", 200)];
    assert!(select_containers_to_remove(&policy, &cs, NOW).is_empty());
}

#[test]
fn too_young_dead_containers_are_kept() {
    // age 50 < MinAge 100 -> not yet a candidate.
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 0,
        max_containers: -1,
    };
    let cs = vec![dead("c1", "a", "x", 950)];
    assert!(select_containers_to_remove(&policy, &cs, NOW).is_empty());
}

#[test]
fn max_per_pod_keeps_newest_and_removes_older() {
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 1,
        max_containers: -1,
    };
    let cs = vec![
        dead("c1", "a", "x", 100),
        dead("c2", "a", "x", 200),
        dead("c3", "a", "x", 300),
    ];
    // Keep newest (c3); remove the two older.
    assert_eq!(
        sorted(select_containers_to_remove(&policy, &cs, NOW)),
        vec!["c1".to_string(), "c2".to_string()]
    );
}

#[test]
fn max_per_pod_zero_removes_all_evictable() {
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 0,
        max_containers: -1,
    };
    let cs = vec![dead("c1", "a", "x", 100), dead("c2", "a", "x", 200)];
    assert_eq!(
        sorted(select_containers_to_remove(&policy, &cs, NOW)),
        vec!["c1".to_string(), "c2".to_string()]
    );
}

#[test]
fn evict_units_are_split_by_pod_and_name() {
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 1,
        max_containers: -1,
    };
    let cs = vec![
        dead("c1", "a", "x", 100),
        dead("c2", "a", "x", 200),
        dead("c3", "a", "y", 100),
        dead("c4", "a", "y", 200),
    ];
    // Each (pod,name) unit keeps its own newest -> removes c1 and c3.
    assert_eq!(
        sorted(select_containers_to_remove(&policy, &cs, NOW)),
        vec!["c1".to_string(), "c3".to_string()]
    );
}

#[test]
fn max_containers_recaps_units_evenly() {
    // 3 units of 2; MaxPerPod keeps all 6; MaxContainers 4 -> per-unit cap
    // 4/3 == 1, so each unit keeps its newest -> the 3 older removed.
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 5,
        max_containers: 4,
    };
    let cs = vec![
        dead("a1", "a", "x", 100),
        dead("a2", "a", "x", 200),
        dead("b1", "b", "x", 100),
        dead("b2", "b", "x", 200),
        dead("c1", "c", "x", 100),
        dead("c2", "c", "x", 200),
    ];
    assert_eq!(
        sorted(select_containers_to_remove(&policy, &cs, NOW)),
        vec!["a1".to_string(), "b1".to_string(), "c1".to_string()]
    );
}

#[test]
fn max_containers_then_removes_globally_oldest() {
    // One unit of 6. MaxPerPod 5 removes the oldest (c1). MaxContainers 3 ->
    // per-unit cap 3/1 == 3 removes c2,c3; remaining c4,c5,c6 == 3, fits.
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 5,
        max_containers: 3,
    };
    let cs = vec![
        dead("c1", "a", "x", 100),
        dead("c2", "a", "x", 200),
        dead("c3", "a", "x", 300),
        dead("c4", "a", "x", 400),
        dead("c5", "a", "x", 500),
        dead("c6", "a", "x", 600),
    ];
    assert_eq!(
        sorted(select_containers_to_remove(&policy, &cs, NOW)),
        vec!["c1".to_string(), "c2".to_string(), "c3".to_string()]
    );
}

#[test]
fn running_containers_do_not_count_toward_max_containers() {
    // 1 running + 3 dead in one unit. MaxPerPod unlimited; MaxContainers 2 over
    // the 3 evictable -> per-unit cap 2 removes the single oldest dead. The
    // running container is untouched and uncounted.
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: -1,
        max_containers: 2,
    };
    let cs = vec![
        running("r1", "a", "x", 50),
        dead("c1", "a", "x", 100),
        dead("c2", "a", "x", 200),
        dead("c3", "a", "x", 300),
    ];
    assert_eq!(
        sorted(select_containers_to_remove(&policy, &cs, NOW)),
        vec!["c1".to_string()]
    );
}

#[test]
fn empty_input_removes_nothing() {
    let policy = GcPolicy {
        min_age_secs: 100,
        max_per_pod_container: 0,
        max_containers: 0,
    };
    assert!(select_containers_to_remove(&policy, &[], NOW).is_empty());
}
