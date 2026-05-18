"""TUI Dashboard screen."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    from mackes.state import (
        MackesState, hardware_summary, service_health, last_snapshot,
    )

    try:
        state = MackesState.load()
        preset = state.active_preset or "—"
    except Exception:  # noqa: BLE001
        preset = "—"

    sh = service_health()
    ok_n = sum(1 for v in sh.values() if v == "ok")
    total = len(sh)

    try:
        from mackes.mesh_vpn import tailscale_status
        mesh_n = len(tailscale_status().get("peers", []) or [])
    except Exception:  # noqa: BLE001
        mesh_n = 0

    try:
        from mackes.presets import active_preset_drift
        _preset, items = active_preset_drift()
        drift_n = len(items or [])
    except Exception:  # noqa: BLE001
        drift_n = 0

    info = hardware_summary()
    snap = last_snapshot()

    body = Container()
    body.compose_add_child(Static(
        f"[b]Dashboard[/b]\n[#c6c6c6]Preset · {preset.title()}[/#c6c6c6]\n"
    ))

    # Stat row
    body.compose_add_child(Static(
        f"[#8d8d8d]MESH PEERS[/]   [#8d8d8d]SERVICES[/]   [#8d8d8d]sshd[/]"
        f"        [#8d8d8d]DRIFT[/]\n"
        f"[b]{mesh_n}[/b]            [b]{ok_n} / {total}[/b]    "
        f"[b {('green' if sh.get('sshd') == 'ok' else 'red')}]"
        f"{'running' if sh.get('sshd') == 'ok' else 'down'}[/]    "
        f"[b]{drift_n}[/b]\n"
    ))

    # Service grid
    body.compose_add_child(Static("[b]Service health[/b]\n"))
    lines: list[str] = []
    for name, status in sh.items():
        col = {"ok": "green", "warn": "yellow",
               "fail": "red", "missing": "#8d8d8d"}.get(status, "white")
        lines.append(f"  [{col}]●[/]  {name:<22}  [{col}]{status}[/]")
    body.compose_add_child(Static("\n".join(lines) + "\n"))

    # Hardware
    body.compose_add_child(Static("[b]This machine[/b]\n"))
    hw_lines = []
    for k, v in (("Hostname", info.get("hostname")),
                 ("OS",       info.get("os")),
                 ("CPU",      info.get("cpu")),
                 ("RAM",      info.get("ram"))):
        hw_lines.append(f"  [#8d8d8d]{k:<10}[/]  {v or '—'}")
    body.compose_add_child(Static("\n".join(hw_lines) + "\n"))

    # Last snapshot
    if snap is not None:
        name, when = snap
        body.compose_add_child(Static(
            f"[b]Last snapshot[/b]\n"
            f"  {when:%Y-%m-%d %H:%M}  ·  {name}\n"
        ))
    else:
        body.compose_add_child(Static("[b]Last snapshot[/b]\n  (none yet)\n"))
    return body
