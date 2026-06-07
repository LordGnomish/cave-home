// SPDX-License-Identifier: Apache-2.0
//! RED-phase test for the **`metrics_server::quantity`** module — the
//! Kubernetes `resource.Quantity` formatting the metrics-server resource-metrics
//! API uses to express CPU and memory usage.
//!
//! Drives a NEW feature that does not yet exist in `cave-home-orchestration`:
//! a behavioural reimplementation of the slice of `k8s.io/apimachinery`
//! `resource.Quantity` that kubernetes-sigs/metrics-server depends on to turn
//! raw kubelet samples into the `metrics.k8s.io/v1beta1` quantities a client
//! (`kubectl top`) renders. No upstream Go was transcribed; the canonical
//! `DecimalSI` (CPU) and `BinarySI` (memory) suffix rules are reproduced from
//! the public `resource.Quantity` contract.
//!
//! ## Public API this drives Phase B to add
//!
//! - `metrics_server::quantity::Quantity` with constructors
//!   `Quantity::from_cpu_nanocores(u64)` and `Quantity::from_bytes(u64)`.
//! - CPU rendering: `to_cpu_string()` → canonical `DecimalSI` (`"250m"`, `"1n"`,
//!   `"2"`); `milli_cpu()` → millicores **rounded up** (kubectl `MilliValue`).
//! - Memory rendering: `to_mem_string()` → canonical `BinarySI` (`"128Mi"`,
//!   `"1536Mi"`, `"0"`); `mebibytes()` → MiB **rounded up** (kubectl top display).
//! - `ResourceList { cpu, memory }` — the `{cpu, memory}` pair the API attaches
//!   to every node / container usage.

use cave_home_orchestration::metrics_server::quantity::{Quantity, ResourceList};

#[test]
fn cpu_canonical_decimal_si_string() {
    // 0.25 cores = 250 000 000 nanocores normalises to "250m" (milli).
    assert_eq!(Quantity::from_cpu_nanocores(250_000_000).to_cpu_string(), "250m");
    // 2 whole cores → "2" (no suffix).
    assert_eq!(Quantity::from_cpu_nanocores(2_000_000_000).to_cpu_string(), "2");
    // 1.5 cores = 1500m, not divisible by a whole core → "1500m".
    assert_eq!(Quantity::from_cpu_nanocores(1_500_000_000).to_cpu_string(), "1500m");
    // A single nanocore stays at nano scale.
    assert_eq!(Quantity::from_cpu_nanocores(1).to_cpu_string(), "1n");
    // Micro scale: 5 000 nano = 5u.
    assert_eq!(Quantity::from_cpu_nanocores(5_000).to_cpu_string(), "5u");
    assert_eq!(Quantity::from_cpu_nanocores(0).to_cpu_string(), "0");
}

#[test]
fn cpu_milli_rounds_up_like_kubectl() {
    // MilliValue rounds the fractional milli UP, never truncates to zero.
    assert_eq!(Quantity::from_cpu_nanocores(250_000_000).milli_cpu(), 250);
    assert_eq!(Quantity::from_cpu_nanocores(1).milli_cpu(), 1);
    assert_eq!(Quantity::from_cpu_nanocores(1_000_001).milli_cpu(), 2);
    assert_eq!(Quantity::from_cpu_nanocores(0).milli_cpu(), 0);
}

#[test]
fn memory_canonical_binary_si_string() {
    // 128 MiB exactly.
    assert_eq!(Quantity::from_bytes(128 * 1024 * 1024).to_mem_string(), "128Mi");
    // 1.5 GiB = 1536 MiB — divisible by Mi, not by Gi.
    assert_eq!(Quantity::from_bytes(1536 * 1024 * 1024).to_mem_string(), "1536Mi");
    // 2 GiB exactly → "2Gi".
    assert_eq!(Quantity::from_bytes(2 * 1024 * 1024 * 1024).to_mem_string(), "2Gi");
    // 4 KiB → "4Ki".
    assert_eq!(Quantity::from_bytes(4096).to_mem_string(), "4Ki");
    // A prime byte count is not divisible by any binary unit → raw bytes.
    assert_eq!(Quantity::from_bytes(1023).to_mem_string(), "1023");
    assert_eq!(Quantity::from_bytes(0).to_mem_string(), "0");
}

#[test]
fn memory_mebibytes_rounds_up_for_top_display() {
    assert_eq!(Quantity::from_bytes(128 * 1024 * 1024).mebibytes(), 128);
    // 1 byte over a MiB rounds up to the next whole MiB.
    assert_eq!(Quantity::from_bytes(1024 * 1024 + 1).mebibytes(), 2);
    // Sub-MiB rounds up to 1, never 0, for a non-empty working set.
    assert_eq!(Quantity::from_bytes(1).mebibytes(), 1);
    assert_eq!(Quantity::from_bytes(0).mebibytes(), 0);
}

#[test]
fn resource_list_pairs_cpu_and_memory() {
    let rl = ResourceList::new(
        Quantity::from_cpu_nanocores(250_000_000),
        Quantity::from_bytes(64 * 1024 * 1024),
    );
    assert_eq!(rl.cpu.to_cpu_string(), "250m");
    assert_eq!(rl.memory.to_mem_string(), "64Mi");
    // Equality is structural so the API layer can compare usage lists.
    assert_eq!(
        rl,
        ResourceList::new(
            Quantity::from_cpu_nanocores(250_000_000),
            Quantity::from_bytes(64 * 1024 * 1024),
        )
    );
}

#[test]
fn quantities_sum_for_pod_aggregation() {
    // The API sums per-container usage into a pod total, so Quantity must add.
    let a = Quantity::from_cpu_nanocores(100_000_000);
    let b = Quantity::from_cpu_nanocores(150_000_000);
    assert_eq!(a.saturating_add(b).to_cpu_string(), "250m");

    let m1 = Quantity::from_bytes(32 * 1024 * 1024);
    let m2 = Quantity::from_bytes(96 * 1024 * 1024);
    assert_eq!(m1.saturating_add(m2).to_mem_string(), "128Mi");
}
