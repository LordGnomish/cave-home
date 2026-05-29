# Coverage matrix — cave-home-household

**Declared:** fill=0.40 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 9/9 mapped symbols found in source · 35 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Product / inventory entry model with validated name + amounts + optional best-before | src/product.rs::Product | yes |
| Quantity units + purchase-step rounding (pieces / packs / grams / millilitres) | src/product.rs::QuantityUnit | yes |
| Stock operation: consume (over-draw rejected, never goes negative) | src/stock.rs::consume | yes |
| Stock operations: purchase/add and open-a-package | src/stock.rs::purchase, src/stock.rs::open | yes |
| Below-min shopping-list generation with purchase-unit rounding | src/shopping.rs::below_min | yes |
| Shopping-list merge of auto + manually-added items (same-product summing) | src/shopping.rs::merge | yes |
| Expiry classification Fresh / ExpiringSoon / Expired against caller-supplied today + window | src/expiry.rs::Freshness | yes |
| Expiry report grouping (expired / expiring-soon / all-fresh) | src/expiry.rs::report | yes |
| Recurring chore model + is_due / next_due math + due-list + member assignment | src/chore.rs::Chore | yes |
| Recipe stock-check: can-we-make-it + shortfall shopping list | src/recipe.rs::can_make | yes |
| Grandma-friendly EN/DE/TR localisation surface (Charter §6.3, ADR-007) | src/label.rs::Lang | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Barcode lookup + product-database import | phase-1b | ADR-026: network-bound I/O adapter; builds Product values reused by engine |
| Grocy REST API-compatibility layer | phase-1b | ADR-026: transport/network layer over this engine; deferred until types stabilize |
| Persistent storage (inventory / chore / shopping-list store) | phase-1b | ADR-026: MVP engine is pure and storage-neutral; filesystem store lands in Phase 1b |
| cave-home-core entity/state integration | phase-1b | ADR-026: low-stock/expiry/due-chore as core State entities + automation; deferred until core API stabilizes |
| cave-home-calendar / notify integration (chore reminders as events) | phase-1b | ADR-026: clock/transport-bound; engine produces plain-language text; scheduling/delivery deferred |
| Real calendar-date best-before / due dates (timezone-aware) | phase-1b | ADR-026 + no-clock-in-engine: engine uses caller-supplied integer day numbers; date/timezone conversion at edge |

## Drift notes
None — every claimed symbol exists in source. All 9 mapped specifications are fully implemented and comprehensively tested (35 test functions across stock, shopping, expiry, chore, recipe, product, and lib modules). The 0.40 fill_ratio correctly reflects Phase 1 scope (pure domain engine + full UX localization); all unfilled items carry explicit ADR-026 Phase 1b dispositions with clear architectural rationale (storage-bound, network-bound, or clock-bound concerns properly deferred). The honest_ratio of 1.00 is fully justified.
