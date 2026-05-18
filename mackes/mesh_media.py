"""Mesh media-server discovery — Airsonic / Subsonic / Jellyfin (v2.1.0).

The Media Sync daemon and the Thunar `Mackes Media/` view both read
from `discover()` to figure out what media servers are reachable via
the mesh interface (`tailscale0`).

Discovery strategy:
  1. mDNS push — browse `_subsonic._tcp` and `_jellyfin._tcp` on the
     mesh interface only (no LAN bleed).
  2. TCP port probe fallback — every cycle, for any tailscale peer not
     yet seen via mDNS, probe :4040 (Airsonic) and :8096 (Jellyfin)
     with a 250ms connect timeout each. Catches servers running on
     peers that haven't joined the mDNS announcement (e.g. a stock
     Airsonic install on a non-Mackes node).

Public API:

  KIND_AIRSONIC      → str   ("airsonic")
  KIND_JELLYFIN      → str   ("jellyfin")
  MediaServer        → dataclass with .kind / .host / .ip / .port / .name
  discover()         → list[MediaServer] (deduped union of mDNS + probe)
"""
from __future__ import annotations

import json
import socket
import subprocess
from dataclasses import dataclass
from typing import List


KIND_AIRSONIC = "airsonic"
KIND_JELLYFIN = "jellyfin"

_AIRSONIC_PORT = 4040
_JELLYFIN_PORT = 8096

_MDNS_SUBSONIC = "_subsonic._tcp.local."
_MDNS_JELLYFIN = "_jellyfin._tcp.local."


@dataclass(frozen=True)
class MediaServer:
    """One media server reachable on the mesh."""
    kind: str         # KIND_AIRSONIC | KIND_JELLYFIN
    host: str         # hostname.local (display name)
    ip: str           # resolved IPv4
    port: int
    name: str = ""    # mDNS service-instance name (empty if discovered via probe)

    @property
    def url(self) -> str:
        scheme = "http"  # mesh-internal; tailscale provides the trust layer
        return f"{scheme}://{self.ip}:{self.port}"


# ---------------------------------------------------------------------------
# mDNS path
# ---------------------------------------------------------------------------


def _scan_mdns(timeout: float = 2.0) -> List[MediaServer]:
    """Browse `_subsonic._tcp` + `_jellyfin._tcp` on the mesh interface.

    Returns an empty list if python-zeroconf isn't installed."""
    try:
        from zeroconf import ServiceBrowser, Zeroconf
    except ImportError:
        return []

    import threading
    found: List[MediaServer] = []
    seen: set[str] = set()
    deadline = threading.Event()

    class _Listener:
        def __init__(self, kind: str) -> None:
            self._kind = kind

        def add_service(self, zc, type_, name):
            if name in seen:
                return
            seen.add(name)
            try:
                info = zc.get_service_info(type_, name, timeout=int(timeout * 1000))
            except Exception:  # noqa: BLE001
                return
            if info is None:
                return
            ip = ""
            if info.addresses:
                try:
                    ip = socket.inet_ntoa(info.addresses[0])
                except OSError:
                    pass
            if not ip:
                return
            host = (info.server or "").rstrip(".") or ip
            found.append(MediaServer(
                kind=self._kind,
                host=host,
                ip=ip,
                port=int(info.port or
                          (_AIRSONIC_PORT if self._kind == KIND_AIRSONIC
                           else _JELLYFIN_PORT)),
                name=name,
            ))

        def remove_service(self, zc, type_, name):  # noqa: ARG002
            pass

        def update_service(self, zc, type_, name):  # noqa: ARG002
            pass

    zc = None
    try:
        zc = Zeroconf()
        ServiceBrowser(zc, _MDNS_SUBSONIC, listener=_Listener(KIND_AIRSONIC))
        ServiceBrowser(zc, _MDNS_JELLYFIN, listener=_Listener(KIND_JELLYFIN))
        deadline.wait(timeout=timeout)
    except Exception:  # noqa: BLE001
        pass
    finally:
        if zc is not None:
            try:
                zc.close()
            except Exception:  # noqa: BLE001
                pass
    return found


# ---------------------------------------------------------------------------
# TCP port probe fallback
# ---------------------------------------------------------------------------


def _tailscale_peer_ips() -> List[tuple[str, str]]:
    """Return (hostname, ip) pairs for every tailscale peer including self."""
    try:
        proc = subprocess.run(
            ["tailscale", "status", "--json"],
            capture_output=True, text=True, timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        return []
    if proc.returncode != 0:
        return []
    try:
        data = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return []
    peers: List[tuple[str, str]] = []
    self_info = data.get("Self") or {}
    self_ips = self_info.get("TailscaleIPs") or []
    if self_ips:
        peers.append((self_info.get("HostName") or "self", self_ips[0]))
    for _key, p in (data.get("Peer") or {}).items():
        ips = p.get("TailscaleIPs") or []
        if not ips or not p.get("Online"):
            continue
        peers.append((
            p.get("HostName") or p.get("DNSName", "").rstrip(".") or ips[0],
            ips[0],
        ))
    return peers


def _probe_port(ip: str, port: int, timeout: float = 0.25) -> bool:
    """One TCP connect with a short timeout."""
    try:
        with socket.create_connection((ip, port), timeout=timeout):
            return True
    except OSError:
        return False


def _scan_probe(already_seen: set[tuple[str, int]]) -> List[MediaServer]:
    """For every tailscale peer not in `already_seen`, probe the two media ports."""
    found: List[MediaServer] = []
    for host, ip in _tailscale_peer_ips():
        if (ip, _AIRSONIC_PORT) not in already_seen \
                and _probe_port(ip, _AIRSONIC_PORT):
            found.append(MediaServer(KIND_AIRSONIC, host, ip, _AIRSONIC_PORT))
        if (ip, _JELLYFIN_PORT) not in already_seen \
                and _probe_port(ip, _JELLYFIN_PORT):
            found.append(MediaServer(KIND_JELLYFIN, host, ip, _JELLYFIN_PORT))
    return found


# ---------------------------------------------------------------------------
# Public surface
# ---------------------------------------------------------------------------


def discover(*, mdns_timeout: float = 2.0,
             enable_probe: bool = True) -> List[MediaServer]:
    """Return the deduped union of mDNS + port-probe results."""
    servers = _scan_mdns(timeout=mdns_timeout)
    if enable_probe:
        seen = {(s.ip, s.port) for s in servers}
        servers.extend(_scan_probe(seen))
    # Dedupe on (kind, ip, port) — keep the first (mDNS-discovered) entry
    # so we preserve the friendly mDNS name when both sources agree.
    deduped: dict[tuple[str, str, int], MediaServer] = {}
    for s in servers:
        key = (s.kind, s.ip, s.port)
        if key not in deduped:
            deduped[key] = s
    return list(deduped.values())


__all__ = [
    "KIND_AIRSONIC", "KIND_JELLYFIN",
    "MediaServer",
    "discover",
]
