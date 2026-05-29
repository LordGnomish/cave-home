#!/usr/bin/env zsh
# Write the hourly checkpoint status file from the live parity index.
set -e
cd "$(dirname "$0")/.."
STATUS=/tmp/cave-home-uplift-status.json
IDX=$(python3 tools/parity_index.py --json)
HONEST=$(print -r -- "$IDX" | python3 -c 'import sys,json; r=json.load(sys.stdin); print(sum(1 for x in r if x.get("honest_ratio",0)>=0.95 and x.get("fill_ratio",0)>=0.15))')
COMPLETE=$(print -r -- "$IDX" | python3 -c 'import sys,json; r=json.load(sys.stdin); print(sum(1 for x in r if x.get("fill_ratio",0)>=1.0 and x.get("adr_justified_ratio",0)>=1.0))')
DISK=$(df -h /System/Volumes/Data | awk 'NR==2{print $5}')
BRANCH=$(git rev-parse --abbrev-ref HEAD)
COMMITS=$(git rev-list --count main..HEAD 2>/dev/null || echo 0)
TARGET=$(du -sh target 2>/dev/null | awk '{print $1}')
TDD=$(python3 tools/tdd_compliance.py 2>/dev/null | awk -F': ' '/test-first ratio/{print $2}' | tr -d '%')
python3 - "$STATUS" "$HONEST" "$COMPLETE" "$DISK" "$BRANCH" "$COMMITS" "$TARGET" "${TDD:-NA}" "$1" <<'PY'
import sys, json
status, honest, complete, disk, branch, commits, target, tdd, note = sys.argv[1:10]
doc = {
  "updated": note or "checkpoint",
  "honest_count": int(honest),
  "complete_count": int(complete),
  "total_crates": 62,
  "disk_root_used": disk,
  "target_size": target,
  "branch": branch,
  "commits_ahead_of_main": int(commits),
  "tdd_test_first_ratio_pct": (None if tdd in ("NA","") else float(tdd)),
}
open(status,"w").write(json.dumps(doc, indent=2)+"\n")
print(json.dumps(doc, indent=2))
PY
