"""mackes.mesh — unified health surface.

Tests for the LayerHealth dataclass, state composition helpers
(overall_state / summary), and with_retry. Probe-level tests are
intentionally minimal; the probes call into other mesh_* modules
that are covered separately.
"""
from __future__ import annotations


import pytest


# ---- LayerHealth shape ----------------------------------------------------


def test_layer_health_to_dict_round_trip():
    from mackes.mesh import LayerHealth
    h = LayerHealth(
        layer="vpn", state="warn",
        label="Online · 1/3 peer(s) up",
        detail="mesh_ip=100.64.0.1\npeers=1 online / 3 known",
        latency_ms=42.5,
        hint="Try `tailscale up` on the offline peers",
    )
    d = h.to_dict()
    assert d == {
        "layer": "vpn",
        "state": "warn",
        "label": "Online · 1/3 peer(s) up",
        "detail": "mesh_ip=100.64.0.1\npeers=1 online / 3 known",
        "latency_ms": 42.5,
        "hint": "Try `tailscale up` on the offline peers",
    }


# ---- overall_state ranks correctly ---------------------------------------


def test_overall_state_picks_worst():
    from mackes.mesh import LayerHealth, overall_state
    snap = {
        "a": LayerHealth("a", "ok",   "fine"),
        "b": LayerHealth("b", "warn", "iffy"),
        "c": LayerHealth("c", "ok",   "fine"),
    }
    assert overall_state(snap) == "warn"
    snap["d"] = LayerHealth("d", "fail", "broken")
    assert overall_state(snap) == "fail"


def test_overall_state_treats_missing_as_warn_not_fail():
    """A layer that's 'missing by design' (e.g. fs with no peer mounts)
    shouldn't make the whole mesh row red."""
    from mackes.mesh import LayerHealth, overall_state
    snap = {
        "a": LayerHealth("a", "ok",      "fine"),
        "b": LayerHealth("b", "missing", "no peers yet"),
    }
    assert overall_state(snap) == "warn"


def test_overall_state_all_ok():
    from mackes.mesh import LayerHealth, overall_state
    snap = {f"l{i}": LayerHealth(f"l{i}", "ok", "fine") for i in range(8)}
    assert overall_state(snap) == "ok"


# ---- summary formats counts ----------------------------------------------


def test_summary_counts_states():
    from mackes.mesh import LayerHealth, summary
    snap = {
        "a": LayerHealth("a", "ok",      "fine"),
        "b": LayerHealth("b", "ok",      "fine"),
        "c": LayerHealth("c", "warn",    "iffy"),
        "d": LayerHealth("d", "fail",    "broken"),
        "e": LayerHealth("e", "missing", "off"),
    }
    s = summary(snap)
    assert "2/5 ok" in s
    assert "1 warn"  in s
    assert "1 fail"  in s
    assert "1 off"   in s


# ---- with_retry behaviour ------------------------------------------------


def test_with_retry_returns_on_first_success():
    from mackes.mesh import with_retry
    calls = [0]
    def fn():
        calls[0] += 1
        return 42
    assert with_retry(fn, attempts=3) == 42
    assert calls[0] == 1


def test_with_retry_retries_on_oserror():
    from mackes.mesh import with_retry
    calls = [0]
    def fn():
        calls[0] += 1
        if calls[0] < 3:
            raise OSError("transient")
        return "ok"
    assert with_retry(fn, attempts=3, backoff=1.0) == "ok"
    assert calls[0] == 3


def test_with_retry_raises_after_exhaustion():
    from mackes.mesh import with_retry
    calls = [0]
    def fn():
        calls[0] += 1
        raise OSError("always fails")
    with pytest.raises(OSError):
        with_retry(fn, attempts=2, backoff=1.0)
    assert calls[0] == 2


def test_with_retry_does_not_swallow_unlisted_exception():
    """A ValueError isn't in retry_on; should propagate immediately."""
    from mackes.mesh import with_retry
    calls = [0]
    def fn():
        calls[0] += 1
        raise ValueError("nope")
    with pytest.raises(ValueError):
        with_retry(fn, attempts=3)
    assert calls[0] == 1


# ---- diagnose runs without raising even on a fresh machine ----------------


def test_diagnose_returns_lines_and_does_not_raise():
    """On a machine with no mesh state, diagnose() should still run
    every probe and return a multi-line report rather than blowing up.
    """
    from mackes.mesh import diagnose
    lines = diagnose()
    assert isinstance(lines, list)
    assert any("mesh state:" in ln.lower() for ln in lines)
    # Every active layer name must appear at least once. The
    # umbrella was pruned to 4 layers by DEAD-2.15 on 2026-05-26
    # (thumbnailer/services/sync/browser retired with their
    # backing modules under DEAD-2.2/2.9/2.10/2.11); fs +
    # notifications retire under DEAD-2.12 (HW-gated v5.2) +
    # DEAD-2.8 (BUS-4.2 hard cut). Source-of-truth list lives at
    # `mackes.mesh:_LAYERS`.
    text = "\n".join(lines)
    for layer in ("vpn", "ssh", "fs", "notifications"):
        assert layer in text, f"layer {layer!r} missing from diagnose output"
