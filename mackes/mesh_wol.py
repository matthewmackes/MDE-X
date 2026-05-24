"""Mesh Wake-on-LAN.

Pure-Python magic-packet sender + ARP-cache MAC lookup. Used by the
Mesh Performance panel ("Wake" button on every offline peer) and the
Get Online wizard (auto-fire wake before a connection attempt).

References (open-source standards we're following):
  RFC 2965-ish Wake-on-LAN magic packet format — 6 × 0xFF then 16 ×
  the 6-byte destination MAC. Total 102 bytes. Sent to UDP/9 or UDP/7
  broadcast.

Public API:

  wake(mac, *, broadcast="255.255.255.255", port=9) -> bool
  peer_mac(ip_or_hostname) -> str | None
  wake_peer(peer_name_or_ip) -> tuple[bool, str]
  cache_peer_macs() -> int   # fills the cache from the current ARP table
"""
from __future__ import annotations

import warnings as _warnings

_warnings.warn(
    "mackes.mesh_wol is deprecated. Per-peer MAC inventory comes from "
    "the topology snapshot in `mackesd_core::topology`, and Wake-on-LAN "
    "dispatch is reconciled (request → ack → retry) through "
    "`mackesd_core::reconcile`. See "
    "docs/design/v12.0-enterprise-mesh.md and "
    "docs/MIGRATION_TO_MACKESD.md. This Python module is retained for "
    "the 1.x compatibility window and will be removed in 2.0.",
    DeprecationWarning,
    stacklevel=2,
)

import json
import re
import socket
import subprocess
from pathlib import Path
from typing import Optional


MAC_CACHE_PATH = Path.home() / ".config/mackes-shell/peer-macs.json"

_MAC_RE = re.compile(
    r"^([0-9a-fA-F]{2}([:-]?))(?:[0-9a-fA-F]{2}\2){4}[0-9a-fA-F]{2}$"
)


# ---------------------------------------------------------------------------
# Magic packet
# ---------------------------------------------------------------------------


def _normalise_mac(mac: str) -> Optional[bytes]:
    """Accept aa:bb:cc:dd:ee:ff / aa-bb-... / aabbccddeeff. Return raw
    6 bytes, or None if the input is malformed."""
    if not mac:
        return None
    if not _MAC_RE.match(mac):
        return None
    # Bare-hex form first (no separator → can't split)
    if ":" not in mac and "-" not in mac:
        if len(mac) == 12:
            try:
                return bytes.fromhex(mac)
            except ValueError:
                return None
        return None
    parts = re.split(r"[:-]", mac)
    if len(parts) != 6:
        return None
    try:
        return bytes(int(p, 16) for p in parts)
    except ValueError:
        return None


def wake(mac: str, *, broadcast: str = "255.255.255.255",
         port: int = 9) -> bool:
    """Send the Wake-on-LAN magic packet for `mac` to UDP `broadcast:port`.

    Returns True on successful send (not on successful wake — the
    target machine has to receive + act on it).
    """
    raw = _normalise_mac(mac)
    if raw is None:
        return False
    packet = b"\xff" * 6 + raw * 16
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        s.sendto(packet, (broadcast, port))
        # Belt-and-suspenders: many switches forward only UDP/9, but
        # legacy clients listen on UDP/7. Send both.
        s.sendto(packet, (broadcast, 7))
        s.close()
        return True
    except OSError:
        return False


# ---------------------------------------------------------------------------
# ARP-cache MAC lookup
# ---------------------------------------------------------------------------


def peer_mac(host: str) -> Optional[str]:
    """Best-effort MAC lookup for `host` (IP or hostname).

    Tries (in order):
      1. The Mackes peer-MAC cache (populated by cache_peer_macs())
      2. /proc/net/arp parse
      3. `ip neigh show` parse
    """
    cached = _read_cache().get(host)
    if cached:
        return cached
    # Resolve hostname → IP first
    try:
        ip = socket.gethostbyname(host)
    except OSError:
        ip = host
    cached = _read_cache().get(ip)
    if cached:
        return cached
    # /proc/net/arp
    try:
        with open("/proc/net/arp", encoding="utf-8") as f:
            for line in f.readlines()[1:]:
                cols = line.split()
                if len(cols) >= 4 and cols[0] == ip and cols[3] != "00:00:00:00:00:00":
                    return cols[3]
    except OSError:
        pass
    # `ip neigh`
    try:
        r = subprocess.run(
            ["ip", "neigh", "show", ip],
            capture_output=True, text=True, timeout=3,
        )
        for line in (r.stdout or "").splitlines():
            parts = line.split()
            for i, p in enumerate(parts):
                if p == "lladdr" and i + 1 < len(parts):
                    return parts[i + 1]
    except (OSError, subprocess.TimeoutExpired):
        pass
    return None


def cache_peer_macs() -> int:
    """Snapshot every (peer_ip, mac) pair currently in the ARP table
    plus the mesh peer list, persisting to MAC_CACHE_PATH. Returns the
    number of cached entries.

    Run this regularly (every few minutes) so we still know peer MACs
    after they go offline."""
    cache: dict[str, str] = _read_cache()
    # ARP table
    try:
        with open("/proc/net/arp", encoding="utf-8") as f:
            for line in f.readlines()[1:]:
                cols = line.split()
                if (len(cols) >= 4
                        and cols[3] != "00:00:00:00:00:00"
                        and _MAC_RE.match(cols[3])):
                    cache[cols[0]] = cols[3]
    except OSError:
        pass
    # Mesh peers — also map peer_name → mac so we don't need IP first.
    # NF-13.6 (v2.5, 2026-05-23): prefer Nebula overlay roster via
    # mackes.mesh_nebula.nebula_peer_ips; fall back to the legacy
    # tailscale path during the migration window.
    try:
        from mackes.mesh_nebula import nebula_peer_ips
        for name, mip in nebula_peer_ips():
            if mip and mip in cache and name:
                cache[name.split(".", 1)[0]] = cache[mip]
    except Exception:  # noqa: BLE001
        pass
    try:
        from mackes.mesh_vpn import tailscale_status
        for p in (tailscale_status().get("peers") or []):
            mip = p.get("mesh_ip", "")
            name = p.get("name", "")
            if mip and mip in cache:
                if name:
                    cache[name.split(".", 1)[0]] = cache[mip]
    except Exception:  # noqa: BLE001
        pass
    _write_cache(cache)
    return len(cache)


def _read_cache() -> dict:
    if not MAC_CACHE_PATH.exists():
        return {}
    try:
        return json.loads(MAC_CACHE_PATH.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def _write_cache(data: dict) -> None:
    try:
        MAC_CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
        MAC_CACHE_PATH.write_text(json.dumps(data, indent=2, sort_keys=True),
                                  encoding="utf-8")
    except OSError:
        pass


# ---------------------------------------------------------------------------
# High-level wrappers
# ---------------------------------------------------------------------------


def wake_peer(peer_name_or_ip: str) -> tuple[bool, str]:
    """Look up the peer's MAC + send a magic packet. Returns
    (success, human-readable message)."""
    mac = peer_mac(peer_name_or_ip)
    if mac is None:
        return False, (
            f"no cached MAC for {peer_name_or_ip} — connect at least "
            "once while online so the ARP cache learns its MAC"
        )
    ok = wake(mac)
    return ok, (
        f"sent magic packet to {mac} for {peer_name_or_ip}"
        if ok else f"send failed for {mac}"
    )


__all__ = [
    "wake", "wake_peer", "peer_mac", "cache_peer_macs",
    "MAC_CACHE_PATH",
]
