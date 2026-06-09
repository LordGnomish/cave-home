#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 cave-home contributors
"""
Build `docs/parity/parity-index.json` from on-disk `parity.manifest.toml`
files.

This is the cave-home counterpart of the cave-runtime parity-index
builder. It is purpose-written for cave-home's manifest schema, which
differs from cave-runtime's:

  * the numeric ratios live in a `[ratios]` table (cave-runtime uses
    `[parity]`), carrying `fill_ratio`, `honest_ratio`,
    `adr_justified_ratio` and `test_port_ratio`;
  * upstreams are declared as an array-of-tables `[[upstream]]` with
    `repo` / `release` / `sha` / `license` (cave-runtime uses a single
    `[upstream]` with `org` / `version`).

Source-of-truth ordering:

  1. The per-crate `parity.manifest.toml` on disk is the live source of
     truth for every numeric field and upstream identity.
  2. `git log -1 --format=%H -- <crate-dir>` provides `last_commit` so a
     dashboard can render staleness.

Output schema (one entry per workspace crate that owns a `Cargo.toml`):

    {
      "generated_at": "<iso8601>",
      "disk_overlay_stats": { "injected": int, "with_manifest": int },
      "crates": {
        "<crate-name>": {
          "crate_dir": "crates/<crate>",
          "fill_ratio": float | null,
          "honest_ratio": float | null,
          "adr_justified_ratio": float | null,
          "test_port_ratio": float | null,
          "infra_only": bool,
          "upstream": "org/repo" | null,
          "upstream_version": str | null,
          "upstream_license": str | null,
          "adr": str | null,
          "ported_at": str | null,
          "last_commit": str | null,
          "last_commit_at": str | null,
          "manifest_filled": bool
        }
      }
    }

The reader (cave-autopilot's tracker) ranks on `honest_ratio`, falling
back to `fill_ratio`; every field is optional on its side, so a crate
with no manifest still produces a valid (zero-completion) entry.
"""
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_PATH = REPO_ROOT / "docs" / "parity" / "parity-index.json"


def discover_workspace_crates() -> dict:
    """Return {crate_name: crate_dir} for every workspace crate that owns a
    `Cargo.toml`, scanning both the flat layout (`crates/<crate>/`) and a
    themed layout (`crates/<theme>/<crate>/`) so a future reorg needs no
    change here."""
    out = {}
    crates_dir = REPO_ROOT / "crates"
    if not crates_dir.is_dir():
        return out
    for p in sorted(crates_dir.iterdir()):
        if not p.is_dir():
            continue
        if (p / "Cargo.toml").is_file():
            out[p.name] = p
            continue
        for sub in sorted(p.iterdir()):
            if sub.is_dir() and (sub / "Cargo.toml").is_file():
                out[sub.name] = sub
    return out


def section_block(text: str, header: str) -> str:
    """Pull the lines under `[header]` up to the next top-level table.
    Comment lines (`#...`) that contain `[` do not terminate the block."""
    lines = text.splitlines()
    block, in_block = [], False
    target = f"[{header}]"
    for line in lines:
        stripped = line.lstrip()
        if stripped.startswith(target):
            in_block = True
            block.append(line)
            continue
        if in_block:
            if stripped.startswith("[") and not stripped.startswith("#"):
                break
            block.append(line)
    return "\n".join(block) if block else ""


def array_blocks(text: str, header: str) -> list:
    """Pull every `[[header]]` array-of-tables entry as its own block."""
    lines = text.splitlines()
    blocks, cur, in_block = [], [], False
    target = f"[[{header}]]"
    for line in lines:
        stripped = line.lstrip()
        if stripped.startswith(target):
            if cur:
                blocks.append("\n".join(cur))
            cur, in_block = [], True
            continue
        if in_block:
            # A new table header (single or array) closes the current entry.
            if stripped.startswith("[") and not stripped.startswith("#"):
                if stripped.startswith(target):
                    blocks.append("\n".join(cur))
                    cur = []
                    continue
                blocks.append("\n".join(cur))
                cur, in_block = [], False
                continue
            cur.append(line)
    if cur:
        blocks.append("\n".join(cur))
    return blocks


def re_str(block: str, key: str):
    m = re.search(rf'^\s*{re.escape(key)}\s*=\s*"([^"]+)"', block, flags=re.MULTILINE)
    return m.group(1) if m else None


