"""Mackes Desktop Environment (MDE) — Python package.

Phase 0.10 (transitional) — `mde` is a thin re-export facade over
the legacy `mackes` package during the v2.0.0 back-compat window.
This lets new code call

    from mde import wizard
    from mde.mde_settings_bridge import set_setting

without touching the existing `from mackes.X` import sites. Once
every `from mackes.X` is converted to `from mde.X` (Phase F.x
panel ports as each one rewrites), the `mackes/` directory
retires at v2.1 cut and the facade goes with it.

How the facade works:

  * `mde` is a real Python package (this `__init__.py` exists)
    so `import mde` succeeds.
  * It re-exports `mackes.__version__` so `mde.__version__`
    reads the same source of truth.
  * For sub-modules, we use `sys.modules` aliasing in
    `_install_facade()` so `import mde.foo` resolves to
    `mackes.foo`. This means `from mde.foo import bar` works
    AND `import mde; mde.foo.bar()` works, with `mde.foo is
    mackes.foo`.

The facade is intentionally one-way: code can use either name,
but the canonical module identity stays at `mackes.X` for the
duration of the back-compat window so a single panel doesn't
hit "two copies of the same module" subtleties.
"""
from __future__ import annotations

import importlib
import sys
from typing import Iterable

import mackes

# Re-export the version so `mde.__version__` works the same way
# `mackes.__version__` does. Pinned to the live mackes module so
# the cut-release flow only has to update one file.
__version__: str = mackes.__version__


def _install_facade(submodules: Iterable[str]) -> None:
    """Alias each `mackes.<name>` submodule into `sys.modules`
    as `mde.<name>` so `from mde.<name> import …` resolves to
    the same module object as `mackes.<name>`. Also set the
    attribute on the `mde` package so `mde.<name>` works after
    `import mde`."""
    me = sys.modules[__name__]
    for name in submodules:
        full_legacy = f"mackes.{name}"
        full_new = f"mde.{name}"
        # Ensure the legacy module is loaded; ignore optional
        # ones that aren't available (e.g. GTK-only modules
        # in a headless environment).
        try:
            mod = importlib.import_module(full_legacy)
        except ImportError:
            continue
        sys.modules[full_new] = mod
        # Set the attribute on the package so `mde.X` resolves
        # without requiring a prior `from mde import X`.
        setattr(me, name, mod)


# The set of top-level mackes/* submodules to alias. New
# additions land here whenever a new mackes.X is added; the
# duplication is the price of an explicit allow-list (we don't
# auto-walk pkgutil because that drags in every optional GTK
# module at import time, slowing the headless paths).
_FACADE_SUBMODULES: tuple[str, ...] = (
    "about",
    "admin_session",
    "app",
    "app_mgmt",
    "audio",
    "help_utils",
    "birthright",
    "birthright_check",
    "birthright_rollback",
    "clipboard_app",
    "drawer",
    "headless",
    "legacy_import",
    "lightdm",
    "mackesd_bridge",
    "mde_settings_bridge",
    "mesh_browser",
    "mesh_discovery",
    "mesh_fs",
    "mesh_fs_fuse",
    "mesh_mdns",
    "mesh_media",
    "mesh_metrics",
    "mesh_nats",
    "mesh_notifications",
    "mesh_services",
    "mesh_ssh",
    "mesh_sync",
    "mesh_thumbnailer",
    "mesh_vpn",
    "mesh_wol",
    "presets",
    "recover",
    "snapshots",
    "state",
    "uninstall",
    "wizard",
    "xfconf_bridge",
)


_install_facade(_FACADE_SUBMODULES)


__all__ = ["__version__"]
