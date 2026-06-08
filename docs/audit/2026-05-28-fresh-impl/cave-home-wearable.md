# Coverage matrix — cave-home-wearable

**Declared:** fill=0.00 · adr_justified=(implicit scaffold) · honest=0.00 · port method per manifest.  
**Verified:** 0/0 mapped symbols found in source · 0 test fns · drift: no.

## Status
This crate is a **deferred scaffold** per ADR-031. Implementation is scheduled for Phase M11+. The manifest contains only the crate metadata and ratification section; no port work has begun.

## MAPPED (implemented + claimed)
*None — this is a scaffold-only crate with no mapped symbols.*

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Eight Sleep integration | phase-2-m11+ | Deferred; ADR-031 establishes sleep-system actuators as out-of-scope until after ADR-025 wellness-sensor work completes. Line-by-line port to begin in future milestone. |
| Sleep Number integration | phase-2-m11+ | Deferred; see above. Upstream: home-assistant/core sleep_number module. |
| Integration tests | phase-2-m11+ | Deferred until implementation phase begins. |

## Drift notes
None — every declared ratio (fill=0.00, test_port=0.00) is fully supported by the scaffold codebase. No symbols are claimed, so no orphaned references exist. The manifest is honest about its deferred status.
