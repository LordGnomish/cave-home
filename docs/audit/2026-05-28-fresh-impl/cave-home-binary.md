# Coverage matrix — cave-home-binary

**Declared:** fill=0.00 · adr_justified=N/A · honest=N/A · port method: first-party (no upstream).
**Verified:** 0/0 mapped symbols found in source · 0 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| _(none — scaffold phase)_ | — | — |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Real port wiring per Charter §5 | phase-1 | Per manifest basis: "scaffold + manifest only (real port lands in phase 1)". All implementation deferred to M1 roadmap. |

## Drift notes
None — manifest correctly declares 0% fill. This is a first-party scaffold crate with placeholder main() function only; real binary composition lands in phase 1.
