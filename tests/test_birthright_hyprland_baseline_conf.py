"""Tests for HYP-5.b — `apply_hyprland_baseline_conf` in
`mackes/birthright.py`.

HYP-5.a ships the baseline at `/usr/share/mde/hyprland.conf`;
HYP-5.b is the operator-side seed step that writes
`~/.config/hypr/hyprland.conf` so the running compositor reads
the baseline via `source = /usr/share/mde/hyprland.conf`.

These tests cover:

  1. First-login writes the seed file with the `source = ...`
     line + the empty operator override block.
  2. Idempotency: re-runs preserve an existing file so operator
     overrides survive.
  3. Destination dir auto-creation: `~/.config/hypr/` is mkdir'd.
  4. Body shape: the seed contains the canonical baseline source
     line + the override-block marker.

Each test monkeypatches `os.path.expanduser` to redirect `$HOME`
into a tmpdir so the suite never touches the host filesystem
outside the test tree.
"""
from __future__ import annotations

from pathlib import Path

from mackes import birthright


def _patch_home(monkeypatch, home: Path) -> None:
    """Redirect `os.path.expanduser('~')` to `home` for the test."""
    real_expanduser = birthright.os.path.expanduser

    def fake_expanduser(path: str) -> str:
        if path == "~" or path.startswith("~/"):
            tail = path[2:] if path.startswith("~/") else ""
            return str(home / tail) if tail else str(home)
        return real_expanduser(path)

    monkeypatch.setattr(birthright.os.path, "expanduser", fake_expanduser)


def test_first_login_writes_seed(tmp_path, monkeypatch):
    """First run lands the seed at ~/.config/hypr/hyprland.conf."""
    home = tmp_path / "home"
    home.mkdir()
    _patch_home(monkeypatch, home)

    actions = birthright.apply_hyprland_baseline_conf(None)

    dst = home / ".config" / "hypr" / "hyprland.conf"
    assert dst.is_file()
    body = dst.read_text(encoding="utf-8")
    assert "source = /usr/share/mde/hyprland.conf" in body
    assert "Operator overrides below this line" in body
    assert any("wrote" in a for a in actions)


def test_idempotent_preserves_existing(tmp_path, monkeypatch):
    """Re-runs leave an operator-edited file untouched."""
    home = tmp_path / "home"
    home.mkdir()
    _patch_home(monkeypatch, home)

    dst_dir = home / ".config" / "hypr"
    dst_dir.mkdir(parents=True)
    dst = dst_dir / "hyprland.conf"
    operator_edit = (
        "source = /usr/share/mde/hyprland.conf\n"
        "# Operator overrides below this line\n"
        "monitor = DP-1, 2560x1440@165, 0x0, 1\n"
    )
    dst.write_text(operator_edit, encoding="utf-8")

    actions = birthright.apply_hyprland_baseline_conf(None)

    # Operator override survives.
    assert dst.read_text(encoding="utf-8") == operator_edit
    assert any("already present" in a for a in actions)


def test_destination_dir_auto_created(tmp_path, monkeypatch):
    """~/.config/hypr/ is mkdir'd on first run when missing."""
    home = tmp_path / "home"
    home.mkdir()
    _patch_home(monkeypatch, home)

    dst_dir = home / ".config" / "hypr"
    assert not dst_dir.exists()

    birthright.apply_hyprland_baseline_conf(None)

    assert dst_dir.is_dir()
    assert (dst_dir / "hyprland.conf").is_file()


def test_body_carries_source_line_and_override_marker(tmp_path, monkeypatch):
    """Verify both anchor lines land in the body."""
    home = tmp_path / "home"
    home.mkdir()
    _patch_home(monkeypatch, home)

    birthright.apply_hyprland_baseline_conf(None)

    dst = home / ".config" / "hypr" / "hyprland.conf"
    body = dst.read_text(encoding="utf-8")
    lines = body.splitlines()
    # The `source =` line sits alone (no trailing override).
    assert "source = /usr/share/mde/hyprland.conf" in lines
    # The override marker is the last meaningful line.
    assert "# Operator overrides below this line" in lines
