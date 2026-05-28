"""Run the tests suite without pytest installed.

Walks tests/test_*.py, imports each, runs every test_* function. Shims
`pytest.raises` and `pytest.fixture` so most existing tests pass; tests that
rely on tmp_path / monkeypatch fixtures are skipped with a reason.

Use:  python3 tests/_run_without_pytest.py
Or:   make test-nodeps
"""
from __future__ import annotations

import importlib
import importlib.util
import inspect
import sys
import traceback
import types
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO_ROOT = ROOT.parent

# Prefer the repo's mackes over any installed mackes package
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))
# Evict any pre-imported system mackes module
for mod in list(sys.modules):
    if mod == "mackes" or mod.startswith("mackes."):
        del sys.modules[mod]


# ---- pytest shim ----------------------------------------------------------


class _Raises:
    def __init__(self, exc_type, match=None):
        self.exc_type = exc_type
        self.match = match
        self.value = None
    def __enter__(self):
        return self
    def __exit__(self, et, ev, tb):
        if et is None:
            raise AssertionError(f"expected {self.exc_type.__name__}")
        if not issubclass(et, self.exc_type):
            return False
        self.value = ev
        if self.match and self.match not in str(ev):
            raise AssertionError(f"message {ev!r} missing {self.match!r}")
        return True


def _fixture(*args, **kwargs):
    """Marker only — fixtures aren't actually wired."""
    def deco(fn):
        fn._is_fixture = True
        return fn
    if args and callable(args[0]):
        return deco(args[0])
    return deco


def _mark_skip(reason: str):
    def deco(fn):
        fn._skip_reason = reason
        return fn
    return deco


def _importorskip(modname, *args, **kwargs):
    try:
        return importlib.import_module(modname)
    except ImportError as e:
        raise _Skip(f"missing dep {modname}: {e}")


class _Mark:
    """Minimal pytest.mark shim. parametrize is a no-op decorator that
    leaves the function callable with default args; tests that depend on
    real parametrization are then skipped by the fixture detector
    (parameter names look like fixtures to it)."""
    @staticmethod
    def parametrize(*_args, **_kwargs):
        def deco(fn):
            fn._parametrized = True
            return fn
        return deco

    def __getattr__(self, _name):
        def deco(fn=None, *args, **kwargs):
            if callable(fn):
                return fn
            def inner(real):
                return real
            return inner
        return deco


def _fail(msg=""):
    raise AssertionError(msg)


def _install_shim() -> None:
    pytest = types.ModuleType("pytest")
    pytest.raises = _Raises
    pytest.fixture = _fixture
    # pytest's real `skip()` accepts `allow_module_level=True` so a
    # test module can short-circuit at import time. The shim ignores
    # the kwarg (every skip path here raises _Skip identically) but
    # must accept it to match the call sites.
    pytest.skip = lambda reason="", **_kwargs: (_ for _ in ()).throw(_Skip(reason))
    pytest.importorskip = _importorskip
    pytest.mark = _Mark()
    pytest.fail = _fail
    sys.modules["pytest"] = pytest


class _Skip(Exception):
    pass


# ---- Test discovery + execution ------------------------------------------


def _load(path: Path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _runs_with_no_args(fn) -> bool:
    sig = inspect.signature(fn)
    return len(sig.parameters) == 0


def main() -> int:
    _install_shim()
    pass_n = fail_n = skip_n = 0
    failed: list[tuple[str, str]] = []
    test_files = sorted(ROOT.glob("test_*.py"))
    if not test_files:
        print("(no tests found)")
        return 0
    for tf in test_files:
        try:
            mod = _load(tf)
        except _Skip as e:
            # Module-level `pytest.skip(..., allow_module_level=True)`
            # raises _Skip during exec_module. That's a deliberate
            # skip of the whole file (e.g. no $DISPLAY, no GTK typelib),
            # not a load failure — count it under skips.
            skip_n += 1
            print(f"  SKIP {tf.name} (module-level: {e})")
            continue
        except Exception as e:  # noqa: BLE001
            print(f"  LOAD-FAIL {tf.name}: {e}")
            fail_n += 1
            failed.append((tf.name, str(e)))
            continue
        for name, fn in inspect.getmembers(mod, inspect.isfunction):
            if not name.startswith("test_"):
                continue
            if not _runs_with_no_args(fn):
                # needs a fixture we don't have — skip
                skip_n += 1
                print(f"  SKIP {tf.name}::{name} (fixture: "
                      f"{','.join(inspect.signature(fn).parameters)})")
                continue
            try:
                fn()
                pass_n += 1
                print(f"  PASS {tf.name}::{name}")
            except _Skip as e:
                skip_n += 1
                print(f"  SKIP {tf.name}::{name} ({e})")
            except Exception as e:  # noqa: BLE001
                fail_n += 1
                print(f"  FAIL {tf.name}::{name}: {e}")
                traceback.print_exc()
                failed.append((f"{tf.name}::{name}", str(e)))

    print()
    print(f"{pass_n} passed · {skip_n} skipped · {fail_n} failed")
    return 1 if fail_n else 0


if __name__ == "__main__":
    sys.exit(main())
