// SPDX-License-Identifier: Apache-2.0
//! Integration tests for `cave_home_kubelet_rs::cgroup` — the cgroup **v2**
//! resource-conversion + QoS-hierarchy decision logic.
//!
//! Behavioural reference targets (documented kubelet container-manager logic):
//! - pkg/kubelet/cm/helpers_linux.go::MilliCPUToShares / MilliCPUToQuota
//! - the libcontainer/systemd cgroup-v2 cpu.weight conversion
//!   `1 + ((shares - 2) * 9999) / 262142` (shares -> [1,10000])
//! - pkg/kubelet/cm/cgroup_manager_linux.go — the cpu.max / memory.max
//!   unified-hierarchy values
//! - pkg/kubelet/cm/qos_container_manager_linux.go + pod_container_manager_linux.go
//!   — the /kubepods{,/burstable,/besteffort}/pod<uid> cgroupfs hierarchy
//!
//! cgroup **v1** is intentionally NOT modelled (Charter §8 no-backcompat: the
//! kubelet here is cgroupv2-only).

use cave_home_kubelet_rs::cgroup::{
    CgroupHierarchy, CgroupV2Resources, DEFAULT_CPU_PERIOD_US, cpu_max, memory_max,
    milli_cpu_to_cpu_weight, milli_cpu_to_quota_us, milli_cpu_to_shares, shares_to_cpu_weight,
};
use cave_home_kubelet_rs::eviction::QosClass;
use cave_home_kubelet_rs::resources::ResourceRequirements;

// -- milli-cpu -> shares -----------------------------------------------------

#[test]
fn shares_zero_request_floors_to_min() {
    assert_eq!(milli_cpu_to_shares(0), 2);
}

#[test]
fn shares_one_cpu_is_1024() {
    assert_eq!(milli_cpu_to_shares(1000), 1024);
}

#[test]
fn shares_half_cpu_is_512() {
    assert_eq!(milli_cpu_to_shares(500), 512);
}

#[test]
fn shares_tiny_request_floors_to_min() {
    // 1m -> 1*1024/1000 == 1, floored up to MinShares (2).
    assert_eq!(milli_cpu_to_shares(1), 2);
}

// -- shares -> cgroup-v2 cpu.weight ------------------------------------------

#[test]
fn weight_min_shares_is_one() {
    assert_eq!(shares_to_cpu_weight(2), 1);
}

#[test]
fn weight_one_cpu_is_39() {
    // 1 + ((1024-2)*9999)/262142 == 1 + 38 == 39
    assert_eq!(shares_to_cpu_weight(1024), 39);
}

#[test]
fn weight_half_cpu_is_20() {
    // 1 + ((512-2)*9999)/262142 == 1 + 19 == 20
    assert_eq!(shares_to_cpu_weight(512), 20);
}

#[test]
fn weight_caps_at_10000() {
    assert_eq!(shares_to_cpu_weight(262_144), 10_000);
    assert_eq!(shares_to_cpu_weight(1_000_000), 10_000);
}

#[test]
fn milli_cpu_to_weight_composes_shares_then_weight() {
    assert_eq!(milli_cpu_to_cpu_weight(1000), 39);
    assert_eq!(milli_cpu_to_cpu_weight(500), 20);
    assert_eq!(milli_cpu_to_cpu_weight(0), 1);
}

// -- milli-cpu -> cpu.max quota ----------------------------------------------

#[test]
fn quota_one_cpu_default_period() {
    assert_eq!(milli_cpu_to_quota_us(1000, DEFAULT_CPU_PERIOD_US), 100_000);
}

#[test]
fn quota_tiny_limit_floors_to_min() {
    // 1m -> 1*100000/1000 == 100, floored up to MinQuotaPeriod (1000).
    assert_eq!(milli_cpu_to_quota_us(1, DEFAULT_CPU_PERIOD_US), 1000);
}

#[test]
fn cpu_max_unlimited_is_max_keyword() {
    assert_eq!(cpu_max(None, DEFAULT_CPU_PERIOD_US), "max 100000");
}

#[test]
fn cpu_max_one_cpu() {
    assert_eq!(cpu_max(Some(1000), DEFAULT_CPU_PERIOD_US), "100000 100000");
}

#[test]
fn cpu_max_half_cpu() {
    assert_eq!(cpu_max(Some(500), DEFAULT_CPU_PERIOD_US), "50000 100000");
}

#[test]
fn cpu_max_honours_custom_period() {
    assert_eq!(cpu_max(Some(1000), 50_000), "50000 50000");
}

// -- memory.max --------------------------------------------------------------

#[test]
fn memory_max_unlimited_is_max_keyword() {
    assert_eq!(memory_max(None), "max");
}

#[test]
fn memory_max_bytes_verbatim() {
    assert_eq!(memory_max(Some(268_435_456)), "268435456");
}

// -- CgroupV2Resources::from_requirements ------------------------------------

#[test]
fn resources_from_full_requirements() {
    let req = ResourceRequirements {
        cpu_request_milli: Some(500),
        cpu_limit_milli: Some(1000),
        memory_request_bytes: Some(134_217_728),
        memory_limit_bytes: Some(268_435_456),
    };
    let cg = CgroupV2Resources::from_requirements(&req);
    // weight derives from the *request*; max values from the *limits*.
    assert_eq!(cg.cpu_weight, 20);
    assert_eq!(cg.cpu_max, "100000 100000");
    assert_eq!(cg.memory_max, "268435456");
}

#[test]
fn resources_from_empty_requirements_are_unbounded_with_min_weight() {
    let cg = CgroupV2Resources::from_requirements(&ResourceRequirements::default());
    assert_eq!(cg.cpu_weight, 1);
    assert_eq!(cg.cpu_max, "max 100000");
    assert_eq!(cg.memory_max, "max");
}

// -- QoS cgroupfs hierarchy --------------------------------------------------

#[test]
fn default_root_is_kubepods() {
    let h = CgroupHierarchy::default();
    assert_eq!(h.root(), "/kubepods");
}

#[test]
fn guaranteed_pod_sits_directly_under_root() {
    let h = CgroupHierarchy::default();
    assert_eq!(h.pod_path(QosClass::Guaranteed, "abc"), "/kubepods/podabc");
}

#[test]
fn burstable_pod_under_burstable_subtree() {
    let h = CgroupHierarchy::default();
    assert_eq!(
        h.pod_path(QosClass::Burstable, "abc"),
        "/kubepods/burstable/podabc"
    );
}

#[test]
fn besteffort_pod_under_besteffort_subtree() {
    let h = CgroupHierarchy::default();
    assert_eq!(
        h.pod_path(QosClass::BestEffort, "abc"),
        "/kubepods/besteffort/podabc"
    );
}

#[test]
fn container_path_nests_under_pod() {
    let h = CgroupHierarchy::default();
    assert_eq!(
        h.container_path(QosClass::Burstable, "abc", "c1"),
        "/kubepods/burstable/podabc/c1"
    );
}

#[test]
fn custom_root_is_honoured() {
    let h = CgroupHierarchy::with_root("/sys/fs/cgroup/kubepods");
    assert_eq!(
        h.pod_path(QosClass::BestEffort, "u"),
        "/sys/fs/cgroup/kubepods/besteffort/podu"
    );
}
