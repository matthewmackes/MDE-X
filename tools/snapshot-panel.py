#!/usr/bin/env python3
"""Snapshot the live xfce4-panel xfconf channel to data/panel/.

Run this once on a machine whose panel layout you want to canonicalize.
The script dumps every property in the `xfce4-panel` channel along with
an inferred type, then writes:

  data/panel/xfce4-panel.snapshot.json     — {prop: {type, value}, …}
  data/panel/panel-rc/<plugin>.rc          — any per-plugin RC files

`mackes/birthright.py:apply_panel_layout` deploys from these on every
preset apply (idempotent).

Re-run after editing the panel in xfce4-panel-preferences to update the
shipping default.
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path


REPO_DATA = Path(__file__).resolve().parent.parent / "data" / "panel"
RC_SRC = Path.home() / ".config" / "xfce4" / "panel"
SNAPSHOT_FILE = REPO_DATA / "xfce4-panel.snapshot.json"
RC_DEST = REPO_DATA / "panel-rc"


_FLOAT_RE = re.compile(r"^-?\d+\.\d+$")
_INT_RE   = re.compile(r"^-?\d+$")


def _coerce(text: str):
    """Infer (type, value) from an xfconf-query verbose value cell."""
    s = text.strip()
    if s == "":
        return "string", ""
    if s in ("true", "false"):
        return "bool", s == "true"
    if s.startswith("[") and s.endswith("]"):
        inner = s[1:-1]
        if inner == "":
            return "array-string", []
        elems = [x.strip() for x in inner.split(",")]
        # Pick the array's element type from the first element. All
        # xfconf arrays are homogeneous.
        et, _ = _coerce(elems[0])
        coerced = [_coerce(e)[1] for e in elems]
        return f"array-{et}", coerced
    if _FLOAT_RE.match(s):
        return "double", float(s)
    if _INT_RE.match(s):
        # Use uint where the value is non-negative; the xfconf-query CLI
        # accepts uint or int interchangeably for non-negative ints, so
        # picking uint preserves the most-common upstream choice without
        # forcing a recheck.
        v = int(s)
        return ("uint" if v >= 0 else "int"), v
    return "string", s


def dump_channel(channel: str) -> dict:
    """Return {property: {type, value}} for every property in `channel`."""
    out = subprocess.check_output(
        ["xfconf-query", "-c", channel, "-l", "-v"], text=True
    )
    snap: dict[str, dict] = {}
    for line in out.splitlines():
        # Two-column whitespace split — property is left-aligned, value
        # starts after a wide gap.
        m = re.match(r"^(\S+)\s+(.*)$", line)
        if not m:
            continue
        prop, val = m.group(1), m.group(2)
        ty, coerced = _coerce(val)
        snap[prop] = {"type": ty, "value": coerced}
    return snap


def copy_rc_files() -> list[str]:
    """Copy any per-plugin RC files (e.g. whiskermenu favourites)."""
    copied: list[str] = []
    if not RC_SRC.is_dir():
        return copied
    RC_DEST.mkdir(parents=True, exist_ok=True)
    for p in RC_SRC.iterdir():
        if p.suffix in (".rc",) or p.is_dir():
            dst = RC_DEST / p.name
            if p.is_dir():
                shutil.copytree(p, dst, dirs_exist_ok=True)
            else:
                shutil.copy2(p, dst)
            copied.append(p.name)
    return copied


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--channel", default="xfce4-panel")
    args = ap.parse_args(argv)

    if shutil.which("xfconf-query") is None:
        print("xfconf-query not installed", file=sys.stderr)
        return 1

    REPO_DATA.mkdir(parents=True, exist_ok=True)

    snap = dump_channel(args.channel)
    SNAPSHOT_FILE.write_text(
        json.dumps(snap, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"snapshot: {len(snap)} properties → {SNAPSHOT_FILE}")

    rc = copy_rc_files()
    if rc:
        print(f"panel-rc:  {len(rc)} files/dirs → {RC_DEST}")
    else:
        print("panel-rc:  no per-plugin RC files to copy")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
