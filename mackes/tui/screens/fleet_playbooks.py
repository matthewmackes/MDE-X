"""TUI Fleet Playbooks screen."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Fleet · Playbooks[/b]\n"
        "[#c6c6c6]Curated Ansible roles under "
        "QNM-Shared/.qnm-sync/playbooks/[/]\n"
    ))
    try:
        from mackes.fleet import list_playbooks, list_runs, current_peer_name
        pbs = list_playbooks()
        if not pbs:
            body.compose_add_child(Static(
                "[#8d8d8d](no playbooks — re-run the wizard's "
                "Fleet management step)[/]"
            ))
            return body

        recent_by_pb: dict[str, list] = {}
        me = current_peer_name()
        for r in list_runs(peer=me, limit=100):
            recent_by_pb.setdefault(r.playbook, []).append(r)

        for pb in pbs:
            tag_str = " ".join(f"[#8d8d8d on #262626] {t} [/]" for t in pb.tags)
            recent = recent_by_pb.get(pb.name, [])
            if recent:
                last = recent[0]
                run_str = (f"  changed={last.changed} ok={last.ok} "
                           f"failed={last.failed} rc={last.exit_code}")
            else:
                run_str = "  (never run on this peer)"
            body.compose_add_child(Static(
                f"[b]{pb.name}[/]  {tag_str}\n"
                f"  [#c6c6c6]{pb.description}[/]\n"
                f"[#8d8d8d]{run_str}[/]\n"
            ))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]playbook read failed: {e}[/]"))
    return body
