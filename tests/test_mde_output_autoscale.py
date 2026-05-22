"""Tests for v2.0.3 mde-output-autoscale.

The helper file lives at bin/mde-output-autoscale and has no .py
extension (it's an executable shipped to /usr/bin), so we load it
via SourceFileLoader the same way test_mde_migrate_from_1x.py does.
"""
from __future__ import annotations

import importlib.machinery
import importlib.util
from pathlib import Path


def _load_module():
    repo = Path(__file__).resolve().parent.parent
    path = repo / "bin" / "mde-output-autoscale"
    loader = importlib.machinery.SourceFileLoader(
        "mde_output_autoscale", str(path),
    )
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


# ----------------------------------------------------------------------
# pick_scale — heuristic mapping width → scale factor
# ----------------------------------------------------------------------

def test_pick_scale_4k_and_wider_uses_2x():
    mod = _load_module()
    assert mod.pick_scale(3840) == 2.0
    assert mod.pick_scale(4096) == 2.0
    assert mod.pick_scale(5120) == 2.0


def test_pick_scale_2k_uses_1_5x():
    mod = _load_module()
    assert mod.pick_scale(2560) == 1.5
    assert mod.pick_scale(2880) == 1.5


def test_pick_scale_1080p_and_smaller_uses_1x():
    mod = _load_module()
    # 1080p
    assert mod.pick_scale(1920) == 1.0
    # 720p
    assert mod.pick_scale(1366) == 1.0
    # tiny
    assert mod.pick_scale(800) == 1.0


def test_pick_scale_boundary_3840_is_2x_not_1_5x():
    """Lock the boundary — exactly 3840 wide (the 4K-UHD width) hits
    the 2x branch, not the 2K branch."""
    mod = _load_module()
    assert mod.pick_scale(3840) == 2.0
    assert mod.pick_scale(3839) == 1.5  # one pixel narrower → 2K bucket


# ----------------------------------------------------------------------
# plan_scales — walks swaymsg JSON, applies override-respect rule
# ----------------------------------------------------------------------

def _output(name: str, width: int, *,
            scale: float = 1.0, active: bool = True) -> dict:
    """Build a minimal swaymsg output dict for the planner."""
    return {
        "name": name,
        "active": active,
        "current_mode": {"width": width, "height": int(width * 9 / 16)},
        "scale": scale,
    }


def test_plan_scales_emits_for_4k_output_at_default_scale():
    mod = _load_module()
    plan = mod.plan_scales([_output("DP-2", 3840)])
    assert plan == [("DP-2", 2.0)]


def test_plan_scales_skips_when_scale_already_set():
    """Operator override (scale != 1.0) is treated as intentional and
    left alone — the helper never fights a manual tweak."""
    mod = _load_module()
    plan = mod.plan_scales([_output("DP-2", 3840, scale=1.5)])
    assert plan == []


def test_plan_scales_skips_inactive_output():
    mod = _load_module()
    plan = mod.plan_scales([_output("DP-2", 3840, active=False)])
    assert plan == []


def test_plan_scales_skips_1080p_output():
    """1080p already at scale=1.0 needs no change — keep the plan
    empty so the helper exits 0 with no swaymsg calls."""
    mod = _load_module()
    plan = mod.plan_scales([_output("eDP-1", 1920)])
    assert plan == []


def test_plan_scales_mixed_bench_rig_emits_only_for_4k():
    """The canonical bench rig: laptop 1366×768 + 4K TV 3840×2160.
    Plan should touch DP-2 only."""
    mod = _load_module()
    plan = mod.plan_scales([
        _output("eDP-1", 1366),
        _output("DP-2", 3840),
    ])
    assert plan == [("DP-2", 2.0)]


def test_plan_scales_handles_missing_fields_gracefully():
    """Malformed swaymsg output (missing width, no current_mode, etc.)
    must not crash — the helper skips and moves on."""
    mod = _load_module()
    bad = [
        {"name": "X-0", "active": True},                       # no current_mode
        {"name": "X-1", "active": True, "current_mode": {}},   # no width
        {"name": "",    "active": True, "current_mode": {"width": 3840}, "scale": 1.0},  # empty name
        {"active": True, "current_mode": {"width": 3840}, "scale": 1.0},  # no name key
    ]
    plan = mod.plan_scales(bad)
    assert plan == []


def test_plan_scales_handles_2560_correctly():
    mod = _load_module()
    plan = mod.plan_scales([_output("HDMI-A-1", 2560)])
    assert plan == [("HDMI-A-1", 1.5)]
