# Coverage matrix — cave-home-pool

**Declared:** fill=0.00 · adr_justified=scaffold · honest=0 · port method per manifest.
**Verified:** 0/0 mapped symbols found in source · 0 test fns · drift: no.

## MAPPED (implemented + claimed)
None. Crate is a deferred scaffold (see ADR-030, ROADMAP M11+).

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Pool/spa integration (Hayward, Pentair, Jandy, Intex Spa) | M11+ | Deferred per ADR-030. Crate locks workspace shape to prevent premature parallel "pool" crate creation. Line-by-line port from home-assistant/core will begin in phase 1b. Currently contains only lib.rs documentation stub. |

## Drift notes
None — every claimed symbol exists in source. Crate is a placeholder scaffold with no implementation and no mapped symbols declared in manifest.
