"""TUI Fleet Run history screen."""
from __future__ import annotations

import time
from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Fleet · Run history[/b]\n"
        "[#c6c6c6]Last 30 days of ansible-pull / push runs across the mesh.[/]\n"
    ))
    try:
        from mackes.fleet import list_runs
        runs = list_runs(limit=40)
        total = len(runs)
        ok = sum(1 for r in runs if r.exit_code == 0)
        fail = sum(1 for r in runs if r.exit_code != 0)
        changed = sum(r.changed for r in runs)
        body.compose_add_child(Static(
            f"[#8d8d8d]TOTAL[/] [b]{total}[/]   "
            f"[#8d8d8d]OK[/] [green]{ok}[/]   "
            f"[#8d8d8d]FAILED[/] [red]{fail}[/]   "
            f"[#8d8d8d]CHANGES[/] [b]{changed}[/]\n"
        ))
        if not runs:
            body.compose_add_child(Static(
                "[#8d8d8d](no runs yet — the timer fires every 30 min)[/]"
            ))
            return body
        lines = []
        for r in runs:
            when = time.strftime("%m-%d %H:%M", time.localtime(r.timestamp))
            mark = "[green]ok[/]" if r.exit_code == 0 else "[red]FAIL[/]"
            lines.append(
                f"  [#c6c6c6]{when}[/]  {r.peer:<14}  "
                f"{r.playbook:<22}  {mark}  "
                f"[#8d8d8d]changed={r.changed:<3} via {r.triggered_by}[/]"
            )
        body.compose_add_child(Static("\n".join(lines)))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]run history read failed: {e}[/]"))
    return body
