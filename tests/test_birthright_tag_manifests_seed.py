"""Tests for HYP-8.5.birthright — `apply_tag_manifests_seed` in
`mackes/birthright.py`.

The v6.5 Hyprland tag-driven workspace system reads operator tag
manifests from `~/.config/mde/tags/<name>.toml`. Without the
birthright copy step, fresh installs boot with zero manifests in
the operator's home + the mackesd tag_manifest loader publishes
zero `event/config/tags/loaded` events.

These tests cover:

  1. Happy-path first-login: copies every *.toml from
     /usr/share/mde/tag-manifests/ to ~/.config/mde/tags/.
  2. Idempotency: re-runs preserve existing destination files
     (operator edits survive).
  3. Missing source dir: returns a "no seeds to copy" log line
     without raising.
  4. Empty source dir: returns "no *.toml seeds" log line.
  5. Destination dir auto-creation: parent dirs are mkdir'd.
  6. Mixed state: some files copied, others already present.

Each test monkeypatches `os.path.expanduser` to redirect $HOME
into a tmpdir + uses `tmp_path` for the source dir so the suite
never touches the host's real filesystem outside the test tree.
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


def _patch_src_dir(monkeypatch, src: Path) -> None:
    """Redirect the hardcoded /usr/share/mde/tag-manifests path
    to `src` so the test doesn't depend on RPM-installed files."""
    real_path = birthright.Path

    class _FakePath(real_path):
        def __new__(cls, *args, **kwargs):
            # Intercept the exact src path used by the function.
            if args and args[0] == "/usr/share/mde/tag-manifests":
                return real_path(src)
            return real_path.__new__(cls, *args, **kwargs)

    monkeypatch.setattr(birthright, "Path", _FakePath)


def _write_seed(dir: Path, name: str, body: str = "name = \"x\"\n") -> Path:
    dir.mkdir(parents=True, exist_ok=True)
    path = dir / f"{name}.toml"
    path.write_text(body, encoding="utf-8")
    return path


def test_first_login_copies_every_seed(tmp_path, monkeypatch):
    """All system seeds land in ~/.config/mde/tags/ on first run."""
    home = tmp_path / "home"
    home.mkdir()
    src = tmp_path / "share"
    _write_seed(src, "voip")
    _write_seed(src, "dev")
    _write_seed(src, "hub")
    _patch_home(monkeypatch, home)
    _patch_src_dir(monkeypatch, src)

    actions = birthright.apply_tag_manifests_seed(None)

    dst_dir = home / ".config" / "mde" / "tags"
    assert (dst_dir / "voip.toml").is_file()
    assert (dst_dir / "dev.toml").is_file()
    assert (dst_dir / "hub.toml").is_file()
    # One log line per copied file.
    assert any("voip.toml" in a for a in actions)
    assert any("dev.toml" in a for a in actions)
    assert any("hub.toml" in a for a in actions)


def test_idempotent_preserves_existing(tmp_path, monkeypatch):
    """Re-runs leave operator-edited destination files alone."""
    home = tmp_path / "home"
    home.mkdir()
    src = tmp_path / "share"
    _write_seed(src, "voip", body="name = \"system\"\n")
    _patch_home(monkeypatch, home)
    _patch_src_dir(monkeypatch, src)

    dst_dir = home / ".config" / "mde" / "tags"
    dst_dir.mkdir(parents=True)
    (dst_dir / "voip.toml").write_text("name = \"operator-edit\"\n", encoding="utf-8")

    actions = birthright.apply_tag_manifests_seed(None)

    # Operator edit survives.
    assert (dst_dir / "voip.toml").read_text() == "name = \"operator-edit\"\n"
    # Log line reflects the skip.
    assert any("already present" in a for a in actions)


def test_missing_source_dir_returns_clean(tmp_path, monkeypatch):
    """Dev-checkout layouts (no /usr/share) skip silently."""
    home = tmp_path / "home"
    home.mkdir()
    src = tmp_path / "nonexistent-share"  # Never created.
    _patch_home(monkeypatch, home)
    _patch_src_dir(monkeypatch, src)

    actions = birthright.apply_tag_manifests_seed(None)
    assert any("source dir" in a and "missing" in a for a in actions)
    # No destination created.
    assert not (home / ".config" / "mde" / "tags").exists()


def test_empty_source_dir_returns_no_seeds(tmp_path, monkeypatch):
    """Source dir exists but holds no *.toml files."""
    home = tmp_path / "home"
    home.mkdir()
    src = tmp_path / "share"
    src.mkdir()
    # Drop a non-TOML to confirm extension filtering.
    (src / "readme.md").write_text("ignore", encoding="utf-8")
    _patch_home(monkeypatch, home)
    _patch_src_dir(monkeypatch, src)

    actions = birthright.apply_tag_manifests_seed(None)
    assert any("no *.toml seeds" in a for a in actions)


def test_destination_dir_auto_created(tmp_path, monkeypatch):
    """~/.config/mde/tags/ is mkdir'd on first run."""
    home = tmp_path / "home"
    home.mkdir()
    src = tmp_path / "share"
    _write_seed(src, "voip")
    _patch_home(monkeypatch, home)
    _patch_src_dir(monkeypatch, src)

    dst_dir = home / ".config" / "mde" / "tags"
    assert not dst_dir.exists()

    birthright.apply_tag_manifests_seed(None)

    assert dst_dir.is_dir()
    assert (dst_dir / "voip.toml").is_file()


def test_mixed_state_partial_copy(tmp_path, monkeypatch):
    """Some files already present, others to copy — both paths fire."""
    home = tmp_path / "home"
    home.mkdir()
    src = tmp_path / "share"
    _write_seed(src, "voip")
    _write_seed(src, "dev")
    _write_seed(src, "hub")
    _patch_home(monkeypatch, home)
    _patch_src_dir(monkeypatch, src)

    # Pre-seed dev.toml so it's "already present."
    dst_dir = home / ".config" / "mde" / "tags"
    dst_dir.mkdir(parents=True)
    (dst_dir / "dev.toml").write_text("name = \"existing\"\n", encoding="utf-8")

    actions = birthright.apply_tag_manifests_seed(None)

    # voip + hub copied; dev preserved.
    assert (dst_dir / "voip.toml").is_file()
    assert (dst_dir / "hub.toml").is_file()
    assert (dst_dir / "dev.toml").read_text() == "name = \"existing\"\n"
    # Mixed log lines.
    copied = [a for a in actions if "copied" in a]
    skipped = [a for a in actions if "already present" in a]
    assert len(copied) == 2
    assert len(skipped) == 1
