"""TUI Debloat levels screen — read-only preview, no apply path."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Debloat levels[/b]\n"
        "[#c6c6c6]5 cumulative tiers of XFCE-desktop slimming. "
        "Apply via GUI (Maintain → Debloat levels) "
        "or CLI: `mackes debloat apply --level N`[/]\n"
    ))
    try:
        from mackes.debloat import LEVELS, preview
        for lvl in LEVELS:
            p = preview(lvl.n)
            body.compose_add_child(Static(
                f"[b]L{lvl.n}[/]  [b]{lvl.name}[/]   "
                f"[#8d8d8d]{len(lvl.packages)} package(s) defined  ·  "
                f"{len(p['removable'])} installed here[/]\n"
                f"  [#c6c6c6]{lvl.blurb}[/]\n"
            ))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]debloat read failed: {e}[/]"))
    return body
