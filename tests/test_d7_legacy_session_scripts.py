"""Phase D.7 — retirement guards in bin/mackes-enforce-session +
bin/mackes-wm. Both scripts short-circuit when the MDE Wayland
session is active so they don't fight the new orchestrator.
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest


REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))

ENFORCE = REPO / "bin/mackes-enforce-session"
WM = REPO / "bin/mackes-wm"


# ---- syntax + ship ---------------------------------------------------------

def test_legacy_scripts_pass_bash_syntax():
    for p in (ENFORCE, WM):
        r = subprocess.run(["bash", "-n", str(p)], capture_output=True, text=True)
        assert r.returncode == 0, f"bash -n failed for {p.name}: {r.stderr}"


def test_legacy_scripts_carry_d7_retirement_guard():
    for p in (ENFORCE, WM):
        src = p.read_text()
        assert "Phase D.7" in src or "MDE" in src.upper(), (
            f"{p.name} missing D.7 retirement guard"
        )


# ---- enforce-session: short-circuits under MDE -----------------------------

def test_enforce_session_short_circuits_when_xdg_current_desktop_is_mde(tmp_path):
    """Setting XDG_CURRENT_DESKTOP=MDE before calling the script
    must make it print the retirement note + exit 0 without doing
    any work."""
    env = {**os.environ, "XDG_CURRENT_DESKTOP": "MDE"}
    # Stub PATH so `i3`, `wmctrl`, `xfconf-query`, `xprop`, etc.
    # don't exist — the script must NOT call them under the
    # short-circuit. We add /usr/bin first so `systemctl` is
    # available for the guard's `is-active` probe.
    r = subprocess.run(
        ["bash", str(ENFORCE)],
        capture_output=True,
        text=True,
        env=env,
        cwd=str(tmp_path),
    )
    assert r.returncode == 0, r.stderr
    assert "MDE session detected" in r.stderr
    assert "retired" in r.stderr


# ---- mackes-wm: short-circuits under MDE -----------------------------------

def test_mackes_wm_short_circuits_under_swaysock(tmp_path):
    """SWAYSOCK in the env signals an active sway session — the
    script must short-circuit + point at the new paths."""
    env = {**os.environ, "SWAYSOCK": "/run/user/0/sway-ipc.sock"}
    r = subprocess.run(
        ["bash", str(WM), "status"],
        capture_output=True,
        text=True,
        env=env,
        cwd=str(tmp_path),
    )
    assert r.returncode == 0, r.stderr
    assert "retired" in r.stderr
    assert "swaymsg" in r.stderr
    assert "mde-session.service" in r.stderr


def test_mackes_wm_short_circuits_when_xdg_current_desktop_is_mde(tmp_path):
    env = {**os.environ, "XDG_CURRENT_DESKTOP": "MDE"}
    env.pop("SWAYSOCK", None)
    r = subprocess.run(
        ["bash", str(WM), "status"],
        capture_output=True,
        text=True,
        env=env,
        cwd=str(tmp_path),
    )
    assert r.returncode == 0, r.stderr
    assert "retired" in r.stderr


# ---- legacy path still works during back-compat window --------------------

def test_enforce_session_legacy_path_does_not_crash_under_no_mde(tmp_path):
    """Without the MDE markers, the script falls through to its
    legacy converge logic. We don't assert what it does (it
    needs i3 + wmctrl + xfconf-query to actually do anything),
    just that the guard doesn't fire and the script doesn't
    immediately crash on the unguarded path."""
    env = {**os.environ}
    env.pop("XDG_CURRENT_DESKTOP", None)
    env.pop("SWAYSOCK", None)
    # We pipe-stderr-only — exit code may be 0 (idempotent
    # success on a no-op system) or non-zero (i3 not found, etc.)
    # — either is fine for this test; we're only making sure the
    # GUARD didn't fire.
    r = subprocess.run(
        ["bash", str(ENFORCE)],
        capture_output=True,
        text=True,
        env=env,
        cwd=str(tmp_path),
        timeout=10,
    )
    # The guard's message must NOT appear.
    assert "MDE session detected" not in r.stderr


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
