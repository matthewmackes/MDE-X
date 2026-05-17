#!/bin/sh
# Conky helper — Mesh media services (Jellyfin, Airsonic, Plex, etc.).
python3 - <<'PY' 2>/dev/null || echo "  (services unavailable)"
try:
    from mackes.mesh_services import load_registry, load_catalog
    hits = load_registry()
    if not hits:
        print("  (none discovered)")
    else:
        peers = {h.peer for h in hits}
        print(f"  {len(hits)} services · {len(peers)} peers")
        # Show up to 3 short service lines
        catalog = {d.name: d for d in load_catalog()}
        for h in hits[:3]:
            disp = catalog.get(h.service).display \
                if h.service in catalog and catalog.get(h.service) else h.service
            disp = (disp[:18] + '…') if len(disp) > 19 else disp
            print(f"  · {disp} @ {h.peer}")
        if len(hits) > 3:
            print(f"  + {len(hits) - 3} more")
except Exception:
    print("  (services unavailable)")
PY