def re_float(block: str, key: str):
    m = re.search(rf'^\s*{re.escape(key)}\s*=\s*([0-9]+\.?[0-9]*)', block, flags=re.MULTILINE)
    return float(m.group(1)) if m else None


def re_bool(block: str, key: str):
    m = re.search(rf'^\s*{re.escape(key)}\s*=\s*(true|false)', block, flags=re.MULTILINE)
    return (m.group(1) == "true") if m else None


def parse_manifest(path: Path) -> dict:
    """Read a cave-home `parity.manifest.toml` and return a structured
    snapshot. Returns {} when the file is unreadable."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return {}

    out = {}

    ratios = section_block(text, "ratios")
    if ratios:
        out["fill_ratio"] = re_float(ratios, "fill_ratio")
        out["honest_ratio"] = re_float(ratios, "honest_ratio")
        out["adr_justified_ratio"] = re_float(ratios, "adr_justified_ratio")
        out["test_port_ratio"] = re_float(ratios, "test_port_ratio")
        infra = re_bool(ratios, "infra_only")
        if infra is not None:
            out["infra_only"] = infra

    crate = section_block(text, "crate")
    if crate:
        out["adr"] = re_str(crate, "adr")
        out["ported_at"] = re_str(crate, "ported_at")
        # infra_only may live in [crate] in some manifests.
        if "infra_only" not in out:
            infra = re_bool(crate, "infra_only")
            if infra is not None:
                out["infra_only"] = infra

    # `[[upstream]]` array-of-tables — surface the first declared upstream
    # for the dashboard. License presence drives `manifest_filled`.
    ups = array_blocks(text, "upstream")
    license_seen = None
    if ups:
        first = ups[0]
        out["upstream"] = re_str(first, "repo")
        out["upstream_version"] = re_str(first, "release")
        out["source_sha"] = re_str(first, "sha")
        for u in ups:
            lic = re_str(u, "license")
            if lic:
                license_seen = lic
                break
        if license_seen:
            out["upstream_license"] = license_seen

    out["manifest_filled"] = (
        out.get("fill_ratio") is not None
        and (license_seen is not None or out.get("infra_only") is True)
    )
    return out


def last_commit_for(crate_dir: Path):
    """`git log -1` for the crate dir. Returns (sha, iso8601) or (None, None)."""
    try:
        rel = crate_dir.relative_to(REPO_ROOT)
        res = subprocess.run(
            ["git", "-C", str(REPO_ROOT), "log", "-1",
             "--format=%H%x09%cI", "--", str(rel)],
            capture_output=True, text=True, check=False,
        )
        if res.returncode != 0 or not res.stdout.strip():
            return (None, None)
        sha, _, when = res.stdout.strip().partition("\t")
        return (sha or None, when or None)
    except Exception:
        return (None, None)


def now_iso() -> str:
    """Commit time of HEAD as a deterministic, network-free timestamp."""
    res = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "log", "-1", "--format=%cI"],
        capture_output=True, text=True, check=False,
    )
    return res.stdout.strip() or "unknown"


def main() -> int:
    workspace = discover_workspace_crates()
    crates = {}
    with_manifest = 0
    for name, crate_dir in workspace.items():
        entry = {
            "crate_dir": str(crate_dir.relative_to(REPO_ROOT)),
            "fill_ratio": None,
            "honest_ratio": None,
            "adr_justified_ratio": None,
            "test_port_ratio": None,
            "infra_only": False,
            "upstream": None,
            "upstream_version": None,
            "upstream_license": None,
            "adr": None,
            "ported_at": None,
            "manifest_filled": False,
        }
        manifest = crate_dir / "parity.manifest.toml"
        if manifest.is_file():
            with_manifest += 1
            disk = parse_manifest(manifest)
            for k, v in disk.items():
                if v is not None:
                    entry[k] = v
        sha, when = last_commit_for(crate_dir)
        if sha:
            entry["last_commit"] = sha
        if when:
            entry["last_commit_at"] = when
        crates[name] = entry

    out = {
        "generated_at": now_iso(),
        "disk_overlay_stats": {
            "injected": len(crates),
            "with_manifest": with_manifest,
        },
        "crates": crates,
    }
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")

    measured = sum(
        1 for e in crates.values()
        if e.get("honest_ratio") is not None or e.get("fill_ratio") is not None
    )
    print(
        f"Wrote {OUT_PATH.relative_to(REPO_ROOT)}: {len(crates)} crates "
        f"({with_manifest} with manifest, {measured} with a ratio)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
