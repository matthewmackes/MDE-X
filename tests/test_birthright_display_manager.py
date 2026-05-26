"""Tests for DM-5 — ``apply_display_manager`` in ``mackes/birthright.py``.

The step's job is the LightDM → greetd swap. It probes the current
systemd state with non-privileged ``systemctl is-enabled`` /
``systemctl is-active`` (``_run``) and applies changes with the
privileged variants (``_run_root``) only when needed.

These tests cover (each path acceptance-locked in the DM-5 task body):

  1. lightdm-installed-and-active: disables + stops + enables
     greetd + starts greetd + sets default target.
  2. lightdm-not-installed: skips disable; enables + starts greetd;
     sets default target.
  3. greetd-already-enabled-and-active: re-run is a no-op (no
     ``_run_root`` calls at all beyond the set-default check that
     also short-circuits when the default is already graphical).
  4. profile=lighthouse: returns immediately with skip message;
     zero ``_run`` and ``_run_root`` calls.
  5. profile=headless: same skip semantics as lighthouse.
  6. active-graphical-session defers ``systemctl start greetd``:
     greetd is enabled but NOT started; deferred-message logged.
  7. ``systemctl disable lightdm`` fails: surfaces the rc.
  8. ``systemctl set-default graphical.target`` fails: surfaces
     the rc.
  9. greetd not installed (DM-1 didn't run): aborts the swap with
     a clear log line.
"""
from __future__ import annotations

from typing import Any, Dict, List, Tuple

from mackes import birthright


class _FakePreset:
    """Stand-in for ``mackes.birthright.Preset`` carrying the
    fields apply_display_manager reads. Plain class (no @dataclass)
    because tests/_run_without_pytest.py loads test files via
    importlib without registering them in sys.modules, which breaks
    @dataclass's KW_ONLY-introspection on Python 3.14+.
    """

    def __init__(
        self,
        profile: str = "full",
        name: str = "test-preset",
        appearance: Dict[str, Any] | None = None,
        network: Dict[str, Any] | None = None,
    ) -> None:
        self.profile = profile
        self.name = name
        self.appearance = appearance or {}
        self.network = network or {}


class _RunRecorder:
    """Replaces ``birthright._run`` or ``birthright._run_root``
    with a deterministic mock. Records every call; replies via a
    list of ``(argv-prefix-match, (rc, stdout))`` pairs (first
    match wins). Unmatched commands → ``(0, "")``.
    """

    def __init__(
        self,
        responses: List[Tuple[List[str], Tuple[int, str]]] | None = None,
    ) -> None:
        self.responses = responses or []
        self.calls: List[List[str]] = []

    def __call__(self, cmd: List[str], *, timeout: int = 60) -> Tuple[int, str]:
        self.calls.append(cmd[:])
        for prefix, response in self.responses:
            if cmd[: len(prefix)] == prefix:
                return response
        return 0, ""


def _patch(monkeypatch, run: _RunRecorder, run_root: _RunRecorder) -> None:
    monkeypatch.setattr(birthright, "_run", run)
    monkeypatch.setattr(birthright, "_run_root", run_root)


# Probe responses helpers — composed by tests so each scenario is readable.

def _r_lightdm_present_enabled_active() -> List[Tuple[List[str], Tuple[int, str]]]:
    return [
        (["systemctl", "list-unit-files", "lightdm.service"], (0, "lightdm.service enabled\n")),
        (["systemctl", "is-enabled", "lightdm.service"], (0, "enabled\n")),
        (["systemctl", "is-active", "lightdm.service"], (0, "active\n")),
        (["systemctl", "list-unit-files", "greetd.service"], (0, "greetd.service disabled\n")),
        (["systemctl", "is-enabled", "greetd.service"], (1, "disabled\n")),
        (["systemctl", "is-active", "greetd.service"], (3, "inactive\n")),
        # graphical.target NOT active → safe to stop lightdm + start greetd
        (["systemctl", "is-active", "graphical.target"], (3, "inactive\n")),
        (["systemctl", "get-default"], (0, "multi-user.target\n")),
    ]


def _r_no_lightdm_greetd_disabled() -> List[Tuple[List[str], Tuple[int, str]]]:
    return [
        (["systemctl", "list-unit-files", "lightdm.service"], (0, "")),
        (["systemctl", "list-unit-files", "greetd.service"], (0, "greetd.service disabled\n")),
        (["systemctl", "is-enabled", "greetd.service"], (1, "disabled\n")),
        (["systemctl", "is-active", "greetd.service"], (3, "inactive\n")),
        (["systemctl", "is-active", "graphical.target"], (3, "inactive\n")),
        (["systemctl", "get-default"], (0, "multi-user.target\n")),
    ]


