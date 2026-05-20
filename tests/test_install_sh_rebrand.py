"""CB-5.x — install.sh rebrand smoke tests.

Asserts the v2.0.0 installer banner, hand-off exec, and headless
fallback all use the MDE naming + binary names. Pure-string
assertions over install.sh — no subprocess invocation needed.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest


REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))

INSTALL_SH = REPO / "install.sh"


def install_sh() -> str:
    return INSTALL_SH.read_text(encoding="utf-8")


def test_install_sh_ships():
    assert INSTALL_SH.is_file()


def test_install_sh_passes_bash_syntax_check():
    import subprocess
    result = subprocess.run(
        ["bash", "-n", str(INSTALL_SH)],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, f"bash -n failed: {result.stderr}"


def test_banner_uses_mde_naming():
    src = install_sh()
    assert "Mackes Desktop Environment (MDE)" in src
    # PF6 + Wayland subtitle replaces the v1.x "Carbon · XFCE · Fedora".
    assert "PatternFly 6 · Wayland · Fedora" in src
    assert "Carbon Design System chrome · XFCE" not in src


def test_handoff_exec_targets_mde_binary():
    src = install_sh()
    assert "exec mde" in src
    # The old `exec mackes` must be gone; bin-shim handles back-compat
    # per CB-3.7.
    assert "exec mackes\n" not in src


def test_wizard_hint_uses_mde_binary():
    src = install_sh()
    assert "mde --wizard" in src
    assert "mackes --wizard" not in src


def test_headless_tui_hint_uses_mde_binary():
    src = install_sh()
    assert "mde --tui" in src
    assert "mackes --tui" not in src


def test_cb_5_4_wayland_capability_hint_present():
    src = install_sh()
    # CB-5.4 — when neither DISPLAY nor WAYLAND_DISPLAY is set,
    # nudge user toward the greeter session entry.
    assert "MDE 2.0.0 needs a Wayland session" in src
    assert "Mackes Desktop Environment" in src
    # No GPU probing (Q2 hard-switch lock).
    assert "nvidia-smi" not in src
    assert "lspci" not in src


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
