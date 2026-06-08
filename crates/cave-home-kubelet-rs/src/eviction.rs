// SPDX-License-Identifier: Apache-2.0
//! Eviction ranking under resource pressure.
//!
//! Behavioural reimplementation of the documented kubelet eviction *ordering*
//! (`pkg/kubelet/eviction/helpers.go::rankMemoryPressure` /
//! `rankDiskPressure` and the QoS-aware comparator). When the node is under
//! memory or disk pressure the kubelet evicts pods in a deterministic order so
//! that the least-important, most-over-its-request pods go first.
//!
//! The documented ranking, highest-priority-to-evict first:
//!
//! 1. **QoS class** — `BestEffort` evicted before `Burstable` before
//!    `Guaranteed`. (Guaranteed pods are evicted last and only under node-level
//!    pressure they themselves did not cause.)
//! 2. Within a QoS class, the pod whose **usage exceeds its request by the most**
//!    (for the pressured resource) is evicted first. Usage at or below request
//!    sorts after usage above request.
//!
//! The QoS class itself is derived per the documented rules
//! (`pkg/apis/core/v1/helper/qos/qos.go::GetPodQOS`):
//!
//! * **Guaranteed** — every container has CPU & memory limits == requests
//!   (all set, all equal).
//! * **BestEffort** — no container sets any request or limit.
//! * **Burstable**  — everything else.
//!
//! Pure, `std`-only.

use crate::resources::ResourceRequirements;

/// QoS class (`v1.PodQOSClass`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QosClass {
    /// No requests/limits set anywhere — evicted first.
    BestEffort,
    /// Some requests/limits but not fully guaranteed.
    Burstable,
    /// All containers: limits set == requests for both CPU & memory.
    Guaranteed,
}

impl QosClass {
    /// Eviction rank: a *higher* number is evicted *earlier*.
    #[must_use]
    const fn eviction_rank(self) -> u8 {
        match self {
            Self::BestEffort => 2,
            Self::Burstable => 1,
            Self::Guaranteed => 0,
        }
    }
}

/// Derive a pod's [`QosClass`] from its containers' resource requirements.
#[must_use]
pub fn qos_class(containers: &[ResourceRequirements]) -> QosClass {
    if containers.is_empty() {
        return QosClass::BestEffort;
    }

    let mut any_request_or_limit = false;
    let mut all_guaranteed = true;

    for c in containers {
        let cpu_req = c.cpu_request_milli;
        let cpu_lim = c.cpu_limit_milli;
        let mem_req = c.memory_request_bytes;
        let mem_lim = c.memory_limit_bytes;

        if cpu_req.is_some() || cpu_lim.is_some() || mem_req.is_some() || mem_lim.is_some() {
            any_request_or_limit = true;
        }

        // Guaranteed requires both CPU and memory limits set AND equal to
        // their (set) requests for every container.
        let cpu_guaranteed = matches!((cpu_req, cpu_lim), (Some(r), Some(l)) if r == l);
        let mem_guaranteed = matches!((mem_req, mem_lim), (Some(r), Some(l)) if r == l);
        if !(cpu_guaranteed && mem_guaranteed) {
            all_guaranteed = false;
        }
    }

    if !any_request_or_limit {
        QosClass::BestEffort
    } else if all_guaranteed {
        QosClass::Guaranteed
    } else {
        QosClass::Burstable
    }
}

/// The resource axis under pressure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PressuredResource {
    /// Node memory pressure.
    Memory,
    /// Node ephemeral-storage / disk pressure.
    Disk,
}

/// A candidate pod for eviction ranking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvictionCandidate {
    /// Pod identifier (opaque; used only to report the order).
    pub pod_uid: String,
    /// QoS class of the pod.
    pub qos: QosClass,
    /// Current usage of the pressured resource (bytes for memory/disk).
    pub usage: u64,
    /// Request for the pressured resource (bytes); 0 if unset.
    pub request: u64,
}

impl EvictionCandidate {
    /// Usage above request (saturating); 0 if at or below request.
    #[must_use]
    const fn over_request(&self) -> u64 {
        self.usage.saturating_sub(self.request)
    }
}

