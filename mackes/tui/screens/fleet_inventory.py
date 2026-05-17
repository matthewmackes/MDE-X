"""TUI Fleet Inventory screen."""
from __future__ import annotations

import time
from textual.containers import Container
from textual.widgets import Static


def _age(ts):
    if ts is None:
        return "never"
    d = int(time.time() - ts)
    if d < 60: return f"{d}s ago"
    if d < 3600: return f"{d // 60}m ago"
    if d < 86400: return f"{d // 3600}h ago"
    return f"{d // 86400}d ago"


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Fleet · Inventory[/b]\n"
        "[#c6c6c6]Every mesh peer + its ansible-pull state.[/]\n"
    ))
    try:
        from mackes.fleet import build_inventory, current_peer_name
        peers = build_inventory()
        me = current_peer_name()
        online = sum(1 for p in peers if p.online)
        ok = sum(1 for p in peers if p.last_pull_ok is True)
        body.compose_add_child(Static(
            f"[green]●[/]  {online}/{len(peers)} peers online  ·  "
            f"{ok} successful pulls in window\n"
        ))
        body.compose_add_child(Static("[b]Peers[/b]"))
        lines = []
        for p in peers:
            dot = "[green]●[/]" if p.online else "[#8d8d8d]○[/]"
            tag = ("[green]ok[/]" if p.last_pull_ok else
                   "[red]failed[/]" if p.last_pull_ok is False else
                   "[#8d8d8d]never[/]")
            self_tag = "  [#f1853d](this peer)[/]" if p.name == me else ""
            lines.append(
                f"  {dot}  {p.name:<18}  "
                f"[#c6c6c6]{p.mesh_ip or '—':<14}[/]  "
                f"[#8d8d8d]last pull {_age(p.last_pull_at):<10}[/]  "
                f"{tag}{self_tag}"
            )
        body.compose_add_child(Static("\n".join(lines) + "\n"))
        body.compose_add_child(Static(
            "[#8d8d8d]Use `mackes fleet --push <peer1,peer2> --tags <tag>` "
            "for ad-hoc runs.[/]"
        ))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]inventory read failed: {e}[/]"))
    return body
