"""Tests for GF-3.2 — `apply_gluster_bootstrap` in `mackes/birthright.py`.

The wizard apply step's job is operator-visibility: probe whether
the v5.0.0 gluster substrate is in place + report what the
`mackesd::workers::gluster_worker` daemon will do on its next
tick. It does NOT bootstrap the mesh-home volume itself (that's
GF-2.4, owned by the daemon).

These tests cover:

  1. CLI not installed: clean "v5.0.0 substrate inactive" log
     line; zero subprocess calls beyond the `shutil.which` probe.
  2. glusterd not active: clear "try systemctl enable --now"
     instruction.
  3. `gluster pool list` fails: surfaces the last-line error.
  4. Volume already exists: reports "gluster_worker bootstrapped
     it on a previous tick."
  5. Volume not yet created: reports "will bootstrap on next
     tick" + names the overlay-ip dependency.

Every test monkeypatches `shutil.which` (so the CLI-not-installed
branch can be exercised on a host with gluster installed) +
`birthright._run` (the subprocess wrapper).
"""
from __future__ import annotations

from typing import Any, Dict, List, Tuple

from mackes import birthright


class _RunRecorder:
    """Replaces `birthright._run` with a deterministic mock.

    The script `responses` field is a list of `(argv-prefix-match,
    (rc, stdout))` pairs — the FIRST matching prefix wins.
    Unmatched commands return `(0, "")` (success-with-empty-output).
    """

    def __init__(
        self, responses: List[Tuple[List[str], Tuple[int, str]]] | None = None
    ) -> None:
        self.responses = responses or []
        self.calls: List[List[str]] = []

    def __call__(self, cmd: List[str], *, timeout: int = 60) -> Tuple[int, str]:
        self.calls.append(cmd[:])
        for prefix, response in self.responses:
            if cmd[: len(prefix)] == prefix:
                return response
        return 0, ""


def _patch_run(monkeypatch, recorder: _RunRecorder) -> None:
    monkeypatch.setattr(birthright, "_run", recorder)


def _patch_which(monkeypatch, available: Dict[str, str | None]) -> None:
    """Override `shutil.which` so the test can simulate
    gluster-installed vs gluster-not-installed without mutating
    the host PATH."""
    import shutil

    real_which = shutil.which

    def fake_which(name: str, mode: int = 1, path: str | None = None) -> str | None:
        if name in available:
            return available[name]
        return real_which(name, mode, path)

    monkeypatch.setattr(shutil, "which", fake_which)


def _dummy_preset() -> Any:
    return object()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_gluster_cli_not_installed_reports_inactive_substrate(monkeypatch):
    _patch_which(monkeypatch, {"gluster": None})
    recorder = _RunRecorder()
    _patch_run(monkeypatch, recorder)

    out = birthright.apply_gluster_bootstrap(_dummy_preset())

    # No subprocess calls — the function should short-circuit.
    assert recorder.calls == []
    assert any("CLI not installed" in line for line in out)
    assert any("v5.0.0 substrate inactive" in line for line in out)


def test_glusterd_not_active_reports_enable_instruction(monkeypatch):
    _patch_which(monkeypatch, {"gluster": "/usr/sbin/gluster"})
    recorder = _RunRecorder(
        responses=[(["systemctl", "is-active"], (3, "inactive\n"))]
    )
    _patch_run(monkeypatch, recorder)

    out = birthright.apply_gluster_bootstrap(_dummy_preset())

    # Only the is-active probe ran; we don't shell pool-list when
    # the service is down.
    assert [c[0] for c in recorder.calls] == ["systemctl"]
    msg = " | ".join(out)
    assert "glusterd.service not active" in msg
    assert "systemctl enable --now" in msg


def test_pool_list_failure_surfaces_last_stderr_line(monkeypatch):
    _patch_which(monkeypatch, {"gluster": "/usr/sbin/gluster"})
    recorder = _RunRecorder(
        responses=[
            (["systemctl", "is-active"], (0, "active\n")),
            (
                ["gluster", "pool", "list"],
                (1, "connection failed\nerror: glusterd is not reachable\n"),
            ),
        ]
    )
    _patch_run(monkeypatch, recorder)

    out = birthright.apply_gluster_bootstrap(_dummy_preset())

    verbs = [c[0] for c in recorder.calls]
    assert verbs == ["systemctl", "gluster"]
    msg = " | ".join(out)
    assert "pool list failed" in msg
    # The last-line of stderr should be quoted in the log.
    assert "glusterd is not reachable" in msg


def test_mesh_home_volume_exists_reports_already_bootstrapped(monkeypatch):
    _patch_which(monkeypatch, {"gluster": "/usr/sbin/gluster"})
    recorder = _RunRecorder(
        responses=[
            (["systemctl", "is-active"], (0, "active\n")),
            (["gluster", "pool", "list"], (0, "Connected\n")),
            (["gluster", "volume", "info"], (0, "Volume Name: mesh-home\n")),
        ]
    )
    _patch_run(monkeypatch, recorder)

    out = birthright.apply_gluster_bootstrap(_dummy_preset())

    # All three probes ran in order.
    assert [c[0] for c in recorder.calls] == [
        "systemctl",
        "gluster",
        "gluster",
    ]
    msg = " | ".join(out)
    assert "glusterd reachable" in msg
    assert "mesh-home volume already exists" in msg
    assert "previous tick" in msg


def test_mesh_home_volume_missing_reports_pending_bootstrap(monkeypatch):
    _patch_which(monkeypatch, {"gluster": "/usr/sbin/gluster"})
    recorder = _RunRecorder(
        responses=[
            (["systemctl", "is-active"], (0, "active\n")),
            (["gluster", "pool", "list"], (0, "Connected\n")),
            (
                ["gluster", "volume", "info"],
                (1, "Volume mesh-home does not exist\n"),
            ),
        ]
    )
    _patch_run(monkeypatch, recorder)

    out = birthright.apply_gluster_bootstrap(_dummy_preset())

    msg = " | ".join(out)
    assert "mesh-home volume not yet created" in msg
    assert "next tick" in msg
    # The body names the overlay-ip dependency so the operator
    # knows what gluster_worker is waiting on.
    assert "overlay-ip" in msg
