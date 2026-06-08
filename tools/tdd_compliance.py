#!/usr/bin/env python3
"""tdd_compliance — measure strict-TDD test-first ratio from git history.

Strict-TDD (Burak's directive, 2026-05-29): each feature lands as a `test(crate):`
commit (failing test, verified RED) BEFORE its `feat(crate):` commit (impl,
verified GREEN). This scans `git log main..HEAD` and reports, per `feat(` commit,
whether a `test(` commit for the SAME crate appeared earlier on the branch.

A `<<TDD-DIRECTIVE>>` marker commit splits history into pre-directive (the initial
atomic uplift, committed test+impl together — not force-pushed) and the
strict-TDD era. The headline ratio covers the strict-TDD era only.

Usage: tools/tdd_compliance.py
"""
from __future__ import annotations
import re
import subprocess
import sys

CT = re.compile(r"^[0-9a-f]+\s+(test|feat)\(([^)]+)\)", re.I)


def log():
    out = subprocess.run(
        ["git", "log", "--oneline", "--reverse", "main..HEAD"],
        capture_output=True, text=True, check=True,
    ).stdout.splitlines()
    return out


def main():
    lines = log()
    # find the directive marker (first commit whose subject contains the tag)
    era_start = 0
    for i, l in enumerate(lines):
        if "<<TDD-DIRECTIVE>>" in l or "tdd: enforce strict" in l.lower():
            era_start = i
            break

    tested_crates_before = {}  # crate -> earliest index of a test( commit
    feats = []  # (idx, crate)
    for i, l in enumerate(lines):
        m = CT.match(l)
        if not m:
            continue
        kind, crate = m.group(1).lower(), m.group(2).strip()
        if kind == "test":
            tested_crates_before.setdefault(crate, i)
        else:  # feat
            feats.append((i, crate))

    era_feats = [(i, c) for (i, c) in feats if i >= era_start]
    test_first = 0
    for i, c in era_feats:
        t = tested_crates_before.get(c)
        if t is not None and t < i:
            test_first += 1
    total = len(era_feats)
    ratio = (test_first / total * 100.0) if total else 100.0

    print(f"strict-TDD era feat() commits: {total}")
    print(f"  test-first (a test({{crate}}) preceded the feat): {test_first}")
    print(f"  TDD test-first ratio: {ratio:.0f}%")
    if era_start == 0 and not any("TDD" in l for l in lines):
        print("  (no directive marker found; counting whole branch)")
    # also report raw counts
    nt = sum(1 for l in lines if re.match(r"^[0-9a-f]+\s+test\(", l, re.I))
    nf = sum(1 for l in lines if re.match(r"^[0-9a-f]+\s+feat\(", l, re.I))
    print(f"  raw: {nt} test() commits, {nf} feat() commits on branch")
    return 0


if __name__ == "__main__":
    sys.exit(main())
