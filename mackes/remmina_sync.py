"""Auto-populate Remmina with every detected SSH/RDP/VNC service on the
mesh. Design locked via 5-question survey on 2026-05-17:

  Q1 Trigger    button + peer-event hook + 5-min systemd timer
  Q2 Discovery  live TCP probe of :22, :3389, :5900 — cached 5 min
  Q3 Auth       mesh SSH key for SSH; blank password fields for RDP/VNC
                (user fills in once, Remmina keyring stores)
  Q4 Cleanup    every Mackes-managed entry has group="Mesh Peers";
                stale entries inside that group are deleted, entries
                outside are never touched
  Q5 UI         headless by default; toggle + "Sync now" button live
                in System → Tweaks (mackes.workbench.system.tweaks_full)

Public API:

  probe_peer(host)               → {"ssh": bool, "rdp": bool, "vnc": bool}
  current_peers()                → list[Peer]    (from tailscale_status)
  sync()                         → SyncReport
  is_enabled() / enable() / disable()  (tweaks.json toggle)
  install_units() / uninstall_units()  (systemd --user)
"""
from __future__ import annotations

import configparser
import json
import os
import re
import shutil
import socket
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Optional


# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------


REMMINA_DIR = Path.home() / ".local/share/remmina"
MACKES_GROUP = "Mesh Peers"   # the Remmina group folder we own end-to-end
MACKES_TAG = "X-Mackes-Managed"  # marker key inside the .remmina file
PROTOCOLS = (
    # (protocol, port, suffix for filename, Remmina protocol string)
    ("ssh", 22,   "SSH"),
    ("rdp", 3389, "RDP"),
    ("vnc", 5900, "VNC"),
)
TWEAKS_KEY = "remmina_sync_enabled"
SYSTEMD_UNIT = "mackes-remmina-sync.timer"


# ---------------------------------------------------------------------------
# Data shapes
# ---------------------------------------------------------------------------


@dataclass
class PeerProbe:
    name: str
    host: str
    ssh: bool = False
    rdp: bool = False
    vnc: bool = False


@dataclass
class SyncReport:
    added:   list[str] = field(default_factory=list)
    updated: list[str] = field(default_factory=list)
    deleted: list[str] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)
    peers_probed: int = 0

    def __str__(self) -> str:
        parts = []
        if self.added:   parts.append(f"+{len(self.added)} new")
        if self.updated: parts.append(f"~{len(self.updated)} updated")
        if self.deleted: parts.append(f"-{len(self.deleted)} removed")
        if not parts:    parts.append("no changes")
        return (f"Remmina sync: {self.peers_probed} peer(s) probed · "
                + " · ".join(parts))


# ---------------------------------------------------------------------------
# Probing (uses mackes.probe_cache for 5-min TTL)
# ---------------------------------------------------------------------------


def _tcp_open(host: str, port: int, *, timeout: float = 1.0) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except (OSError, TimeoutError):
        return False


def probe_peer(host: str) -> dict[str, bool]:
    """Probe :22 / :3389 / :5900 on `host`. Cached 5 min.

    Returns {"ssh": bool, "rdp": bool, "vnc": bool}.
    """
    from mackes.probe_cache import cached

    def _probe() -> dict[str, bool]:
        out: dict[str, bool] = {}
        for proto, port, _ in PROTOCOLS:
            out[proto] = _tcp_open(host, port, timeout=1.0)
        return out
    return cached(f"remmina.probe:{host}",
                  factory=_probe, ttl_s=300.0)


def current_peers() -> list[PeerProbe]:
    """Return a list of mesh peers with probe results.

    Source of truth is tailscale_status().peers — same list the mesh
    health surface uses. Each peer's TailscaleIP becomes the host. The
    local peer (self) is excluded — you don't need a Remmina entry to
    connect to yourself.
    """
    try:
        from mackes.mesh_vpn import tailscale_status
    except Exception:  # noqa: BLE001
        return []
    status = tailscale_status()
    if not status.get("online"):
        return []
    my_ip = status.get("mesh_ip", "")
    out: list[PeerProbe] = []
    for p in status.get("peers", []) or []:
        host = p.get("mesh_ip") or p.get("name", "")
        if not host or host == my_ip:
            continue
        name = (p.get("name") or host).split(".", 1)[0]
        probe = probe_peer(host)
        out.append(PeerProbe(
            name=name, host=host,
            ssh=probe["ssh"], rdp=probe["rdp"], vnc=probe["vnc"],
        ))
    return out


