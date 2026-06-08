#!/usr/bin/env python3
"""parity_index — derive the cave-home honest-parity index from per-crate manifests.

There is no central index file: the index is *computed* from each crate's
`parity.manifest.toml`. This keeps the manifests the single source of truth and
makes "self-reported parity" (Charter §6) impossible to fake — the numbers come
from structural facts in the manifest, not a hand-edited dashboard.

Definitions
-----------
fill_ratio
    implemented scope / total intended scope. Declared in [ratios].fill_ratio.

adr_justified_ratio
    Of the UNFILLED gap, the fraction that is honestly accounted for — i.e. the
    share of [[unmapped]] / [[scope_cut]] entries that carry both a `priority`
    (phase-1b / phase-2 / permanent) AND a justifying `note`. A gap that is
    written down and dispositioned is "honest"; an undeclared gap is not.
    May be declared explicitly in [ratios].adr_justified_ratio; otherwise
    computed. If declared, we flag any divergence from the computed value > 0.05
    so paperwork can't drift from reality.

honest_ratio
    fill / (fill + unjustified_gap)   where unjustified_gap = (1-fill)*(1-adr)

    i.e. the share of *what remains in scope* that is really implemented, after
    ADR-justified deferrals are removed from the denominator entirely. The ADR
    mechanism legitimately SHRINKS the scope (de-scope via ADR); it never
    INFLATES the "done" count. This is what makes the metric paperwork-proof:
    listing deferrals can raise honest_ratio only if real code already exists.

    Anti-gaming floor: a crate with fill == 0 scores honest_ratio = 0 no matter
    how complete its paperwork (0/0 is defined as 0). And the G3 *gate* (not the
    ratio) additionally requires fill_ratio >= MVP_FLOOR so one trivial function
    + a wall of deferrals cannot pass as "honest". This is the Charter v2 G3
    gate (honest_ratio >= 0.95 AND fill >= MVP_FLOOR).

    Worked examples:
      zigbee  fill .45 adr 1.0 -> .45/(.45+0)      = 1.00  (real slice, rest deferred)
      core    fill .44 adr 0.0 -> .44/(.44+.56)    = 0.44  (half done, nothing accounted)
      stub    fill 0   adr 1.0 -> 0                = 0.00  (paperwork can't save zero code)

Usage
-----
    tools/parity_index.py            # full table, sorted by honest_ratio asc
    tools/parity_index.py --uplift   # only crates needing work (honest < 1.0)
    tools/parity_index.py --json     # machine-readable
"""
from __future__ import annotations

import re
import sys
import json
import tomllib
from pathlib import Path

CRATES = Path(__file__).resolve().parent.parent / "crates"
JUSTIFIED_PRIORITIES = {"phase-1b", "phase-2", "phase-3", "permanent", "deferred"}
MVP_FLOOR = 0.15  # G3 gate: a crate must ship a real MVP slice to count as honest.


def _f(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def analyse(manifest: Path) -> dict:
    data = tomllib.loads(manifest.read_text())
    ratios = data.get("ratios", {})
    crate = data.get("crate", {})
    name = crate.get("name") or manifest.parent.name

    fill = _f(ratios.get("fill_ratio")) or 0.0

    # Honest accounting of the gap: unmapped + scope_cut entries that carry a
    # disposition (priority + note) count as "justified".
    gap_entries = data.get("unmapped", []) + data.get("scope_cut", [])
    total = len(gap_entries)
    justified = sum(
        1
        for e in gap_entries
        if str(e.get("priority", "")).lower() in JUSTIFIED_PRIORITIES
        and e.get("note")
    )
    # If a crate is fully filled it needs no gap entries to be honest.
    if fill >= 1.0:
        computed_adr = 1.0
    elif total == 0:
        # gap exists (fill < 1) but nothing is written down -> not justified.
        computed_adr = 0.0
    else:
        computed_adr = justified / total

    declared_adr = _f(ratios.get("adr_justified_ratio"))
    adr = declared_adr if declared_adr is not None else computed_adr

    # Denominator-shrink model: justified gap leaves scope; it is not "done".
    if fill <= 0.0:
        honest = 0.0
    else:
        unjustified_gap = (1.0 - fill) * (1.0 - adr)
        honest = fill / (fill + unjustified_gap)
    declared_honest = _f(ratios.get("honest_ratio"))

    flags = []
    if honest >= 0.95 and fill < MVP_FLOOR:
        flags.append(f"below-mvp-floor(fill={fill:.2f}<{MVP_FLOOR})")
    if declared_adr is not None and abs(declared_adr - computed_adr) > 0.05:
        flags.append(f"adr_declared={declared_adr:.2f}!=computed={computed_adr:.2f}")
    if declared_honest is not None and abs(declared_honest - honest) > 0.05:
        flags.append(f"honest_declared={declared_honest:.2f}!=computed={honest:.2f}")

    return {
        "crate": name,
        "method": crate.get("port_method", ""),
        "fill_ratio": round(fill, 3),
        "adr_justified_ratio": round(adr, 3),
        "honest_ratio": round(honest, 3),
        "gap_entries": total,
        "gap_justified": justified,
        "flags": flags,
    }


def main(argv):
    rows = []
    for m in sorted(CRATES.glob("*/parity.manifest.toml")):
        try:
            rows.append(analyse(m))
        except Exception as e:  # noqa: BLE001 - report and continue
            rows.append({"crate": m.parent.name, "error": str(e), "honest_ratio": -1})
    rows.sort(key=lambda r: (r.get("honest_ratio", 0), r.get("fill_ratio", 0)))

    if "--json" in argv:
        print(json.dumps(rows, indent=2))
        return 0

    uplift_only = "--uplift" in argv
    print(f"{'crate':40} {'fill':>5} {'adr':>5} {'honest':>6}  method / flags")
    print("-" * 92)
    n_honest = n_complete = 0
    for r in rows:
        if "error" in r:
            print(f"{r['crate']:40} ERROR: {r['error']}")
            continue
        is_honest = r["honest_ratio"] >= 0.95 and r["fill_ratio"] >= MVP_FLOOR
        if is_honest:
            n_honest += 1
        if r["fill_ratio"] >= 1.0 and r["adr_justified_ratio"] >= 1.0:
            n_complete += 1
        if uplift_only and is_honest:
            continue
        flag = ("  ⚠ " + "; ".join(r["flags"])) if r["flags"] else ""
        print(
            f"{r['crate']:40} {r['fill_ratio']:>5.2f} "
            f"{r['adr_justified_ratio']:>5.2f} {r['honest_ratio']:>6.2f}  "
            f"{r['method']}{flag}"
        )
    print("-" * 92)
    print(f"total={len(rows)}  honest(>=0.95)={n_honest}  "
          f"fully-complete(fill>=1 & adr>=1)={n_complete}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
