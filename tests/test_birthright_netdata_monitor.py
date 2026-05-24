"""Tests for MON-1 — `apply_netdata_monitor` in `mackes/birthright.py`.

MON-1's substrate half: writes /etc/netdata/netdata.conf with
the locked baseline params (dbengine memory mode + ~7d
retention + cloud disabled + bind to 127.0.0.1) + triggers a
netdata reload. The dynamic stream-block rewriter that handles
leader-elected aggregator-IP changes is the future MON-1.b
task; this step only ships the substrate.

These tests cover:

  1. netdata CLI not installed: clean "v2.6 substrate inactive"
     log line; zero subprocess + zero file-write calls.
  2. Config already matches the locked baseline: clean "already
     matches" log line; zero file-write + zero reload calls.
  3. Config differs (or missing): atomic-write triggers + reload
     fires.
  4. systemctl reload fails: fall-back restart is attempted.
  5. Both reload + restart fail: clear error log surfacing both
     fall-back attempts.
"""
from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List, Tuple

from mackes import birthright


class _RunRecorder:
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
        return 0, "ok"


def _patch_run_root(monkeypatch, recorder: _RunRecorder) -> None:
    monkeypatch.setattr(birthright, "_run_root", recorder)


def _patch_which(monkeypatch, available: Dict[str, str | None]) -> None:
    import shutil

    real_which = shutil.which

    def fake_which(name: str, mode: int = 1, path: str | None = None) -> str | None:
        if name in available:
            return available[name]
        return real_which(name, mode, path)

    monkeypatch.setattr(shutil, "which", fake_which)


def _patch_config_path(monkeypatch, path: Path) -> None:
    """Replace the hard-coded /etc/netdata/netdata.conf with
    a test-controlled path so the test doesn't need root."""
    real_path = birthright.Path

    class _PathPatched:
        def __init__(self, *args, **kwargs) -> None:
            self._real = real_path(*args, **kwargs)

        def __getattr__(self, name: str) -> Any:
            return getattr(self._real, name)

    # Replace _write_root_file so the test can verify writes
    # without needing actual root.
    written: List[Tuple[Path, str]] = []

    def fake_write_root_file(target: Path, content: str) -> None:
        # Only redirect the netdata.conf write to the test path.
        if str(target) == "/etc/netdata/netdata.conf":
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
            written.append((path, content))
        else:
            # For any other path, mark as a recorded call but
            # don't actually write — keeps the test scoped.
            written.append((target, content))

    monkeypatch.setattr(birthright, "_write_root_file", fake_write_root_file)
    return written


def _patch_read(monkeypatch, path: Path, existing_content: str | None) -> None:
    """Redirect Path('/etc/netdata/netdata.conf').read_text()
    to return synthetic 'already on disk' content."""
    real_read_text = birthright.Path.read_text

    def fake_read_text(self, encoding=None):
        if str(self) == "/etc/netdata/netdata.conf":
            if existing_content is None:
                raise OSError("ENOENT")
            return existing_content
        return real_read_text(self, encoding=encoding) if encoding else real_read_text(self)

    monkeypatch.setattr(birthright.Path, "read_text", fake_read_text, raising=True)


def _dummy_preset() -> Any:
    return object()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_netdata_cli_not_installed_reports_inactive_substrate(monkeypatch, tmp_path):
    _patch_which(monkeypatch, {"netdata": None})
    recorder = _RunRecorder()
    _patch_run_root(monkeypatch, recorder)

    out = birthright.apply_netdata_monitor(_dummy_preset())

    assert recorder.calls == []
    msg = " | ".join(out)
    assert "CLI not installed" in msg
    assert "v2.6 monitoring substrate inactive" in msg


