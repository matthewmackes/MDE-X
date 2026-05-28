"""CB-2.1 + CB-3.6 — Wayland-session entry + v2.0.0 preset smoke.

Asserts the new files ship, carry the locked content, and the spec
installs them under the right paths.
"""
from __future__ import annotations

import sys
from pathlib import Path

import pytest


REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))


# ---- CB-2.1 — Wayland-session entry ---------------------------------------

WAYLAND_SESSION = REPO / "data/wayland-sessions/mde-hyprland.desktop"


def test_wayland_session_entry_ships():
    assert WAYLAND_SESSION.is_file()


def test_wayland_session_carries_locked_fields():
    """CB-2.1 + HYP-29 — v6.5 renamed the entry to
    `mde-hyprland.desktop` + flipped the visible name to
    `MDE Hyprland` + env-forced MDE_COMPOSITOR=hyprland in
    Exec; TryExec still points at the bare /usr/bin/mde-session
    so greeters that probe executability accept the entry."""
    src = WAYLAND_SESSION.read_text()
    assert "Name=MDE Hyprland" in src
    assert "MDE_COMPOSITOR=hyprland" in src
    assert "Exec=/usr/bin/env MDE_COMPOSITOR=hyprland /usr/bin/mde-session" in src
    assert "TryExec=/usr/bin/mde-session" in src
    assert "Type=Application" in src
    assert "DesktopNames=MDE" in src


def test_wayland_session_starts_with_desktop_entry_header():
    src = WAYLAND_SESSION.read_text()
    assert src.startswith("[Desktop Entry]")


# ---- CB-3.6 — 90-mde.preset -----------------------------------------------

MDE_PRESET = REPO / "data/systemd/90-mde.preset"


def test_mde_preset_ships():
    assert MDE_PRESET.is_file()


def test_mde_preset_enables_session():
    src = MDE_PRESET.read_text()
    assert "enable mde-session.service" in src


def test_mde_preset_does_not_enable_retired_units():
    """Phase B.13 retired 10 v1.x standalone systemd units; the
    v2.0.0 preset must not enable any of them."""
    src = MDE_PRESET.read_text()
    for retired in (
        "mackes-clipboard-daemon",
        "mackes-gvfsd-mesh",
        "mackes-remmina-sync",
        "mackes-media-sync",
        "mackes-ansible-pull",
        "mackesd-kdc-bridge",
    ):
        assert retired not in src, f"{retired} retired; preset must not enable it"


# ---- spec install lines ---------------------------------------------------

SPEC = REPO / "packaging/fedora/mackes-shell.spec"


def test_spec_installs_wayland_session_entry():
    """HYP-29 renamed the desktop entry; the spec install line
    + the %files manifest must reference the new filename."""
    src = SPEC.read_text()
    assert "data/wayland-sessions/mde-hyprland.desktop" in src
    assert "%{_datadir}/wayland-sessions/mde-hyprland.desktop" in src


def test_spec_installs_mde_preset():
    src = SPEC.read_text()
    assert "data/systemd/90-mde.preset" in src
    assert "%{_prefix}/lib/systemd/user-preset/90-mde.preset" in src


# ---- CB-6.5 — release checklist -------------------------------------------

CHECKLIST = REPO / "docs/RELEASE_2_0_0_CHECKLIST.md"


def test_release_checklist_ships():
    assert CHECKLIST.is_file()


def test_release_checklist_has_the_locked_sections():
    src = CHECKLIST.read_text()
    for section in [
        "## A. Code-side gates",
        "## B. Build gates",
        "## C. Static analysis + lint gates",
        "## D. Live VM gates",
        "## E. Docs gates",
        "## F. Tag + release gates",
        "## G. Post-cut bookkeeping",
    ]:
        assert section in src, f"missing checklist section {section!r}"


def test_release_checklist_marks_shipped_items():
    """CB-5.x (A8) + bash-syntax (C6) + CHANGELOG (E4) are already
    landed; the checklist must reflect that."""
    src = CHECKLIST.read_text()
    assert "| A8 | All CB-5.x" in src
    assert "[✓]" in src


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