# ---------------------------------------------------------------------------
# .remmina file I/O
# ---------------------------------------------------------------------------


def _safe_slug(text: str) -> str:
    """Lowercase, replace anything non-alnum with '-'. For filenames."""
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def _file_for(peer: PeerProbe, proto: str) -> Path:
    return REMMINA_DIR / (
        f"mackes-mesh-{_safe_slug(peer.name)}-{proto}.remmina"
    )


def _render_remmina(peer: PeerProbe, proto: str) -> str:
    """Build a minimal .remmina INI for one (peer, protocol) entry."""
    cp = configparser.ConfigParser(interpolation=None)
    cp.optionxform = lambda x: x      # preserve case
    section = {
        "group": MACKES_GROUP,
        MACKES_TAG: "1",
        "name": f"{peer.name} ({proto.upper()})",
        "server": f"{peer.host}:{_port_for(proto)}",
        "protocol": _remmina_proto(proto),
    }
    if proto == "ssh":
        section.update({
            "ssh_username": os.environ.get("USER", "mm"),
            "ssh_auth": "3",   # 3 = public key
            "ssh_privatekey": str(
                Path.home() / ".ssh/mackes_mesh_ed25519"
            ),
        })
    else:
        # RDP / VNC — blank password fields; Remmina prompts and stores
        # via its own keyring once the user supplies them.
        section.update({"username": "", "password": ""})
    cp["remmina"] = section
    from io import StringIO
    buf = StringIO()
    cp.write(buf)
    return buf.getvalue()


def _port_for(proto: str) -> int:
    return {"ssh": 22, "rdp": 3389, "vnc": 5900}[proto]


def _remmina_proto(proto: str) -> str:
    return {"ssh": "SSH", "rdp": "RDP", "vnc": "VNC"}[proto]


def _existing_managed_files() -> list[Path]:
    """List .remmina files in REMMINA_DIR whose `group` matches our
    MACKES_GROUP. Any file lacking that group is left alone — even if
    its filename happens to start with mackes-mesh-."""
    if not REMMINA_DIR.is_dir():
        return []
    out: list[Path] = []
    for p in REMMINA_DIR.glob("*.remmina"):
        try:
            cp = configparser.ConfigParser(interpolation=None)
            cp.optionxform = lambda x: x
            cp.read(p, encoding="utf-8")
            if cp.has_section("remmina"):
                if cp["remmina"].get("group", "") == MACKES_GROUP:
                    out.append(p)
        except (OSError, configparser.Error):
            continue
    return out


# ---------------------------------------------------------------------------
# sync() — the reconciler
# ---------------------------------------------------------------------------


def sync(*, peers: Optional[Iterable[PeerProbe]] = None) -> SyncReport:
    """Reconcile Remmina's `Mesh Peers` group against detected peers.

    Adds new entries for (peer, proto) pairs where probe_peer() said
    the port is open. Removes managed entries that no longer have a
    matching live target. Updates existing entries when content drifts
    (e.g. peer's mesh_ip changed).

    Idempotent: running twice in a row with the same input produces
    zero changes the second time.

    Files outside MACKES_GROUP are NEVER touched.
    """
    report = SyncReport()
    REMMINA_DIR.mkdir(parents=True, exist_ok=True)

    peers_list = list(peers) if peers is not None else current_peers()
    report.peers_probed = len(peers_list)

    # Target file → desired content
    targets: dict[Path, str] = {}
    for peer in peers_list:
        for proto in ("ssh", "rdp", "vnc"):
            if not getattr(peer, proto):
                continue
            targets[_file_for(peer, proto)] = _render_remmina(peer, proto)

    existing = {p: p.read_text(encoding="utf-8")
                for p in _existing_managed_files()
                if p.is_file()}

    # Add or update
    for path, desired in targets.items():
        if path in existing:
            if existing[path] != desired:
                path.write_text(desired, encoding="utf-8")
                report.updated.append(path.name)
            else:
                report.skipped.append(path.name)
        else:
            path.write_text(desired, encoding="utf-8")
            report.added.append(path.name)

    # Delete stale (managed files no longer in targets)
    for path in existing:
        if path not in targets:
            try:
                path.unlink()
                report.deleted.append(path.name)
            except OSError:
                pass

    return report


