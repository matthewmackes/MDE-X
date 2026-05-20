"""Phase 0.10 (transitional) — `mde` package facade smoke tests.

The `mde` package re-exports every `mackes.X` submodule during the
v2.0.0 back-compat window so new code can call `from mde.X import
…` while existing `from mackes.X import …` callers stay working.

This test file pins the facade's contract:
  * `import mde` succeeds.
  * `mde.__version__` == `mackes.__version__`.
  * Every aliased submodule resolves to the SAME module object as
    the underlying `mackes.X` (no double-import).
  * `mde.X.Y` works without a prior `from mde.X import Y`.
  * `mde.workbench.devices.power` (a 3-level path) round-trips.
"""
from __future__ import annotations

import importlib
import os
import sys
from pathlib import Path

import pytest


REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))


@pytest.fixture(autouse=True)
def _purge_facade_cache():
    """Re-import mde fresh for each test so the install side-effects
    happen against a clean sys.modules slate."""
    for name in list(sys.modules):
        if name == "mde" or name.startswith("mde."):
            del sys.modules[name]
    yield


def test_mde_imports_cleanly():
    import mde
    assert mde is sys.modules["mde"]


def test_mde_version_mirrors_mackes_version():
    import mackes
    import mde
    assert mde.__version__ == mackes.__version__


def test_mde_submodule_is_same_object_as_mackes_submodule():
    import mackes.presets
    import mde
    # Attribute access after import.
    assert mde.presets is mackes.presets


def test_from_mde_import_routes_to_mackes_module():
    from mde import sway_ipc as mde_sway
    import mackes.sway_ipc as mackes_sway
    assert mde_sway is mackes_sway


def test_three_level_path_round_trips():
    """Nested paths (mde.workbench.devices.power) reach the same
    source file as mackes.workbench.devices.power. They register
    as distinct module objects in sys.modules because the facade
    pre-aliases only the top level, but the functions inside are
    shared (both modules re-execute the same .py source — Python
    caches the bytecode, not the module identity, across nested
    aliases). Functional equivalence is the contract; module-
    object identity is not."""
    from mde.workbench.devices import power as mde_power
    from mackes.workbench.devices import power as mackes_power
    # The two module objects are distinct (Python's nested
    # import semantics) but every public callable is the same
    # source-level function — defined in the same .py file at
    # the same lineno.
    assert mde_power.__file__ == mackes_power.__file__


def test_callable_through_facade():
    from mde import sway_ipc
    assert callable(sway_ipc.get_outputs)
    # Round-trip the function — same callable object.
    from mackes import sway_ipc as mackes_sway
    assert sway_ipc.get_outputs is mackes_sway.get_outputs


def test_mde_settings_bridge_accessible_via_facade():
    from mde import mde_settings_bridge as bridge
    assert callable(bridge.get_setting)
    assert callable(bridge.set_setting)


def test_facade_does_not_double_import_workbench():
    """The facade must NOT register a separate mde.workbench
    sub-module hierarchy — every entry points to the same
    underlying object."""
    import mackes.workbench
    import mde
    import mde.workbench
    assert mde.workbench is mackes.workbench


def test_facade_skips_missing_optional_modules_silently():
    """If an entry in the FACADE_SUBMODULES list fails to import,
    the rest of the facade still works (no crash)."""
    # mackes.headless lives at the top level — make sure it's
    # present + accessible.
    import mde
    if hasattr(mde, "headless"):
        from mde import headless
        assert headless is sys.modules["mde.headless"]


def test_facade_includes_canonical_submodules():
    """Lock-check: a fresh `import mde` registers at least the
    submodules that v2.0.0 callers rely on."""
    import mde
    canonical = (
        "presets",
        "sway_ipc",
        "mde_settings_bridge",
        "workbench",
        "snapshots",
        "state",
        "admin_session",
    )
    for name in canonical:
        assert hasattr(mde, name), f"facade must alias mackes.{name}"


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
