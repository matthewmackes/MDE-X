"""TUI Snapshots screen."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Snapshots[/b]\n"
        "[#c6c6c6]Restore points: xfconf + panel layout + theme + mesh state.[/]\n"
    ))
    try:
        from mackes.snapshots import list_snapshots
        snaps = list_snapshots()
        body.compose_add_child(Static(
            f"[#8d8d8d]{len(snaps)} snapshot(s) on this peer[/]\n"
        ))
        if not snaps:
            body.compose_add_child(Static(
                "[#8d8d8d](none yet — "
                "`mackes snapshot create --label <name>`)[/]"
            ))
            return body
        lines = []
        for snap in snaps[:30]:
            mf = snap.manifest()
            preset = mf.get("source_preset") or "—"
            lines.append(
                f"  [green]●[/]  [b]{snap.display_label():<32}[/]  "
                f"[#c6c6c6]{snap.created:%Y-%m-%d %H:%M}[/]  "
                f"[#8d8d8d]from preset: {preset}[/]"
            )
        body.compose_add_child(Static("\n".join(lines)))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]snapshot read failed: {e}[/]"))
    return body
