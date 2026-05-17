#!/bin/sh
# Conky helper — Mesh: peers online / total + control node.
python3 - <<'PY' 2>/dev/null || echo "  offline"
try:
    from mackes.mesh_vpn import headscale_list_peers, MeshState, MESH_CAP
    peers = headscale_list_peers()
    online = sum(1 for p in peers if p.online)
    total = len(peers) or MESH_CAP
    state = MeshState.load()
    control = state.control_peer_id or "—"
    print(f"  ● {online} / {total} online")
    print(f"  control · {control}")
except Exception:
    print("  (not configured)")
PY
