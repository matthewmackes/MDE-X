"""Shared test fixtures.

These tests redirect Mackes' XDG state directories into a tmp_path so they
never touch the developer's real ~/.config or ~/.local/share.
"""
from __future__ import annotations

import importlib
import sys
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture
def isolated_xdg(tmp_path, monkeypatch):
    """Re-root Mackes' XDG paths into tmp_path and reload the state module.

    Returns a dict with 'home', 'config', 'data', 'snapshots', 'logs'.
    """
    home = tmp_path / "home"
    config = home / ".config"
    data = home / ".local" / "share"
    for d in (home, config, data):
        d.mkdir(parents=True, exist_ok=True)

    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(config))
    monkeypatch.setenv("XDG_DATA_HOME", str(data))

    # Re-import state-touching modules so their HOME constants pick up the
    # patched environment variables. Drop the entire `mackes.*` namespace so
    # cached package attributes on `mackes` don't keep pointing at the stale
    # module objects after submodule purge.
    # Important: if we drop `mackes` itself but leave a cached
    # `mackes.mesh_perf` in sys.modules, a later `import mackes.mesh_perf`
    # returns the cached submodule WITHOUT re-binding it on the fresh
    # mackes package — so `mackes.mesh_perf` AttributeErrors. Purge the
    # pure-backend mesh_* / mdns_* / fleet / caddy_* submodules alongside
    # the package itself. Keep mackes.app / mackes.workbench / mackes.wizard
    # OUT because they import GTK and would fail to reload.
    # (v2.0.0 Phase F.10: mackes.menu_integration retired; removed from the
    # purge set.)
    _PURGE_EXACT = {"mackes", "mackes.state", "mackes.logging",
                    "mackes.snapshots",
                    "mackes.presets", "mackes.app_mgmt",
                    "mackes.uninstall", "mackes.xfconf_bridge",
                    "mackes.qnm_bridge", "mackes.fleet"}
    _PURGE_PREFIXES = ("mackes.mesh_", "mackes.mdns_", "mackes.caddy_")
    for mod_name in [m for m in list(sys.modules)
                     if m == "mackes" or m.startswith("mackes.")]:
        if (mod_name in _PURGE_EXACT
                or any(mod_name.startswith(p) for p in _PURGE_PREFIXES)):
            del sys.modules[mod_name]

    sys.path.insert(0, str(REPO_ROOT))
    import mackes.state
    importlib.reload(mackes.state)
    mackes.state.ensure_dirs()

    # Keep the runtime preset lookups inside the dev tree so an
    # already-installed `/usr/share/mackes-shell/` from a previous RPM build
    # doesn't shadow the local repo's data dirs during tests.
    import mackes.presets as _presets
    monkeypatch.setattr(_presets, "SHIPPED_PRESET_DIRS",
                        [REPO_ROOT / "data" / "presets"])

    yield {
        "home": home,
        "config": mackes.state.CONFIG_DIR,
        "data": mackes.state.DATA_DIR,
        "snapshots": mackes.state.SNAPSHOT_DIR,
        "logs": mackes.state.LOG_DIR,
    }