def test_config_already_matches_baseline_skips_write_and_reload(monkeypatch, tmp_path):
    _patch_which(monkeypatch, {"netdata": "/usr/sbin/netdata"})
    # First call to apply_netdata_monitor seeds the file +
    # captures the canonical content.
    config = tmp_path / "netdata.conf"
    written = _patch_config_path(monkeypatch, config)
    _patch_read(monkeypatch, config, None)  # initially missing
    _patch_run_root(monkeypatch, _RunRecorder())
    first = birthright.apply_netdata_monitor(_dummy_preset())
    canonical = config.read_text(encoding="utf-8")
    assert len(written) >= 1
    assert any("wrote /etc/netdata/netdata.conf" in line for line in first)

    # Second call: pre-seed the read_text to return canonical
    # content; expect no further writes + no reload.
    _patch_read(monkeypatch, config, canonical)
    recorder = _RunRecorder()
    _patch_run_root(monkeypatch, recorder)
    written2 = _patch_config_path(monkeypatch, config)
    out = birthright.apply_netdata_monitor(_dummy_preset())

    assert recorder.calls == []
    assert written2 == []
    assert any("already matches the locked baseline" in line for line in out)


def test_config_differs_triggers_write_and_reload(monkeypatch, tmp_path):
    _patch_which(monkeypatch, {"netdata": "/usr/sbin/netdata"})
    config = tmp_path / "netdata.conf"
    # Pretend a stale config is on disk.
    _patch_read(monkeypatch, config, "# stale content\n")
    written = _patch_config_path(monkeypatch, config)
    recorder = _RunRecorder()
    _patch_run_root(monkeypatch, recorder)

    out = birthright.apply_netdata_monitor(_dummy_preset())

    # Wrote the new config.
    assert len(written) == 1
    # Reload was attempted (systemctl).
    assert recorder.calls == [
        ["systemctl", "reload", "netdata.service"]
    ]
    msg = " | ".join(out)
    assert "wrote /etc/netdata/netdata.conf" in msg
    assert "systemctl reload ok" in msg


def test_reload_failure_falls_back_to_restart(monkeypatch, tmp_path):
    _patch_which(monkeypatch, {"netdata": "/usr/sbin/netdata"})
    config = tmp_path / "netdata.conf"
    _patch_read(monkeypatch, config, "# stale\n")
    _patch_config_path(monkeypatch, config)
    recorder = _RunRecorder(responses=[
        (["systemctl", "reload"], (1, "Unknown reload operation\n")),
        (["systemctl", "restart"], (0, "ok\n")),
    ])
    _patch_run_root(monkeypatch, recorder)

    out = birthright.apply_netdata_monitor(_dummy_preset())

    verbs = [c[1] for c in recorder.calls]
    assert verbs == ["reload", "restart"]
    msg = " | ".join(out)
    assert "reload unavailable; restart ok" in msg


def test_reload_and_restart_both_fail_surfaces_both_errors(monkeypatch, tmp_path):
    _patch_which(monkeypatch, {"netdata": "/usr/sbin/netdata"})
    config = tmp_path / "netdata.conf"
    _patch_read(monkeypatch, config, "# stale\n")
    _patch_config_path(monkeypatch, config)
    recorder = _RunRecorder(responses=[
        (["systemctl", "reload"], (1, "reload failed: not supported\n")),
        (["systemctl", "restart"], (1, "restart failed: missing dep\n")),
    ])
    _patch_run_root(monkeypatch, recorder)

    out = birthright.apply_netdata_monitor(_dummy_preset())

    msg = " | ".join(out)
    assert "reload + restart both failed" in msg
    assert "reload: reload failed: not supported" in msg
    assert "restart: restart failed: missing dep" in msg


def test_config_contains_locked_design_lock_params(monkeypatch, tmp_path):
    """Spot-check that the generated config includes the
    operator-locked 2026-05-24 design parameters."""
    _patch_which(monkeypatch, {"netdata": "/usr/sbin/netdata"})
    config = tmp_path / "netdata.conf"
    _patch_read(monkeypatch, config, None)
    _patch_config_path(monkeypatch, config)
    _patch_run_root(monkeypatch, _RunRecorder())

    birthright.apply_netdata_monitor(_dummy_preset())

    body = config.read_text(encoding="utf-8")
    # dbengine mode (lock: ~7d retention)
    assert "memory mode = dbengine" in body
    # cloud explicitly disabled (lock: mesh is the only path)
    assert "enabled = no" in body
    # 7-day retention (604800 = 7*24*3600)
    assert "history = 604800" in body
    # bind to localhost only by default (MON-1.b adds overlay)
    assert "bind socket to IP = 127.0.0.1" in body
    # The locked design rationale is in the comment header
    assert "MON-1" in body
    assert "Locked 2026-05-24" in body
