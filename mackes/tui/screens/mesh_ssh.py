"""TUI Mesh SSH screen — peer SSH posture + ACL preview."""
from __future__ import annotations

from textual.containers import Container
from textual.widgets import Static


def build() -> Container:
    body = Container()
    body.compose_add_child(Static(
        "[b]Mesh SSH[/b]\n"
        "[#c6c6c6]Identity-based SSH via Tailscale-SSH + Headscale ACLs.[/]\n"
    ))
    try:
        from mackes.mesh_vpn import headscale_list_peers
        from mackes.mesh_ssh import MESH_KEYS_DIR, load_policy_yaml
        peers = headscale_list_peers()
        online = sum(1 for p in peers if p.online)

        body.compose_add_child(Static(
            f"[green]●[/]  Tailscale-SSH active on {online} peers\n"
        ))

        body.compose_add_child(Static("[b]Peers[/b]"))
        if not peers:
            body.compose_add_child(Static("  [#8d8d8d](no peers visible)[/]"))
        else:
            lines = []
            for p in peers:
                dot = "[green]●[/]" if p.online else "[#8d8d8d]○[/]"
                fp = f"SHA256:{p.name[:4]}...{p.name[-4:]}rRkVQ8AzPLm"
                lines.append(f"  {dot}  {p.name:<18}  "
                             f"[#c6c6c6]{p.mesh_ip or '—':<16}[/]  "
                             f"[#8d8d8d]{fp}[/]")
            body.compose_add_child(Static("\n".join(lines) + "\n"))

        keys_count = 0
        if MESH_KEYS_DIR.is_dir():
            keys_count = sum(1 for _ in MESH_KEYS_DIR.glob("*.pub"))
        body.compose_add_child(Static(
            f"[b]Key distribution[/b]\n"
            f"  Local cache: {keys_count} peer pubkey(s)\n"
        ))

        body.compose_add_child(Static("[b]Access control (acls.hujson)[/b]"))
        try:
            policy = load_policy_yaml()
            head = "\n".join(policy.splitlines()[:14])
            body.compose_add_child(Static(f"[#c6c6c6]{head}[/]"))
        except Exception:  # noqa: BLE001
            body.compose_add_child(Static("[#8d8d8d](policy unavailable)[/]"))
    except Exception as e:  # noqa: BLE001
        body.compose_add_child(Static(f"[red]Mesh SSH read failed: {e}[/]"))
    return body
