"""Remote desktop — Headscale ↔ Guacamole sync, plus override management.

Q3/Q4 v1.2.0 design locks:
  - No auth on the mesh (Guacamole's `noauth-extension` is configured to
    serve the connection picker without a login).
  - Hybrid connection list: auto-populated from the Headscale peer roster,
    then layered with user overrides (favorite / hide / rename) read from
    ~/.config/mackes-shell/remote-overrides.json.

Public API:

  rebuild_connections()          → writes /etc/guacamole/noauth-config.xml
  load_overrides() / save_overrides()
  active_connections()           → list[ResolvedConnection] (auto + overrides)
  sync_daemon_main()             → polling loop entry point for the systemd
                                   service mackes-remote-sync.service.
"""
from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import List

from mackes.logging import log_action
from mackes.state import CONFIG_DIR


GUACAMOLE_ETC = Path("/etc/guacamole")
NOAUTH_CONFIG_PATH = GUACAMOLE_ETC / "noauth-config.xml"
OVERRIDES_FILE = CONFIG_DIR / "remote-overrides.json"


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------


@dataclass
class ResolvedConnection:
    """A connection ready to be written into Guacamole's config."""
    id: str            # stable id, e.g. "anvil-rdp"
    name: str          # display name in the picker
    protocol: str      # "rdp" | "vnc"
    hostname: str      # peer mesh IP or DNS name
    port: int
    online: bool       # informational; Guacamole still tries
    is_favorite: bool = False
    hidden: bool = False


@dataclass
class Overrides:
    """User-managed prefs that layer on top of auto-discovered connections."""
    favorites: List[str] = field(default_factory=list)
    hidden:    List[str] = field(default_factory=list)
    renames:   dict      = field(default_factory=dict)   # id -> custom name

    @classmethod
    def empty(cls) -> "Overrides":
        return cls()


# ---------------------------------------------------------------------------
# Overrides persistence
# ---------------------------------------------------------------------------


