"""mesh_vpn — state round-trip + URL parsing."""
from __future__ import annotations


# ---- MeshState round-trip -------------------------------------------------


def test_mesh_state_round_trip(isolated_xdg, monkeypatch):
    import importlib
    import mackes.mesh_vpn
    importlib.reload(mackes.mesh_vpn)
    from mackes.mesh_vpn import MeshState
    s = MeshState.load()
    assert s.mesh_id == ""
    assert s.is_control is False

    s.mesh_id = "ABCDEF12"
    s.is_control = True
    s.control_peer_id = "node-a"
    s.peer_count = 7
    s.save()

    s2 = MeshState.load()
    assert s2.mesh_id == "ABCDEF12"
    assert s2.is_control is True
    assert s2.control_peer_id == "node-a"
    assert s2.peer_count == 7


def test_mesh_state_unknown_fields_ignored(isolated_xdg, monkeypatch):
    """A future-shape state.json with extra keys must load — drop the
    extras, keep the known ones. Avoids losing state on downgrade."""
    import importlib
    import json
    import mackes.mesh_vpn
    importlib.reload(mackes.mesh_vpn)
    from mackes.mesh_vpn import MeshState, SEED_STATE_FILE
    SEED_STATE_FILE.parent.mkdir(parents=True, exist_ok=True)
    SEED_STATE_FILE.write_text(json.dumps({
        "mesh_id": "FUTURE",
        "peer_count": 3,
        "future_field_we_dont_know_about": [1, 2, 3],
    }))
    s = MeshState.load()
    assert s.mesh_id == "FUTURE"
    assert s.peer_count == 3


# ---- parse_join_link ------------------------------------------------------


def test_parse_join_link_extracts_query_string():
    from mackes.mesh_vpn import parse_join_link
    out = parse_join_link("mesh-join://?code=abc123&seed=node-a&tag=tag:mackes-XYZ")
    assert out == {"code": "abc123", "seed": "node-a", "tag": "tag:mackes-XYZ"}


def test_parse_join_link_rejects_wrong_scheme():
    from mackes.mesh_vpn import parse_join_link
    assert parse_join_link("https://example.com") == {}
    assert parse_join_link("") == {}


def test_parse_join_link_ignores_malformed_pairs():
    from mackes.mesh_vpn import parse_join_link
    out = parse_join_link("mesh-join://?code=abc&malformed&also=fine")
    assert out == {"code": "abc", "also": "fine"}
