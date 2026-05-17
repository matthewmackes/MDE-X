#!/bin/sh
# Conky helper — emit the active preset name from state.json.
# Replaces the inline 'python3 -c "import sys,json…"' in the template.
STATE="${XDG_CONFIG_HOME:-$HOME/.config}/mackes-shell/state.json"
if [ ! -f "$STATE" ]; then
    printf '%s' "—"
    exit 0
fi
# Strict-ish JSON parse via python3 with stdlib only.
python3 - "$STATE" <<'PY' 2>/dev/null || printf '%s' "—"
import json, sys
try:
    with open(sys.argv[1], "r", encoding="utf-8") as fh:
        d = json.load(fh)
    print(d.get("active_preset") or "—", end="")
except Exception:
    print("—", end="")
PY