# ---------------------------------------------------------------------------
# Tweaks-toggle integration
# ---------------------------------------------------------------------------


def _tweaks_path() -> Path:
    from mackes.state import CONFIG_DIR
    return CONFIG_DIR / "tweaks.json"


def is_enabled() -> bool:
    p = _tweaks_path()
    if not p.exists():
        return False
    try:
        return bool(json.loads(p.read_text(encoding="utf-8")).get(TWEAKS_KEY))
    except (OSError, ValueError):
        return False


def _set_tweak(value: bool) -> None:
    p = _tweaks_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    try:
        data = json.loads(p.read_text(encoding="utf-8")) if p.exists() else {}
    except ValueError:
        data = {}
    data[TWEAKS_KEY] = value
    p.write_text(json.dumps(data, indent=2, sort_keys=True), encoding="utf-8")


def enable() -> None:
    _set_tweak(True)
    install_units()


def disable() -> None:
    _set_tweak(False)
    uninstall_units()


# ---------------------------------------------------------------------------
# Systemd-user unit management (timer + service)
# ---------------------------------------------------------------------------


def install_units() -> bool:
    """Install the user-level systemd timer + service from the
    repo/RPM-shipped sources, then enable + start the timer. Returns
    True on success."""
    if shutil.which("systemctl") is None:
        return False
    src = _data_systemd_dir()
    if src is None:
        return False
    dest = Path.home() / ".config/systemd/user"
    dest.mkdir(parents=True, exist_ok=True)
    for name in ("mackes-remmina-sync.service", "mackes-remmina-sync.timer"):
        s = src / name
        if not s.is_file():
            continue
        (dest / name).write_text(s.read_text(encoding="utf-8"),
                                 encoding="utf-8")
    subprocess.run(["systemctl", "--user", "daemon-reload"],
                   capture_output=True, timeout=10)
    rc = subprocess.run(
        ["systemctl", "--user", "enable", "--now", SYSTEMD_UNIT],
        capture_output=True, timeout=10,
    ).returncode
    return rc == 0


def uninstall_units() -> bool:
    if shutil.which("systemctl") is None:
        return False
    subprocess.run(
        ["systemctl", "--user", "disable", "--now", SYSTEMD_UNIT],
        capture_output=True, timeout=10,
    )
    dest = Path.home() / ".config/systemd/user"
    for name in ("mackes-remmina-sync.service", "mackes-remmina-sync.timer"):
        f = dest / name
        if f.exists():
            try:
                f.unlink()
            except OSError:
                pass
    subprocess.run(["systemctl", "--user", "daemon-reload"],
                   capture_output=True, timeout=10)
    return True


def _data_systemd_dir() -> Optional[Path]:
    for cand in (
        Path("/usr/share/mde/data/systemd"),
        Path(__file__).resolve().parent.parent / "data" / "systemd",
    ):
        if cand.is_dir():
            return cand
    return None


# ---------------------------------------------------------------------------
# CLI entry — `python -m mackes.remmina_sync`
# ---------------------------------------------------------------------------


def main(argv: Optional[list[str]] = None) -> int:
    import argparse
    p = argparse.ArgumentParser(
        prog="mackes-remmina-sync",
        description="Auto-populate Remmina with mesh SSH/RDP/VNC services",
    )
    p.add_argument("--once", action="store_true",
                   help="run one sync and exit (the systemd-service path)")
    p.add_argument("--enable", action="store_true",
                   help="enable the auto-sync (writes tweak + systemd timer)")
    p.add_argument("--disable", action="store_true",
                   help="disable the auto-sync")
    p.add_argument("--status", action="store_true",
                   help="print current state (enabled? last-sync?)")
    args = p.parse_args(argv)
    if args.enable:
        enable()
        print("Remmina auto-sync enabled — timer fires every 5 min.")
        return 0
    if args.disable:
        disable()
        print("Remmina auto-sync disabled.")
        return 0
    if args.status:
        print(f"enabled: {is_enabled()}")
        managed = _existing_managed_files()
        print(f"managed entries: {len(managed)}")
        return 0
    # Default + --once: run one sync
    report = sync()
    print(str(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
