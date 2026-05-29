# Coverage matrix — cave-home-orchestration

**Declared:** fill=0.00 · adr_justified=0.00 · honest=0.00 · port method: line-by-line (umbrella/re-export).
**Verified:** 0/0 mapped symbols found in source · 0 test fns · drift: no.

## MAPPED (implemented + claimed)
(no entries — scaffold phase)

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
**Note:** Entire port deferred to Phase 1–4 per ADR-004. Sub-crates (containerd, kubelet, kube-proxy, cni, apiserver, scheduler, controller-manager, kine, helm-controller, klipper-lb, traefik) will be implemented and re-exported in subsequent phases.

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| All K3s orchestration components | phase-1 | Umbrella crate awaiting Phase 1 sub-crate implementations per ADR-004 (accepted 2026-05-14). Port architecture: line-by-line from k3s-io/k3s. |

## Drift notes
None — zero mapped symbols and declared fill_ratio (0.00) is fully supported by the scaffold-only lib.rs. This is the expected state for a pre-port umbrella crate.
