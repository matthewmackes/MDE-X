"""Audio device helpers (pactl wrappers).

Extracted from `mackes/workbench/devices/sound.py` as part of
`EPIC-RETIRE-PY-WORKBENCH.delete-ported.batch-3` (2026-05-26).
The workbench panel that surfaced these helpers retires under the
EPIC, but the helpers themselves stay alive — they're consumed
outside the workbench tree (`mackes/wizard/pages/hardware.py`),
which is NOT being retired.

The function names keep their leading underscore for minimum-
delta with the previous home; mark as public API by reference
from external consumers + import path. A future cleanup may drop
the leading underscore + rename to `list_sinks` / `default_sink`
/ etc.
"""
from __future__ import annotations

import subprocess

from mackes.logging import log_action


def _pactl(*args: str) -> str:
    try:
        return subprocess.check_output(
            ["pactl", *args], text=True, stderr=subprocess.DEVNULL
        ).strip()
    except (FileNotFoundError, subprocess.CalledProcessError):
        return ""


def _list_sinks() -> list[tuple[str, str]]:
    # pactl sink list is stable until hotplug; cache 20s. Same for
    # sources and the default-sink/source queries.
    from mackes.probe_cache import cached

    def _probe() -> list[tuple[str, str]]:
        raw = _pactl("list", "short", "sinks")
        out: list[tuple[str, str]] = []
        for line in raw.splitlines():
            parts = line.split("\t")
            if len(parts) >= 2:
                out.append((parts[1], parts[1]))
        return out

    return cached("sound.sinks", factory=_probe, ttl_s=20)


def _list_sources() -> list[tuple[str, str]]:
    from mackes.probe_cache import cached

    def _probe() -> list[tuple[str, str]]:
        raw = _pactl("list", "short", "sources")
        out: list[tuple[str, str]] = []
        for line in raw.splitlines():
            parts = line.split("\t")
            if len(parts) >= 2 and not parts[1].endswith(".monitor"):
                out.append((parts[1], parts[1]))
        return out

    return cached("sound.sources", factory=_probe, ttl_s=20)


def _default_sink() -> str:
    from mackes.probe_cache import cached
    return cached(
        "sound.default_sink",
        factory=lambda: _pactl("get-default-sink"),
        ttl_s=10,
    )


def _default_source() -> str:
    from mackes.probe_cache import cached
    return cached(
        "sound.default_source",
        factory=lambda: _pactl("get-default-source"),
        ttl_s=10,
    )


def _set_default_sink(name: str) -> None:
    _pactl("set-default-sink", name)
    log_action(f"sound: default sink -> {name}")


def _set_default_source(name: str) -> None:
    _pactl("set-default-source", name)
    log_action(f"sound: default source -> {name}")