/// Rank `candidates` for eviction, **first to evict first**.
///
/// Returns the pod UIDs in eviction order. The input is not mutated.
///
/// Ordering key (descending priority-to-evict):
/// 1. QoS eviction rank (BestEffort > Burstable > Guaranteed),
/// 2. usage-over-request (more over its request -> evicted earlier),
/// 3. raw usage (tie-break: bigger user evicted earlier),
/// 4. pod UID (final stable tie-break).
#[must_use]
pub fn rank_for_eviction(
    candidates: &[EvictionCandidate],
    _resource: PressuredResource,
) -> Vec<String> {
    let mut idx: Vec<&EvictionCandidate> = candidates.iter().collect();
    idx.sort_by(|a, b| {
        b.qos
            .eviction_rank()
            .cmp(&a.qos.eviction_rank())
            .then_with(|| b.over_request().cmp(&a.over_request()))
            .then_with(|| b.usage.cmp(&a.usage))
            .then_with(|| a.pod_uid.cmp(&b.pod_uid))
    });
    idx.into_iter().map(|c| c.pod_uid.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(cr: Option<u64>, cl: Option<u64>, mr: Option<u64>, ml: Option<u64>) -> ResourceRequirements {
        ResourceRequirements {
            cpu_request_milli: cr,
            cpu_limit_milli: cl,
            memory_request_bytes: mr,
            memory_limit_bytes: ml,
        }
    }

    #[test]
    fn no_resources_is_besteffort() {
        assert_eq!(qos_class(&[ResourceRequirements::default()]), QosClass::BestEffort);
    }

    #[test]
    fn empty_pod_is_besteffort() {
        assert_eq!(qos_class(&[]), QosClass::BestEffort);
    }

    #[test]
    fn limits_equal_requests_is_guaranteed() {
        let c = req(Some(500), Some(500), Some(1000), Some(1000));
        assert_eq!(qos_class(&[c]), QosClass::Guaranteed);
    }

    #[test]
    fn limit_not_equal_request_is_burstable() {
        let c = req(Some(500), Some(1000), Some(1000), Some(1000));
        assert_eq!(qos_class(&[c]), QosClass::Burstable);
    }

    #[test]
    fn request_without_limit_is_burstable() {
        let c = req(Some(500), None, Some(1000), None);
        assert_eq!(qos_class(&[c]), QosClass::Burstable);
    }

    #[test]
    fn only_cpu_guaranteed_but_no_memory_is_burstable() {
        let c = req(Some(500), Some(500), None, None);
        assert_eq!(qos_class(&[c]), QosClass::Burstable);
    }

    #[test]
    fn mixed_containers_one_not_guaranteed_is_burstable() {
        let g = req(Some(500), Some(500), Some(1000), Some(1000));
        let b = req(Some(500), None, Some(1000), None);
        assert_eq!(qos_class(&[g, b]), QosClass::Burstable);
    }

    fn cand(uid: &str, qos: QosClass, usage: u64, request: u64) -> EvictionCandidate {
        EvictionCandidate {
            pod_uid: uid.into(),
            qos,
            usage,
            request,
        }
    }

    #[test]
    fn qos_ordering_besteffort_first_guaranteed_last() {
        let cs = [
            cand("guar", QosClass::Guaranteed, 100, 100),
            cand("burst", QosClass::Burstable, 100, 50),
            cand("be", QosClass::BestEffort, 100, 0),
        ];
        let order = rank_for_eviction(&cs, PressuredResource::Memory);
        assert_eq!(order, vec!["be", "burst", "guar"]);
    }

    #[test]
    fn within_qos_most_over_request_first() {
        let cs = [
            cand("a", QosClass::Burstable, 200, 100), // over by 100
            cand("b", QosClass::Burstable, 500, 100), // over by 400
            cand("c", QosClass::Burstable, 120, 100), // over by 20
        ];
        let order = rank_for_eviction(&cs, PressuredResource::Memory);
        assert_eq!(order, vec!["b", "a", "c"]);
    }

    #[test]
    fn usage_at_or_below_request_sorts_last_within_qos() {
        let cs = [
            cand("over", QosClass::Burstable, 300, 100), // over by 200
            cand("under", QosClass::Burstable, 50, 100), // under -> over_request 0
        ];
        let order = rank_for_eviction(&cs, PressuredResource::Memory);
        assert_eq!(order, vec!["over", "under"]);
    }

    #[test]
    fn ties_broken_by_uid_for_determinism() {
        let cs = [
            cand("z", QosClass::BestEffort, 100, 0),
            cand("a", QosClass::BestEffort, 100, 0),
        ];
        let order = rank_for_eviction(&cs, PressuredResource::Disk);
        assert_eq!(order, vec!["a", "z"]);
    }

    #[test]
    fn disk_and_memory_use_same_comparator() {
        let cs = [
            cand("be", QosClass::BestEffort, 10, 0),
            cand("guar", QosClass::Guaranteed, 9999, 9999),
        ];
        assert_eq!(
            rank_for_eviction(&cs, PressuredResource::Disk),
            vec!["be", "guar"]
        );
        assert_eq!(
            rank_for_eviction(&cs, PressuredResource::Memory),
            vec!["be", "guar"]
        );
    }
}
