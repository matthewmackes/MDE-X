"""Tests for v2.0.0 Phase 0.5 mde-migrate-from-1x.

Imports the script's helpers directly (the file is shipped as
bin/mde-migrate-from-1x; we load it as a module to test the pure
helpers without spawning a subprocess).
"""
from __future__ import annotations

import importlib.util
import os
import tempfile
from pathlib import Path


def _load_module():
    """Load the bin/mde-migrate-from-1x file as a Python module.

    The file has no `.py` extension because it's a system-installable
    executable, so we bypass `spec_from_file_location`'s default
    suffix probe and feed it the explicit SourceFileLoader.
    """
    import importlib.machinery
    repo = Path(__file__).resolve().parent.parent
    path = repo / "bin" / "mde-migrate-from-1x"
    loader = importlib.machinery.SourceFileLoader(
        "mde_migrate_from_1x", str(path),
    )
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def _with_tmpdir(fn):
    tmp = tempfile.TemporaryDirectory()
    try:
        return fn(Path(tmp.name))
    finally:
        tmp.cleanup()


def test_migrate_pair_returns_noop_when_legacy_absent():
    def body(tmp):
        mod = _load_module()
        legacy = tmp / "config" / "mackes-shell"
        target = tmp / "config" / "mde"
        assert mod.migrate_pair(legacy, target) == "noop"
        # No side effect — target dir not created.
        assert not target.exists()
    _with_tmpdir(body)


def test_migrate_pair_moves_when_only_legacy_exists():
    def body(tmp):
        mod = _load_module()
        legacy = tmp / "config" / "mackes-shell"
        target = tmp / "config" / "mde"
        legacy.mkdir(parents=True)
        (legacy / "panel.toml").write_text("[ui]\ntheme = 'mde'\n")
        assert mod.migrate_pair(legacy, target) == "moved"
        assert not legacy.exists(), "legacy should be moved out of the way"
        assert (target / "panel.toml").read_text().strip().endswith("'mde'")
    _with_tmpdir(body)


def test_migrate_pair_collision_when_both_exist_keeps_legacy():
    def body(tmp):
        mod = _load_module()
        legacy = tmp / "config" / "mackes-shell"
        target = tmp / "config" / "mde"
        legacy.mkdir(parents=True)
        (legacy / "panel.toml").write_text("legacy")
        target.mkdir(parents=True)
        (target / "panel.toml").write_text("new")
        assert mod.migrate_pair(legacy, target) == "collision"
        # Both still exist; nothing clobbered.
        assert (legacy / "panel.toml").read_text() == "legacy"
        assert (target / "panel.toml").read_text() == "new"
    _with_tmpdir(body)


def test_migrate_pair_is_idempotent_after_move():
    def body(tmp):
        mod = _load_module()
        legacy = tmp / "config" / "mackes-shell"
        target = tmp / "config" / "mde"
        legacy.mkdir(parents=True)
        (legacy / "panel.toml").write_text("once")
        assert mod.migrate_pair(legacy, target) == "moved"
        # Re-run — legacy is gone, target exists, so it's a noop.
        assert mod.migrate_pair(legacy, target) == "noop"
        assert (target / "panel.toml").read_text() == "once"
    _with_tmpdir(body)


def test_main_processes_every_pair_and_returns_zero_on_success():
    def body(tmp):
        mod = _load_module()
        # Build a synthetic pair list pointing at the tmpdir so we
        # don't touch the real $HOME.
        pairs = [
            (tmp / "config" / "mackes-shell", tmp / "config" / "mde"),
            (tmp / "cache" / "mackes",        tmp / "cache" / "mde"),
        ]
        # Only one legacy tree exists — the other should be noop.
        (tmp / "config" / "mackes-shell").mkdir(parents=True)
        (tmp / "config" / "mackes-shell" / "x.toml").write_text("x")
        code = mod.main(pairs)
        assert code == 0
        assert (tmp / "config" / "mde" / "x.toml").exists()
        assert not (tmp / "cache" / "mde").exists()
    _with_tmpdir(body)


def test_on_same_filesystem_returns_true_for_same_parent_dir():
    def body(tmp):
        mod = _load_module()
        # Both paths under the same tmpdir -> same FS.
        a = tmp / "a" / "x"
        b = tmp / "b" / "y"
        (tmp / "a").mkdir()
        (tmp / "b").mkdir()
        assert mod._on_same_filesystem(a, b) is True
    _with_tmpdir(body)


def test_on_same_filesystem_handles_missing_parent_gracefully():
    def body(tmp):
        mod = _load_module()
        a = tmp / "does" / "not" / "exist" / "x"
        b = tmp / "also" / "missing" / "y"
        # Missing parents -> returns False (no crash).
        assert mod._on_same_filesystem(a, b) is False
    _with_tmpdir(body)