def load_overrides() -> Overrides:
    if not OVERRIDES_FILE.exists():
        return Overrides.empty()
    try:
        data = json.loads(OVERRIDES_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return Overrides.empty()
    return Overrides(
        favorites=list(data.get("favorites") or []),
        hidden=list(data.get("hidden") or []),
        renames=dict(data.get("renames") or {}),
    )


def save_overrides(ov: Overrides) -> None:
    OVERRIDES_FILE.parent.mkdir(parents=True, exist_ok=True)
    OVERRIDES_FILE.write_text(
        json.dumps({
            "favorites": sorted(set(ov.favorites)),
            "hidden":    sorted(set(ov.hidden)),
            "renames":   dict(sorted(ov.renames.items())),
        }, indent=2),
        encoding="utf-8",
    )


# ---------------------------------------------------------------------------
# Connection discovery
# ---------------------------------------------------------------------------


def _discover_peers() -> List[tuple[str, str, bool]]:
    """Return [(name, mesh_ip, online)] from Headscale, falling back to QNM-Mesh dirs."""
    peers: List[tuple[str, str, bool]] = []
    try:
        from mackes.mesh_vpn import headscale_list_peers
        for p in headscale_list_peers():
            peers.append((p.name, p.mesh_ip or "", bool(p.online)))
    except Exception:  # noqa: BLE001
        pass
    if not peers:
        # Fallback: scan ~/QNM-Mesh/ for peer subdirs
        home = Path(os.path.expanduser("~"))
        mesh_root = home / "QNM-Mesh"
        if mesh_root.exists():
            for d in mesh_root.iterdir():
                if d.is_dir():
                    peers.append((d.name, "", False))
    return peers


def active_connections() -> List[ResolvedConnection]:
    """Discovered peers × {RDP, VNC} → list, then apply overrides."""
    ov = load_overrides()
    out: List[ResolvedConnection] = []
    for name, ip, online in _discover_peers():
        host = ip or f"{name}.mesh"
        for protocol, port, suffix in (
            ("rdp", 3389, "Session"),
            ("vnc", 5900, "Mirror"),
        ):
            conn_id = f"{name}-{protocol}"
            renamed = ov.renames.get(conn_id) or f"{name} — {suffix}"
            out.append(ResolvedConnection(
                id=conn_id,
                name=renamed,
                protocol=protocol,
                hostname=host,
                port=port,
                online=online,
                is_favorite=(conn_id in set(ov.favorites)),
                hidden=(conn_id in set(ov.hidden)),
            ))
    # Favorites first, then alphabetical
    out.sort(key=lambda c: (not c.is_favorite, c.name.lower()))
    return out


# ---------------------------------------------------------------------------
# Guacamole config writers
# ---------------------------------------------------------------------------


_ESC = {"&": "&amp;", "<": "&lt;", ">": "&gt;",
        '"': "&quot;", "'": "&apos;"}


def _xml_escape(s: str) -> str:
    return "".join(_ESC.get(c, c) for c in (s or ""))


def render_noauth_xml(conns: List[ResolvedConnection]) -> str:
    """Render the noauth-extension's config XML for the given connections.

    Hidden entries are omitted entirely (so they don't appear in the picker).
    """
    lines = ['<?xml version="1.0" encoding="UTF-8"?>', "<user-mapping>",
             '  <authorize username="" password="">']
    for c in conns:
        if c.hidden:
            continue
        lines.append(
            f'    <connection name="{_xml_escape(c.name)}">'
        )
        lines.append(f"      <protocol>{c.protocol}</protocol>")
        lines.append(f'      <param name="hostname">{_xml_escape(c.hostname)}</param>')
        lines.append(f'      <param name="port">{c.port}</param>')
        if c.protocol == "rdp":
            lines.append('      <param name="security">any</param>')
            lines.append('      <param name="ignore-cert">true</param>')
            lines.append('      <param name="resize-method">display-update</param>')
            lines.append('      <param name="enable-wallpaper">true</param>')
        elif c.protocol == "vnc":
            lines.append('      <param name="color-depth">24</param>')
            lines.append('      <param name="cursor">local</param>')
        lines.append("    </connection>")
    lines.append("  </authorize>")
    lines.append("</user-mapping>")
    return "\n".join(lines) + "\n"


def rebuild_connections() -> List[str]:
    """Regenerate /etc/guacamole/noauth-config.xml from the live peer list.

    Returns a list of action strings for the wizard / sync daemon log.
    Idempotent — only writes if content changed.
    """
    actions: List[str] = []
    conns = active_connections()
    xml = render_noauth_xml(conns)
    NOAUTH_CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    if NOAUTH_CONFIG_PATH.exists():
        try:
            current = NOAUTH_CONFIG_PATH.read_text(encoding="utf-8")
        except OSError:
            current = ""
        if current == xml:
            return [f"remote-desktop: {len(conns)} connection(s) — no changes"]
    try:
        NOAUTH_CONFIG_PATH.write_text(xml, encoding="utf-8")
    except OSError as e:
        actions.append(f"remote-desktop: failed to write {NOAUTH_CONFIG_PATH}: {e}")
        return actions
    visible = sum(1 for c in conns if not c.hidden)
    actions.append(
        f"remote-desktop: wrote {visible} connection(s) "
        f"({sum(1 for c in conns if c.hidden)} hidden) to {NOAUTH_CONFIG_PATH}"
    )
    for line in actions:
        log_action(line)
    return actions


# ---------------------------------------------------------------------------
# Daemon loop (entry point for mackes-remote-sync.service)
# ---------------------------------------------------------------------------


def sync_daemon_main(*, interval_s: int = 30) -> int:
    """Poll the peer roster and regenerate the Guacamole config on change."""
    last_xml = ""
    while True:
        try:
            xml = render_noauth_xml(active_connections())
            if xml != last_xml:
                NOAUTH_CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
                NOAUTH_CONFIG_PATH.write_text(xml, encoding="utf-8")
                last_xml = xml
                log_action("remote-desktop: config refreshed by sync daemon")
        except Exception as e:  # noqa: BLE001
            log_action(f"remote-desktop sync error: {e}")
        time.sleep(interval_s)


# ---------------------------------------------------------------------------
# Service-health check (used by the panel)
# ---------------------------------------------------------------------------


def _cli_main(argv: list[str]) -> int:
    """`python -m mackes.remote_desktop --daemon` entry point."""
    if "--daemon" in argv:
        return sync_daemon_main() or 0
    if "--rebuild" in argv:
        for line in rebuild_connections():
            print(line)
        return 0
    if "--list" in argv:
        for c in active_connections():
            mark = "★" if c.is_favorite else ("○" if c.hidden else " ")
            print(f"  {mark} {c.id:24}  {c.protocol:3}  {c.hostname}:{c.port}")
        return 0
    print(__doc__)
    print("\nUsage:")
    print("  python -m mackes.remote_desktop --daemon    # systemd-managed sync loop")
    print("  python -m mackes.remote_desktop --rebuild   # one-shot config regen")
    print("  python -m mackes.remote_desktop --list      # show resolved connections")
    return 0


if __name__ == "__main__":
    import sys
    raise SystemExit(_cli_main(sys.argv[1:]))


def service_status() -> dict[str, str]:
    """Return {service: ok|warn|fail|missing} for the 4 remote-desktop daemons."""
    import shutil
    import subprocess
    units = ("xrdp.service", "x11vnc@:0.service", "guacd.service", "tomcat.service")
    out: dict[str, str] = {}
    if shutil.which("systemctl") is None:
        return {u: "missing" for u in units}
    for u in units:
        try:
            r = subprocess.run(["systemctl", "is-active", u],
                               capture_output=True, text=True, timeout=4)
            state = (r.stdout or "").strip()
            out[u] = {"active": "ok",
                      "activating": "warn",
                      "inactive": "fail",
                      "failed": "fail"}.get(state, "missing")
        except (OSError, subprocess.TimeoutExpired):
            out[u] = "missing"
    return out
