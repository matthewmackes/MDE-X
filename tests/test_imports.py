"""Smoke-test: every Mackes module imports without GTK on the system.

Modules that require gi/GTK are skipped via try/except. We're verifying
syntax, import graph, and absence of accidental cycles.
"""
from __future__ import annotations

import importlib
import pkgutil


# Modules pending retirement that are intentionally broken at import time.
# Each entry must be paired with the worklist task that retires the
# module, so the entry can be removed when that task closes.
#
# - `mackes.mesh_gvfs.*` — DEAD-2.12 (v5.2, HW carve-out). The package
#   imports `mackes.mesh_sync` which was deleted by DEAD-2.10 on
#   2026-05-26. mesh_gvfs has zero external consumers (verified via
#   grep) so the broken imports are dead code awaiting the
#   HW-gated DEAD-2.12 bundle that deletes the whole `mesh_gvfs/`
#   directory along with `mesh_fs.py` + the `fs_sync.rs` worker. The
#   `__init__.py` IS importable on its own (re-exports nothing); the
#   three submodules (daemon, fuse_backend, operations) fail with
#   "No module named 'mackes.mesh_sync'".
RETIREMENT_PENDING_PREFIXES = (
    "mackes.mesh_gvfs.",
)


def test_every_non_gui_module_imports():
    failures: list[str] = []
    # Walk the mackes package and try to import everything.
    import mackes
    for finder, name, ispkg in pkgutil.walk_packages(mackes.__path__, prefix="mackes."):
        if any(name.startswith(p) for p in RETIREMENT_PENDING_PREFIXES):
            # Allowlisted: pending retirement per the prefix table above.
            continue
        try:
            importlib.import_module(name)
        except ImportError as e:
            # GUI deps (gi.repository / GTK typelibs) are acceptable to miss
            # on minimal CI containers. Real ImportError-related strings to
            # skip: the explicit "gi"/"Gtk"/"PyGObject" matches, plus the
            # less-obvious "Typelib file for namespace 'xlib' / 'cairo' /
            # 'Gtk' …" that PyGObject raises when a typelib is absent.
            msg = str(e)
            if any(kw in msg for kw in
                   ("gi", "Gtk", "PyGObject", "Typelib", "namespace")):
                continue
            failures.append(f"{name}: {e}")
        except Exception as e:  # noqa: BLE001
            # Same GUI-dep tolerance for non-ImportError exceptions PyGObject
            # may raise when a typelib is missing (e.g. ValueError).
            msg = str(e)
            if any(kw in msg for kw in
                   ("Typelib", "namespace", "gi.repository")):
                continue
            failures.append(f"{name}: {type(e).__name__}: {e}")
    assert not failures, "Import failures:\n" + "\n".join(failures)
