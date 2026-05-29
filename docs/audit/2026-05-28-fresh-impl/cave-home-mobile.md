# Coverage matrix — cave-home-mobile

**Declared:** fill=0.00 · adr_justified=not-declared · honest=not-declared · scaffold form.
**Verified:** 0/0 mapped symbols found in source · 2 test fns · drift: no.

## MAPPED (implemented + claimed)
*None — crate is a scaffold with no parity claims yet.*

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
*No gaps explicitly recorded — full port deferred to phase 1.*

Implicit scope:
- FFI bridge surface for Flutter → deferred to phase 2b (see lib.rs docstring)
- State sync, push-notification routing, geofencing logic, Portal API client → implementation deferred to phase 1 (first-party port begins then)

## Drift notes
None — every claimed symbol exists in source. Declared fill_ratio=0.00 is consistent with scaffold status: only `FLUTTER_APP_DIR` constant and `flutter_bridge_present()` marker function present. Manifest instructs to replace scaffold with mapped/unmapped/test_port tables once real port begins.
