# Coverage matrix — cave-home-dns-adguard

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method: clean-room.
**Verified:** 9/9 mapped symbols found in source · 44 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Filter-rule model: block/allow action, exact-vs-subdomain domain pattern | src/rule.rs::{Rule,Action,DomainPattern} | yes |
| Domain normalisation (lowercase ASCII, strip trailing root dot, trim) + plausibility check | src/rule.rs::{normalize_domain,is_plausible_domain} | yes |
| /etc/hosts blackhole parsing (0.0.0.0/127.0.0.1/:: sink + domain) | src/parse.rs::parse_hosts_line | yes |
| Adblock-style domain rules: \|\|domain^ block, @@ allowlist exception, anchors | src/parse.rs::parse_adblock_line | yes |
| Plain domain-list parsing (bare domain → block) + comment/blank handling | src/parse.rs::{parse_plain_domain,is_comment,parse_blocklist} | yes |
| Malformed-line and unsupported-modifier handling: skip, never panic | src/parse.rs::parse_line | yes |
| Match engine: decide(domain) -> Blocked/Allowed/NotFiltered with allowlist precedence | src/engine.rs::RuleSet::decide | yes |
| Custom client rules / per-domain overrides (household allow/block ahead of lists) | src/engine.rs::RuleSet::add_client_rule | yes |
| DNS rewrite model: map a domain to a fixed A/AAAA/CNAME-like answer; first-match-wins | src/rewrite.rs::{Rewrite,RewriteTable,Answer} | yes |
| Query statistics: total/blocked counts, per-domain aggregation, blocked ratio, top-blocked | src/stats.rs::Stats | yes |
| Grandma-friendly localised UX (EN/DE/TR): protection on/off, blocked-today, per-site verdict | src/label.rs | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| DNS server transport (UDP/TCP + DoH/DoT listeners) | phase-1b | ADR-022 / ROADMAP M10: network/socket-bound; sits on top of decision core; clean-room from DNS RFCs. |
| Upstream recursive resolver integration | phase-1b | ADR-022: queries not blocked are forwarded to cave-home-dns-unbound; network-bound integration. |
| Blocklist auto-update fetch + scheduled refresh | phase-1b | ADR-022: HTTP/timer-bound; parser here already compiles whatever text is fetched. |
| Live query-interception loop + persisted query log | phase-1b | ADR-022: async/storage-bound; stats module aggregates in-memory log; persistence deferred. |
| Per-client policy + parental-controls scheduling | phase-1b | ADR-022: builds on client-override model; per-client routing + clock/scheduler deferred. |
| cave-home-core entity/state integration + automation triggers | phase-1b | ADR-022: lands once core's entity API stabilises; decision core is already core-agnostic. |
| Adblock cosmetic/$-modifier rules (element hiding, $third-party, regex/path) | phase-2 | ADR-022: cosmetic filters and $-option scoping are refinement layer; Phase 1 skips them; Phase 2 modifier evaluator. |
| Cloud-relayed / third-party DNS providers | permanent | Charter §9: cave-home never routes DNS through third-party cloud. Intentional permanent gap. |
| Pre-current Adblock-syntax / legacy blocklist-format compatibility | permanent | Charter §7 always-latest + §8 no-backcompat: parses current public syntaxes only. Intentional permanent gap. |
| AdGuard Home GPL source reuse | permanent | Charter §6.1 / ADR-022: upstream is GPL, may not be read or ported. Clean-room only. Intentional permanent gap. |

## Drift notes
None — every claimed symbol exists in source. All 44 tests verify the implemented capabilities (Domain pattern matching, normalisation, parsing 3 formats, comment/blank/modifier/malformed handling, match engine with precedence, client overrides, rewrites, statistics, localised UX). The declared honest_ratio of 1.00 is sound: fill=0.30 (decision core) is fully implemented and tested; the 0.70 unfilled (transport, resolver, auto-update, interception, integration) carries adr_justified=1.00 disposition. Calculation: 0.30 / (0.30 + (1-0.30)×(1-1.00)) = 0.30 / 0.30 = 1.00. Every unfilled item in parity.manifest.toml carries either a phase-1b or permanent ADR-022/Charter disposition.
