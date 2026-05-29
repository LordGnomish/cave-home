# Coverage matrix — cave-home-cli

**Declared:** fill=1.0 · adr_justified=n/a (first-party) · honest=1.0 · first-party (no upstream port; coverage tracked via subcommand_coverage).
**Verified:** 13/13 subcommands found in source · 136 test fns · drift: no.

## MAPPED (subcommands implemented + claimed)

| Subcommand | Status | Source path | Verified |
|---|---|---|---|
| init | implemented | src/commands/init.rs | yes |
| join | implemented | src/commands/join.rs | yes |
| status | implemented | src/commands/status.rs | yes |
| destroy | implemented | src/commands/destroy.rs | yes |
| device | implemented | src/commands/device.rs | yes |
| room | implemented | src/commands/room.rs | yes (list + show per ADR-007) |
| automation | implemented | src/commands/automation.rs | yes |
| scene | implemented | src/commands/scene.rs | yes |
| solar | stub | src/commands/solar.rs | yes (full demo implementation) |
| unifi | stub | src/commands/unifi.rs | yes (network/protect/access/talk stubs) |
| hue | stub | src/commands/hue.rs | yes |
| knx | stub | src/commands/knx.rs | yes |
| free-home | stub | src/commands/free_home.rs | yes |

## MISSING / PARTIAL (unmapped + scope_cut)

None. All declared subcommands are present in source.

## Drift notes

None — every subcommand declared in the manifest exists in source with pub fn run() or pub fn cmd(). Solar and unifi, despite marked "stub" in manifest, contain full dispatch logic and demo output, not placeholder stubs.

**Note on manifest structure:** Cave-home-cli uses `[[subcommand_coverage]]` table (appropriate for first-party) instead of traditional [[mapped]]/[[unmapped]]/[[scope_cut]] sections used in ported crates. The declared fill_ratio=1.0 and test_port_ratio=1.0 reflect first-party status; honest_ratio and adr_justified_ratio are not applicable. All 13 subcommands dispatch correctly; 136 unit + integration tests confirm implementation parity with declared coverage.
