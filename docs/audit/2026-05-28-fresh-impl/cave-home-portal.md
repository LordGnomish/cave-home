# Coverage matrix — cave-home-portal

**Declared:** fill=0.00 · adr_justified=_not_declared_ · honest=_not_declared_ · port method: line-by-line.
**Verified:** 0/0 mapped symbols found in source · 33 test fns · drift: YES.

## MAPPED (implemented + claimed)
_None — manifest contains no [[mapped]] entries (scaffold form)._

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
_None listed in manifest (scaffold form)._

## Drift notes
**CRITICAL DRIFT:** The manifest declares fill_ratio=0.00 with basis "scaffold + manifest only (real port lands in phase 1)", but the actual crate contains:
- 945 lines of Rust source code across 14 .rs files
- 33 #[test] functions with substantive test logic
- Fully functional modules in src/admin/ including:
  - free_home.rs (labels, routes, placeholder logic)
  - unifi.rs (10 test functions)
  - solar.rs (7 test functions)
  - hue.rs (6 test functions)
  - knx.rs (3 test functions)
  - scheduler.rs, apiserver.rs, controller_manager.rs, and others with test coverage

The manifest's fill_ratio=0.00 is NOT honest given the scope and test coverage already shipped in the crate. The manifest must be updated to reflect the actual implementation state, or the code must be moved/removed to match the declared scaffold status.
