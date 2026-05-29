#!/usr/bin/env zsh
# Charter v2 8-gate verification for a single crate. Mechanical where possible;
# reports manifest-asserted facts for the rest. Exit non-zero if any hard gate
# fails. Usage: tools/gates.sh cave-home-<name>
set -uo pipefail
cd "$(dirname "$0")/.."
crate="${1:?usage: gates.sh <crate>}"
dir="crates/$crate"
man="$dir/parity.manifest.toml"
fail=0
pass() { print -r -- "  ✅ $1"; }
bad()  { print -r -- "  ❌ $1"; fail=1; }
warn() { print -r -- "  ⚠️  $1"; }

print -r -- "== 8-gate: $crate =="
[[ -d $dir ]] || { print -r -- "no such crate"; exit 2; }

# G1 — provenance documented: a pinned source SHA (verified line-by-line), OR
# spec_sources (clean-room/spec-based), OR a first-party declaration (original
# work has no upstream), OR a recorded upstream repo reference (documented origin).
if grep -qE '^(source_sha|upstream_sha)' "$man" 2>/dev/null; then
  pass "G1 provenance: upstream source pinned"
elif grep -q 'spec_sources' "$man" 2>/dev/null; then
  pass "G1 provenance: spec_sources recorded (clean-room/spec-based)"
elif grep -qiE 'port_method\s*=\s*".*first-party' "$man" 2>/dev/null; then
  pass "G1 provenance: first-party (no upstream to pin)"
elif grep -qiE '^(upstream_repo|upstream)\s*=' "$man" 2>/dev/null; then
  pass "G1 provenance: upstream repo recorded (documented origin)"
else
  bad "G1 provenance: no source_sha, spec_sources, first-party, or upstream in manifest"
fi

# G2 — SPDX Apache-2.0, not AGPL, license inherited from workspace.
if grep -qE 'license(\.workspace)?\s*=\s*("Apache-2.0"|true)' "$dir/Cargo.toml"; then
  pass "G2 SPDX Apache-2.0 (crate license)"
else
  bad "G2 SPDX: crate Cargo.toml does not declare Apache-2.0 / workspace license"
fi
# A GPL/AGPL token is only a violation if it describes THIS crate's license, not
# when it documents a clean-room upstream that was deliberately NOT read.
gpl_lines=$(grep -niE 'AGPL|GPL-3|GPL-2' "$dir/Cargo.toml" 2>/dev/null \
  | grep -viE 'NOT read|not consulted|clean-room|reference only|reference for|quarantine|never consulted|source NOT')
if grep -qiE '^license\s*=\s*".*(AGPL|GPL-[23])' "$dir/Cargo.toml" 2>/dev/null; then
  bad "G2 SPDX: crate license field declares GPL/AGPL"
elif [[ -n $gpl_lines ]]; then
  warn "G2: GPL/AGPL token in Cargo.toml (verify it is clean-room rationale, not a license claim):"
  print -r -- "$gpl_lines" | sed 's/^/      /'
fi

# G3 — honest_ratio >= 0.95 AND fill >= MVP floor (0.15). Computed by the index.
read -r FILL HONEST < <(python3 tools/parity_index.py --json | python3 -c "
import sys,json
r=json.load(sys.stdin)
m=next((x for x in r if x['crate']=='$crate'),None)
print(m['fill_ratio'], m['honest_ratio']) if m else print('NA NA')")
if python3 -c "import sys; sys.exit(0 if ($HONEST>=0.95 and $FILL>=0.15) else 1)" 2>/dev/null; then
  pass "G3 honest_ratio=$HONEST (fill=$FILL ≥ 0.15 MVP floor)"
else
  bad "G3 honest_ratio=$HONEST / fill=$FILL — below gate (need honest≥0.95 & fill≥0.15)"
fi

# G4 — manifest present and parseable.
if [[ -f $man ]] && python3 -c "import tomllib,sys; tomllib.load(open('$man','rb'))" 2>/dev/null; then
  pass "G4 manifest present & parses"
else
  bad "G4 manifest missing or unparseable"
fi

# G5 — no stub markers in shipped (non-test) code. `todo!`/`unimplemented!` are
# real stubs; `unreachable!()` is NOT a stub — it asserts a provably-dead branch
# (e.g. an exhaustive match the compiler can't prove, or a test-only mock), so it
# is excluded here.
stubs=$(grep -rnE 'todo!|unimplemented!' "$dir/src" 2>/dev/null | grep -viE '#\[cfg\(test|mod tests' )
# crude: also flag a lib.rs that is only a placeholder doc comment.
loc=$(find "$dir/src" -name '*.rs' -exec cat {} + 2>/dev/null | grep -cvE '^\s*(//|$)')
if [[ -n $stubs ]]; then
  bad "G5 stub markers present:"; print -r -- "$stubs" | sed 's/^/      /'
elif [[ ${loc:-0} -lt 20 ]]; then
  bad "G5 placeholder: only $loc non-comment LOC in src/"
else
  pass "G5 no stub markers ($loc non-comment LOC)"
fi

# G6 — no-backcompat: documented stance, no legacy compat cfg.
if grep -qiE 'no-backcompat|no backcompat|backward compat' "$man" || grep -q 'permanent' "$man"; then
  pass "G6 no-backcompat stance recorded in manifest"
else
  warn "G6 no-backcompat not explicitly recorded (Charter §8 still applies)"
fi

# G7 — always-latest: rust-toolchain pins stable; manifest doesn't pin a snapshot.
if grep -q 'channel\s*=\s*"stable"' rust-toolchain.toml; then
  pass "G7 always-latest: toolchain=stable"
else
  bad "G7 always-latest: rust-toolchain not on stable"
fi

# G8 — grandma-friendly UX: crate either produces no user-facing strings, or
# its user-facing strings are jargon-free (best-effort scan of src for banned
# UI terms appearing in string literals).
banned='pod|kubelet|etcd|namespace|RBAC|MQTT topic|PAN-ID|Helm chart|apiserver'
leak=$(grep -rnoE "\"[^\"]*($banned)[^\"]*\"" "$dir/src" 2>/dev/null | grep -viE '#\[cfg\(test|banned|jargon' )
if [[ -n $leak ]]; then
  warn "G8 possible UI jargon in string literals (verify these are not user-facing):"
  print -r -- "$leak" | sed 's/^/      /'
else
  pass "G8 no banned UI jargon in string literals"
fi

print -r -- "-- $crate: $([[ $fail -eq 0 ]] && echo 'GATES PASS' || echo 'GATES FAIL') --"
exit $fail
