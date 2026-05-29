"""TUI Help screen — renders docs/help/index.md in the content pane."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Help[/b]\n"
        "[#c6c6c6]Quick reference. Full docs at /usr/share/mde/help/ "
        "or via `mackes help <topic>`.[/]\n"
    ))
    try:
        from mackes.help_utils import _discover_topics
        topics = _discover_topics()
        body.compose_add_child(Static(
            f"[b]Available topics[/]  [#8d8d8d]({len(topics)})[/]"
        ))
        lines = []
        for tid, label, _ in topics:
            lines.append(f"  [#c6c6c6]·[/]  [b]{label:<28}[/]  "
                         f"[#8d8d8d]mackes help {tid}[/]")
        body.compose_add_child(Static("\n".join(lines) + "\n"))
        body.compose_add_child(Static(
            "[b]Bindings in this TUI[/]\n"
            "  [b]q[/]      quit\n"
            "  [b]r[/]      refresh the active screen\n"
            "  [b]?[/]      open Help (this screen)\n"
            "  [b]↑/↓[/]    navigate the sidebar\n"
            "  [b]Enter[/]  activate a sidebar item\n"
        ))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]help read failed: {e}[/]"))
    return body
