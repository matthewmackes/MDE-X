"""Tests for the sway-config seeding birthright step.

Operator bug 2026-05-24: "logging into MDE from lightdm opens
empty sway" on a freshly installed test system. Root cause:
mde-session execs `sway` with no `-c`, sway uses its standard
config search chain (XDG_CONFIG_HOME, ~/.config/sway/config,
/etc/sway/config), and the MDE default at
/usr/share/mde/sway/config is OUTSIDE that chain — so without
a per-user seed, sway falls back to stock Fedora and the
operator sees a barren desktop.

`apply_sway_config` is the Python-side fix: idempotent copy of
/usr/share/mde/sway/config → ~/.config/sway/config on first
wizard run. The mde-session Rust fallback (sway_config_args)
covers the case where the wizard never ran.

Branches covered:
  1. No primary user → skip.
  2. User not in /etc/passwd → skip.
  3. User config already present → preserve, no copy.
  4. Source config missing → skip with diagnostic.
  5. Happy path → file copied + chown'd to the user.
  6. Idempotent: second run is the already-present branch.
  7. Root login (no SUDO_USER) → skip.
"""
from __future__ import annotations

import os
import pwd
from pathlib import Path
from typing import Any, List, Tuple

from mackes import birthright


def _dummy_preset() -> Any:
    return object()


def _patch_environ(monkeypatch, **values: str | None) -> None:
    for key in ("SUDO_USER", "USER", "LOGNAME"):
        monkeypatch.delenv(key, raising=False)
    for key, val in values.items():
        if val is None:
            monkeypatch.delenv(key, raising=False)
        else:
            monkeypatch.setenv(key, val)


class _ChownRecorder:
    def __init__(self) -> None:
        self.calls: List[Tuple[str, int, int]] = []

    def __call__(self, path: str, uid: int, gid: int) -> None:
        self.calls.append((str(path), uid, gid))


def _patch_getpwnam(monkeypatch, user: str, home: Path, uid: int = 1000, gid: int = 1000) -> None:
    real = pwd.getpwnam

    def fake(name: str) -> Any:
        if name == user:
            return pwd.struct_passwd(
                (user, "x", uid, gid, "", str(home), "/bin/bash")
            )
        return real(name)

    monkeypatch.setattr(pwd, "getpwnam", fake)


def test_no_primary_user_in_environment_skips(monkeypatch):
    _patch_environ(monkeypatch)
    out = birthright.apply_sway_config(_dummy_preset())
    assert len(out) == 1
    assert "no primary user in environment" in out[0]


def test_user_not_in_passwd_skips(monkeypatch):
    _patch_environ(monkeypatch, SUDO_USER="nobodyhere")
    out = birthright.apply_sway_config(_dummy_preset())
    assert len(out) == 1
    assert "not in /etc/passwd" in out[0]


def test_existing_user_config_is_preserved(monkeypatch, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    sway_dir = home / ".config" / "sway"
    sway_dir.mkdir(parents=True)
    user_cfg = sway_dir / "config"
    user_cfg.write_text("# operator's hand-edited config\n", encoding="utf-8")
    mtime_before = user_cfg.stat().st_mtime_ns

    _patch_environ(monkeypatch, SUDO_USER="alice")
    _patch_getpwnam(monkeypatch, "alice", home)

    out = birthright.apply_sway_config(_dummy_preset())

    assert any("already present" in line for line in out)
    assert any("preserving operator config" in line for line in out)
    assert user_cfg.read_text(encoding="utf-8") == "# operator's hand-edited config\n"
    assert user_cfg.stat().st_mtime_ns == mtime_before


def test_source_missing_at_both_paths_skips_with_diagnostic(monkeypatch, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    _patch_environ(monkeypatch, SUDO_USER="alice")
    _patch_getpwnam(monkeypatch, "alice", home)

    monkeypatch.setattr(
        birthright, "_SWAY_SYSTEM_CONFIG", tmp_path / "no-such-config",
    )
    isolated = tmp_path / "isolated-mackes"
    (isolated / "data" / "sway").mkdir(parents=True)
    monkeypatch.setattr(birthright, "__file__", str(isolated / "birthright.py"))

    out = birthright.apply_sway_config(_dummy_preset())

    assert any("source config missing" in line for line in out)
    assert not (home / ".config" / "sway" / "config").exists()


def test_happy_path_copies_and_chowns(monkeypatch, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    _patch_environ(monkeypatch, SUDO_USER="alice")
    _patch_getpwnam(monkeypatch, "alice", home, uid=1042, gid=1042)

    system_cfg = tmp_path / "share-mde-sway" / "config"
    system_cfg.parent.mkdir(parents=True)
    system_cfg.write_text("# MDE shipped default\nexec mde-panel\n", encoding="utf-8")
    monkeypatch.setattr(birthright, "_SWAY_SYSTEM_CONFIG", system_cfg)

    chown = _ChownRecorder()
    monkeypatch.setattr(os, "chown", chown)

    out = birthright.apply_sway_config(_dummy_preset())

    dest = home / ".config" / "sway" / "config"
    assert dest.exists()
    assert dest.read_text(encoding="utf-8") == "# MDE shipped default\nexec mde-panel\n"
    assert any("seeded" in line and str(dest) in line for line in out)
    chowned_paths = {p for p, _, _ in chown.calls}
    assert str(dest) in chowned_paths
    assert str(dest.parent) in chowned_paths
    for _, uid, gid in chown.calls:
        assert (uid, gid) == (1042, 1042)


def test_idempotent_after_happy_path(monkeypatch, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    _patch_environ(monkeypatch, SUDO_USER="alice")
    _patch_getpwnam(monkeypatch, "alice", home)

    system_cfg = tmp_path / "share-mde-sway" / "config"
    system_cfg.parent.mkdir(parents=True)
    system_cfg.write_text("# v1\n", encoding="utf-8")
    monkeypatch.setattr(birthright, "_SWAY_SYSTEM_CONFIG", system_cfg)
    monkeypatch.setattr(os, "chown", _ChownRecorder())

    first = birthright.apply_sway_config(_dummy_preset())
    assert any("seeded" in line for line in first)

    dest = home / ".config" / "sway" / "config"
    dest.write_text("# operator wins\n", encoding="utf-8")

    second = birthright.apply_sway_config(_dummy_preset())
    assert any("already present" in line for line in second)
    assert dest.read_text(encoding="utf-8") == "# operator wins\n"


def test_root_user_in_environment_skips(monkeypatch):
    _patch_environ(monkeypatch, USER="root")
    out = birthright.apply_sway_config(_dummy_preset())
    assert any("no primary user" in line for line in out)
