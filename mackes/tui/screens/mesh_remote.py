"""TUI Mesh Remote screen — service health for xrdp/x11vnc/guacd/tomcat."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Mesh Remote[/b]\n"
        "[#c6c6c6]xrdp + x11vnc + guacd + Guacamole at "
        "https://media.mesh/desktop/[/]\n"
    ))
    try:
        from mackes.remote_desktop import service_status, active_connections
        statuses = service_status()
        ok = sum(1 for v in statuses.values() if v == "ok")
        line = f"[{'green' if ok == len(statuses) else 'yellow'}]●[/]  " \
               f"{ok}/{len(statuses)} services live"
        body.compose_add_child(Static(line + "\n"))

        body.compose_add_child(Static("[b]Local services[/b]"))
        lines = []
        for unit, status in statuses.items():
            col = {"ok": "green", "warn": "yellow",
                   "fail": "red", "missing": "#8d8d8d"}.get(status, "white")
            lines.append(
                f"  [{col}]●[/]  {unit.replace('.service', ''):<20}  "
                f"[{col}]{status}[/]"
            )
        body.compose_add_child(Static("\n".join(lines) + "\n"))

        conns = active_connections()
        body.compose_add_child(Static(
            f"[b]Connections[/b]  [#8d8d8d]({len(conns)})[/]"
        ))
        if not conns:
            body.compose_add_child(Static("  [#8d8d8d](none discovered)[/]"))
        else:
            cl = []
            for c in conns[:24]:
                mark = "[#f1853d]★[/]" if c.is_favorite else (
                    "[#8d8d8d]○[/]" if c.hidden else " ")
                cl.append(f"  {mark}  {c.name:<34}  "
                          f"[#c6c6c6]{c.protocol.upper():<4}[/]  "
                          f"[#8d8d8d]{c.hostname}:{c.port}[/]")
            body.compose_add_child(Static("\n".join(cl)))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]Mesh Remote read failed: {e}[/]"))
    return body