def _r_all_already_converged() -> List[Tuple[List[str], Tuple[int, str]]]:
    return [
        (["systemctl", "list-unit-files", "lightdm.service"], (0, "")),
        (["systemctl", "list-unit-files", "greetd.service"], (0, "greetd.service enabled\n")),
        (["systemctl", "is-enabled", "greetd.service"], (0, "enabled\n")),
        (["systemctl", "is-active", "greetd.service"], (0, "active\n")),
        (["systemctl", "get-default"], (0, "graphical.target\n")),
    ]


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_lightdm_installed_and_active_full_swap_executes(monkeypatch):
    run = _RunRecorder(_r_lightdm_present_enabled_active())
    run_root = _RunRecorder()
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="full"))
    msg = " | ".join(out)
    assert "lightdm.service disabled" in msg
    assert "lightdm.service stopped" in msg
    assert "greetd.service enabled" in msg
    assert "greetd.service started" in msg
    assert "default target set to graphical.target" in msg
    # All four privileged actions land via _run_root.
    root_argvs = [c[:2] for c in run_root.calls]
    assert ["systemctl", "disable"] in root_argvs
    assert ["systemctl", "stop"] in root_argvs
    assert ["systemctl", "enable"] in root_argvs
    assert ["systemctl", "start"] in root_argvs
    assert ["systemctl", "set-default"] in root_argvs


def test_lightdm_not_installed_skips_disable(monkeypatch):
    run = _RunRecorder(_r_no_lightdm_greetd_disabled())
    run_root = _RunRecorder()
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="full"))
    msg = " | ".join(out)
    assert "lightdm.service not installed" in msg
    assert "greetd.service enabled" in msg
    assert "greetd.service started" in msg
    # No `disable lightdm.service` shell-out happened.
    root_argvs = [c[:3] for c in run_root.calls]
    assert ["systemctl", "disable", "lightdm.service"] not in root_argvs


def test_re_run_on_converged_peer_is_noop(monkeypatch):
    run = _RunRecorder(_r_all_already_converged())
    run_root = _RunRecorder()
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="full"))
    msg = " | ".join(out)
    assert "lightdm.service not installed" in msg
    assert "greetd.service already enabled" in msg
    assert "greetd.service already active" in msg
    assert "default target already graphical.target" in msg
    # Zero privileged calls — nothing needed changing.
    assert run_root.calls == []


def test_lighthouse_profile_skips_with_log_line(monkeypatch):
    run = _RunRecorder()
    run_root = _RunRecorder()
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="lighthouse"))
    assert any("no graphical target" in line for line in out)
    assert run.calls == []
    assert run_root.calls == []


def test_headless_profile_skips_with_log_line(monkeypatch):
    run = _RunRecorder()
    run_root = _RunRecorder()
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="headless"))
    assert any("no graphical target" in line for line in out)
    assert run.calls == []
    assert run_root.calls == []


def test_active_graphical_session_defers_starting_greetd(monkeypatch):
    responses = [
        (["systemctl", "list-unit-files", "lightdm.service"], (0, "")),
        (["systemctl", "list-unit-files", "greetd.service"], (0, "greetd.service disabled\n")),
        (["systemctl", "is-enabled", "greetd.service"], (1, "disabled\n")),
        (["systemctl", "is-active", "greetd.service"], (3, "inactive\n")),
        # graphical.target ACTIVE → defer the start
        (["systemctl", "is-active", "graphical.target"], (0, "active\n")),
        (["systemctl", "get-default"], (0, "graphical.target\n")),
    ]
    run = _RunRecorder(responses)
    run_root = _RunRecorder()
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="full"))
    msg = " | ".join(out)
    assert "deferring `systemctl start greetd.service`" in msg
    # greetd should have been ENABLED but NOT started.
    root_argvs = [c[:3] for c in run_root.calls]
    assert ["systemctl", "enable", "greetd.service"] in root_argvs
    assert ["systemctl", "start", "greetd.service"] not in root_argvs


def test_disable_lightdm_failure_surfaces_rc(monkeypatch):
    run = _RunRecorder(_r_lightdm_present_enabled_active())
    # Privileged disable returns failure.
    run_root = _RunRecorder(
        responses=[(["systemctl", "disable", "lightdm.service"], (1, "Failed"))]
    )
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="full"))
    msg = " | ".join(out)
    assert "lightdm.service disable failed" in msg


def test_set_default_failure_surfaces_rc(monkeypatch):
    run = _RunRecorder(_r_no_lightdm_greetd_disabled())
    run_root = _RunRecorder(
        responses=[(["systemctl", "set-default", "graphical.target"], (1, "err"))]
    )
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="full"))
    msg = " | ".join(out)
    assert "set-default failed" in msg


def test_greetd_not_installed_aborts_swap(monkeypatch):
    responses = [
        (["systemctl", "list-unit-files", "lightdm.service"], (0, "")),
        # greetd.service not in unit files → DM-1 didn't run
        (["systemctl", "list-unit-files", "greetd.service"], (0, "")),
    ]
    run = _RunRecorder(responses)
    run_root = _RunRecorder()
    _patch(monkeypatch, run, run_root)
    out = birthright.apply_display_manager(_FakePreset(profile="full"))
    msg = " | ".join(out)
    assert "greetd.service not installed" in msg
    assert "aborting swap" in msg
    # No `enable` or `start` shell-out happened.
    root_argvs = [c[:2] for c in run_root.calls]
    assert ["systemctl", "enable"] not in root_argvs
    assert ["systemctl", "start"] not in root_argvs
