#!/bin/sh
# Conky helper — single-ping latency (ms) to the mesh control peer.
# Output is one float on stdout — consumed by ${execgraph}.
# Prints "0" on any failure so the sparkline shows a flat baseline
# instead of breaking.

CONTROL_IP=""
# Resolve control peer IP from MeshState (best effort).
CONTROL_IP=$(python3 - <<'PY' 2>/dev/null
try:
    from mackes.mesh_vpn import MeshState, headscale_list_peers
    state = MeshState.load()
    pid = state.control_peer_id
    if not pid:
        print("")
    else:
        for p in headscale_list_peers():
            if p.peer_id == pid:
                print(getattr(p, "ip", "") or "")
                break
        else:
            print("")
except Exception:
    print("")
PY
)

if [ -z "$CONTROL_IP" ]; then
    echo 0
    exit 0
fi

# -c1 single packet, -W1 1-second deadline. Extract time= field.
MS=$(ping -c1 -W1 -n -q "$CONTROL_IP" 2>/dev/null \
     | awk -F'/' '/rtt|round-trip/ {print $5}')
if [ -z "$MS" ]; then
    echo 0
else
    echo "$MS"
fi
