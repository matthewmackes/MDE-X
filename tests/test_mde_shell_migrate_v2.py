"""Tests for v2.0.0 Phase H.5 mde-shell-migrate-v2."""
from __future__ import annotations

import importlib.machinery
import importlib.util
import os
import tempfile
from pathlib import Path


def _load_module():
    repo = Path(__file__).resolve().parent.parent
    path = repo / "bin" / "mde-shell-migrate-v2"
    loader = importlib.machinery.SourceFileLoader(
        "mde_shell_migrate_v2", str(path),
    )
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def _with_home(fn):
    tmp = tempfile.TemporaryDirectory()
    try:
        old_home = os.environ.get("HOME")
        os.environ["HOME"] = tmp.name
        try:
            return fn(Path(tmp.name))
        finally:
            if old_home is None:
                del os.environ["HOME"]
            else:
                os.environ["HOME"] = old_home
    finally:
        tmp.cleanup()


def test_step_2_removes_only_mackes_generated_overrides():
    def body(home):
        mod = _load_module()
        autostart = home / ".config" / "autostart"
        autostart.mkdir(parents=True)
        # MDE-generated suppression file (should be removed).
        (autostart / "mackes-suppress-xfce4-panel.desktop").write_text(
            "[Desktop Entry]\nHidden=true\nType=Application\n"
        )
        # Vendor file without Hidden=true (should NOT be removed,
        # even though name matches xfdesktop).
        (autostart / "xfdesktop.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Other\n"
        )
        removed = mod.step_2_remove_xdg_autostart_overrides()
        assert removed == 1
        assert not (autostart / "mackes-suppress-xfce4-panel.desktop").exists()
        assert (autostart / "xfdesktop.desktop").exists(), \
            "non-Hidden vendor file must survive"
    _with_home(body)


def test_step_2_handles_missing_autostart_dir():
    def body(home):
        mod = _load_module()
        # No autostart dir created.
        assert mod.step_2_remove_xdg_autostart_overrides() == 0
    _with_home(body)


def test_step_3_backs_up_xfce4_config_when_present():
    def body(home):
        mod = _load_module()
        src = home / ".config" / "xfce4"
        src.mkdir(parents=True)
        (src / "xfce-perchannel-xml").mkdir()
        (src / "xfce-perchannel-xml" / "xsettings.xml").write_text("<x/>")
        path = mod.step_3_backup_xfce4_config()
        assert path is not None
        backup = Path(path)
        assert backup.exists()
        assert (backup / "xfce-perchannel-xml" / "xsettings.xml").exists()
    _with_home(body)


def test_step_3_no_op_when_xfce4_config_missing():
    def body(home):
        mod = _load_module()
        assert mod.step_3_backup_xfce4_config() is None
    _with_home(body)


def test_step_4_writes_sway_config_from_source_tree():
    """Phase H.5 step 4 walks both /usr/share/mde/sway/ and the
    in-tree data/sway/ — the latter is what makes this testable in
    a checked-out repo without installing the RPM."""
    def body(home):
        mod = _load_module()
        # Pre-condition: no ~/.config/sway/ yet.
        assert not (home / ".config" / "sway").exists()
        wrote = mod.step_4_write_default_sway_config()
        # data/sway/ exists in the source tree (we shipped it in
        # Phase D.5), so this MUST write.
        assert wrote, "step 4 should write the in-tree data/sway/"
        assert (home / ".config" / "sway" / "config").is_file()
    _with_home(body)


def test_step_4_skips_when_user_already_has_sway_config():
    def body(home):
        mod = _load_module()
        (home / ".config" / "sway").mkdir(parents=True)
        (home / ".config" / "sway" / "config").write_text("user-config")
        wrote = mod.step_4_write_default_sway_config()
        assert not wrote, "step 4 must not clobber existing user config"
        # User content preserved.
        assert (home / ".config" / "sway" / "config").read_text() == "user-config"
    _with_home(body)


def test_xfconf_to_mde_key_map_only_includes_transferrable_keys():
    mod = _load_module()
    # The lock list is fixed; sanity-check the shape so future
    # additions go through an intentional code edit.
    for (channel, prop), mde_key in mod.XFCONF_TO_MDE_KEY.items():
        assert channel  # non-empty
        assert prop.startswith("/")
        assert "." in mde_key, f"MDE key {mde_key} must be dot-notated"
        # Every mapped key is in one of the known applier families.
        assert mde_key.split(".")[0] in {
            "theme", "font", "display", "power", "notification",
            "automount", "wallpaper", "keybinds", "autostart",
        }


def test_main_returns_zero_and_is_idempotent():
    def body(home):
        mod = _load_module()
        # First run on a clean tree.
        assert mod.main() == 0
        # Re-run is a no-op (sway config already written, no
        # xfconf, no autostart overrides) — still exits 0.
        assert mod.main() == 0
    _with_home(body)
