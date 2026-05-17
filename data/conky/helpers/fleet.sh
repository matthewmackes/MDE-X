#!/bin/sh
# Conky helper — Fleet: most recent local pull + 24h failure count.
python3 - <<'PY' 2>/dev/null || echo "  (fleet not configured)"
import time
try:
    from mackes.fleet import list_runs, current_peer_name
    me = current_peer_name()
    runs = list_runs(peer=me, limit=1)
    if runs:
        r = runs[0]
        age = int(time.time() - r.timestamp)
        if   age < 60:    age_s = f"{age}s ago"
        elif age < 3600:  age_s = f"{age // 60}m ago"
        elif age < 86400: age_s = f"{age // 3600}h ago"
        else:             age_s = f"{age // 86400}d ago"
        status = "ok" if r.exit_code == 0 else "FAIL"
        print(f"  last pull · {age_s}")
        print(f"  {status} · changed={r.changed} ok={r.ok}")
    else:
        print("  (no runs yet)")
    # 24h failure tally across the mesh
    recent = list_runs(since=time.time() - 86400, limit=500)
    fails = sum(1 for r in recent if r.exit_code != 0)
    if fails:
        print(f"  ⚠ {fails} mesh failures 24h")
except Exception:
    print("  (fleet not configured)")
PY
