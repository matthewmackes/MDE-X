"""v12.16 — self-hosted DERP relay unit smoke tests.

Asserts the systemd unit file ships every locked field (Q4 single-
region, ConditionPathExists gate on the Host-role marker, certmode +
STUN flags) and the spec installs both the unit and the example
DERP-map under %{_datadir}/mde/headscale/.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest


REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))


def test_derper_unit_exists():
    unit = REPO / "data/systemd/mde-derper.service"
    assert unit.is_file(), "v12.16 unit must ship"


def test_derper_unit_is_host_role_gated():
    """ConditionPathExists must match the Host-role marker file."""
    src = (REPO / "data/systemd/mde-derper.service").read_text()
    assert "ConditionPathExists=/var/lib/mde/derper.enabled" in src, (
        "unit must gate on the Host-role marker file"
    )


def test_derper_unit_runs_derper_with_locked_flags():
    src = (REPO / "data/systemd/mde-derper.service").read_text()
    assert "/usr/bin/derper" in src
    assert "--hostname=" in src
    assert "--certmode=" in src
    # Q12.17 — STUN flag stays on by default so symmetric-NAT edges
    # can use the same endpoint.
    assert "--stun=" in src


def test_derper_unit_has_capability_lockdown():
    """Service must give up every cap except NET_BIND_SERVICE."""
    src = (REPO / "data/systemd/mde-derper.service").read_text()
    assert "CapabilityBoundingSet=CAP_NET_BIND_SERVICE" in src
    assert "AmbientCapabilities=CAP_NET_BIND_SERVICE" in src
    assert "NoNewPrivileges=true" in src
    assert "ProtectSystem=strict" in src
    assert "ProtectHome=true" in src


def test_derper_unit_has_resource_caps():
    src = (REPO / "data/systemd/mde-derper.service").read_text()
    assert "MemoryHigh=" in src
    assert "MemoryMax=" in src
    assert "CPUQuota=" in src


def test_derp_map_example_exists():
    p = REPO / "data/headscale/derp-map.example.json"
    assert p.is_file(), "v12.16 DERP-map example must ship"


def test_derp_map_example_is_valid_json():
    import json
    p = REPO / "data/headscale/derp-map.example.json"
    data = json.loads(p.read_text())
    assert "Regions" in data
    assert "900" in data["Regions"], "mde-self region id 900 must be present"
    region = data["Regions"]["900"]
    assert region["RegionCode"] == "mde-self"
    assert len(region["Nodes"]) == 1
    node = region["Nodes"][0]
    assert node["DERPPort"] == 443
    assert node["STUNPort"] == 3478


def test_spec_installs_derper_unit():
    spec = (REPO / "packaging/fedora/mackes-shell.spec").read_text()
    assert "data/systemd/mde-derper.service" in spec
    assert "%{_unitdir}/mde-derper.service" in spec


def test_spec_installs_derp_map_example():
    spec = (REPO / "packaging/fedora/mackes-shell.spec").read_text()
    assert "data/headscale/derp-map.example.json" in spec
    assert "%{_datadir}/mde/headscale/derp-map.example.json" in spec


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
