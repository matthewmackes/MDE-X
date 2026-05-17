#!/bin/sh
# Conky helper — Drift: items differing from active preset.
python3 - <<'PY' 2>/dev/null || echo "  (no preset)"
try:
    from mackes.presets import active_preset_drift
    preset, items = active_preset_drift()
    if preset is None:
        print("  (no preset)")
    elif not items:
        print("  ● 0 items differ")
    else:
        n = len(items)
        first = items[0]
        print(f"  ● {n} differ")
        print(f"  {first.section}.{first.field}")
except Exception:
    print("  (drift unavailable)")
PY
