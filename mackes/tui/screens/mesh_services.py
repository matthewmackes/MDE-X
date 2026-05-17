"""TUI Mesh Services screen — discovered HTTP services on the mesh."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Mesh Services[/b]\n"
        "[#c6c6c6]Discovered HTTP services across every peer.[/]\n"
    ))
    try:
        from mackes.mesh_services import load_registry, load_catalog, url_for
        hits = load_registry()
        catalog = {d.name: d for d in load_catalog()}
        peers = sorted({h.peer for h in hits})
        body.compose_add_child(Static(
            f"[#8d8d8d]{len(hits)} services on {len(peers)} peers[/]\n"
        ))

        if not hits:
            body.compose_add_child(Static(
                "[#8d8d8d](no services discovered — run "
                '`mackes mesh-services scan` to probe peers)[/]'
            ))
            return body

        lines = []
        for hit in hits:
            disp = (catalog.get(hit.service).display
                    if hit.service in catalog and catalog.get(hit.service)
                    else hit.service)
            lines.append(
                f"  [green]●[/]  [b]{disp:<24}[/]  "
                f"[#8d8d8d]on {hit.peer}.mesh[/]\n"
                f"        [#f1853d]{url_for(hit)}[/]"
            )
        body.compose_add_child(Static("\n".join(lines)))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]Mesh Services read failed: {e}[/]"))
    return body
