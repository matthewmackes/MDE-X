"""Tests for the sway-mesh-config birthright step (SWAY-8/Q52).

`apply_sway_mesh_config_link` creates two files on first login:

  ~/.local/share/mde/mesh-storage/sway/shared.conf
      Starter "shared config" that GFS replicates across peers.
      Idempotent: existing content is preserved.

  ~/.config/sway/config.d/50-mesh-shared.conf
      Include fragment with a glob pointing at mesh-storage/sway/.
      Always written if content differs (fixed content).

Branches covered:
  1. No primary user → skip.
  2. User not in /etc/passwd → skip.
  3. Happy path → both files created.
  4. Idempotent: second run preserves operator edits to shared.conf.
  5. Include fragment contains the expected glob directive.
  6. Step is in STEP_FUNCS and PROFILE_STEPS['full'] only.
"""
from __future__ import annotations

import os
import pwd
from pathlib import Path
from typing import Any

import pytest

from mackes import birthright


def _dummy_preset() -> Any:
    return object()


def _patch_environ(monkeypatch, **values: "str | None") -> None:
    for k, v in values.items():
        if v is None:
            monkeypatch.delenv(k, raising=False)
        else:
            monkeypatch.setenv(k, v)


def _patch_getpwnam(monkeypatch, user: str, home: Path) -> None:
    real = pwd.getpwnam

    def fake(name: str) -> pwd.struct_passwd:
        if name == user:
            return pwd.struct_passwd(
                (user, "x", os.getuid(), os.getgid(), "", str(home), "/bin/bash")
            )
        return real(name)

    monkeypatch.setattr(pwd, "getpwnam", fake)


@pytest.fixture()
def fake_user(tmp_path, monkeypatch):
    """Set up a fake user whose home directory is under tmp_path."""
    home = tmp_path / "home" / "fakeuser"
    home.mkdir(parents=True)

    _patch_environ(monkeypatch, SUDO_USER="fakeuser", USER=None, LOGNAME=None)
    _patch_getpwnam(monkeypatch, "fakeuser", home)
    # chown is a no-op in tests (runs as non-root).
    monkeypatch.setattr(os, "chown", lambda *_: None)
    return home


def test_no_primary_user_skips(monkeypatch):
    _patch_environ(monkeypatch, SUDO_USER=None, USER=None, LOGNAME=None)
    actions = birthright.apply_sway_mesh_config_link(_dummy_preset())
    assert any("no primary user" in a for a in actions)


def test_unknown_user_skips(monkeypatch):
    _patch_environ(monkeypatch, SUDO_USER="nonexistent_xyz_1234", USER=None, LOGNAME=None)
    actions = birthright.apply_sway_mesh_config_link(_dummy_preset())
    assert any("not in /etc/passwd" in a for a in actions)


def test_happy_path_creates_both_files(fake_user):
    home = fake_user
    actions = birthright.apply_sway_mesh_config_link(_dummy_preset())

    shared = home / ".local" / "share" / "mde" / "mesh-storage" / "sway" / "shared.conf"
    frag = home / ".config" / "sway" / "config.d" / "50-mesh-shared.conf"

    assert shared.exists(), f"shared.conf missing; actions={actions}"
    assert frag.exists(), f"50-mesh-shared.conf missing; actions={actions}"


def test_shared_conf_contains_mde_reference(fake_user):
    birthright.apply_sway_mesh_config_link(_dummy_preset())
    shared = fake_user / ".local" / "share" / "mde" / "mesh-storage" / "sway" / "shared.conf"
    content = shared.read_text()
    assert "mde" in content.lower() or "mesh" in content.lower()


def test_include_frag_contains_glob(fake_user):
    birthright.apply_sway_mesh_config_link(_dummy_preset())
    frag = fake_user / ".config" / "sway" / "config.d" / "50-mesh-shared.conf"
    content = frag.read_text()
    assert "mesh-storage/sway/*.conf" in content, f"glob missing in: {content!r}"


def test_idempotent_shared_conf_preserved(fake_user):
    """Second run must not overwrite a customised shared.conf."""
    home = fake_user
    birthright.apply_sway_mesh_config_link(_dummy_preset())

    shared = home / ".local" / "share" / "mde" / "mesh-storage" / "sway" / "shared.conf"
    shared.write_text("# operator custom content\n", encoding="utf-8")

    birthright.apply_sway_mesh_config_link(_dummy_preset())

    assert shared.read_text() == "# operator custom content\n", "operator edit was overwritten"


def test_step_registered_in_step_funcs():
    assert "sway-mesh-config" in birthright.STEP_FUNCS


def test_step_in_full_profile():
    assert "sway-mesh-config" in birthright.PROFILE_STEPS["full"]


def test_step_not_in_lighthouse_profile():
    assert "sway-mesh-config" not in birthright.PROFILE_STEPS["lighthouse"]


def test_step_not_in_headless_profile():
    assert "sway-mesh-config" not in birthright.PROFILE_STEPS["headless"]
