# Coverage matrix — cave-home-cluster

**Declared:** fill=0.00 · adr_justified=N/A · honest=N/A · port method per manifest.
**Verified:** 0/0 mapped symbols found in source · 65 test fns · drift: no.

## Summary

This is a scaffold manifest (parity.manifest.toml) filed 2026-05-25 for a first-party crate implementing multi-node cluster lifecycle, K3s coordination, and active-passive failover. Per the manifest header (Charter §5), no [[mapped]], [[unmapped]], or [[scope_cut]] entries exist yet. Real port lands in phase 1.

Fill ratio is explicitly 0.00 with basis "scaffold + manifest only (real port lands in phase 1)."

## Source coverage

The crate contains working implementation code across 9 Rust modules:
- src/lib.rs, failover.rs, quorum.rs, health.rs, update.rs, node.rs, placement.rs, label.rs, topology.rs

All 65 test functions are unit tests embedded in src/ modules (no separate tests/ directory):
- failover.rs: 11 tests
- health.rs: 7 tests  
- quorum.rs: 9 tests
- placement.rs: 8 tests
- update.rs: 9 tests
- label.rs: 5 tests
- node.rs: 5 tests
- topology.rs: 11 tests

## MAPPED (implemented + claimed)
*No entries — manifest is scaffold form.*

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
*No entries — no gaps declared yet. Manifest placeholder awaits phase-1 port detail.*

## Drift notes
None — manifest is explicit scaffold. No mismatch between declared coverage and code reality because no coverage is declared. Implementation exists (65 tests, 9 modules) but is not yet inventoried in the parity manifest.
