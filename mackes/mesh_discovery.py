"""Mesh-join credential discovery — fallback chain (v1.7.0).

The Join flow asks: "where is the mesh I should join?" Most of the time
the user has just copied a `mackes://` link on another machine and a
clipboard scan is the right answer. Sometimes the control node is on
the same LAN and mDNS can find it. The rest of the time the user
pastes the link by hand.

Public API:

  scan_clipboard()         → Optional[str]              — mackes:// from
                                                          GtkClipboard if any
  scan_mdns(timeout=2.0)   → list[ControlEndpoint]      — _mackes-mesh._tcp
                                                          peers on the LAN
  discover(timeout=2.0)    → DiscoveryResult            — the fallback chain

`discover()` returns a DiscoveryResult that names the source and the
credential (if found). Callers check `.source` to decide how to render
the join page (auto-fill vs picker vs manual paste).

Optional dependencies are detected at runtime. Clipboard scan is always
available because Gtk.Clipboard is part of GTK. mDNS browse requires
python3-zeroconf (already a Recommends dep). QR scan would require
zbar-tools + a webcam and is deferred until we actually ship that.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import List, Optional


# A mackes:// link looks like:
#   mackes://join/<mesh-id>?key=<token>&control=https://<host-or-ip>:<port>
# We accept a permissive pattern — the headscale_setup wizard validates
# the structure when it tries to redeem the pre-auth.
_MACKES_LINK = re.compile(r"\bmackes://join/[A-Za-z0-9_\-]+\?[^\s]+")

# mDNS service type advertised by Mackes control nodes.
MESH_SERVICE_TYPE = "_mackes-mesh._tcp.local."


@dataclass
class ControlEndpoint:
    """One Mackes control node discovered on the local network."""
    name: str           # mDNS service-instance name (e.g. "mackes-mesh-foo")
    host: str           # hostname.local (used for friendly display)
    ip: str             # resolved IPv4
    port: int           # Headscale serve port (typically 8080)
    mesh_id: str = ""   # from TXT record `mesh_id=…` if present
    control_url: str = ""  # from TXT record `control_url=…` if present


@dataclass
class DiscoveryResult:
    """Outcome of one pass through the discovery fallback chain."""
    source: str = "manual"                # clipboard | mdns | manual
    link: Optional[str] = None            # mackes://… if clipboard hit
    candidates: List[ControlEndpoint] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Step 1 — clipboard
# ---------------------------------------------------------------------------


def scan_clipboard() -> Optional[str]:
    """Return the first mackes:// URL on the system clipboard, if any.

    Returns None when:
      * GTK isn't available (headless or pre-init)
      * the clipboard is empty
      * no mackes:// URL is present
    """
    try:
        import gi
        gi.require_version("Gtk", "3.0")
        gi.require_version("Gdk", "3.0")
        from gi.repository import Gdk, Gtk
        clip = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
        if clip is None:
            return None
        text = clip.wait_for_text()
    except Exception:  # noqa: BLE001
        return None
    if not text:
        return None
    m = _MACKES_LINK.search(text)
    if m is None:
        text = text.strip()
        return text if text.startswith("mackes://") else None
    return m.group(0)


# ---------------------------------------------------------------------------
# Step 2 — mDNS browse
# ---------------------------------------------------------------------------


def scan_mdns(timeout: float = 2.0) -> List[ControlEndpoint]:
    """Browse the LAN for _mackes-mesh._tcp announcements.

    Returns an empty list if python-zeroconf isn't installed, or if no
    mesh control nodes are announcing. Bounded by `timeout` seconds —
    keeps the Join page responsive.
    """
    try:
        from zeroconf import ServiceBrowser, Zeroconf
    except ImportError:
        return []

    import socket
    import threading

    found: List[ControlEndpoint] = []
    seen_names: set[str] = set()
    done = threading.Event()

    class _Listener:
        def add_service(self, zc, type_, name):
            if name in seen_names:
                return
            seen_names.add(name)
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
            txt: dict[str, str] = {}
            if info.properties:
                for k, v in info.properties.items():
                    try:
                        kk = k.decode("utf-8") if isinstance(k, bytes) else str(k)
                        vv = v.decode("utf-8") if isinstance(v, bytes) else (
                            "" if v is None else str(v)
                        )
                        txt[kk] = vv
                    except UnicodeDecodeError:
                        continue
            found.append(ControlEndpoint(
                name=name,
                host=(info.server or "").rstrip("."),
                ip=ip,
                port=int(info.port or 8080),
                mesh_id=txt.get("mesh_id", ""),
                control_url=txt.get("control_url", ""),
            ))

        def remove_service(self, zc, type_, name):  # noqa: ARG002
            pass

        def update_service(self, zc, type_, name):  # noqa: ARG002
            pass

    zc = None
    try:
        zc = Zeroconf()
        ServiceBrowser(zc, MESH_SERVICE_TYPE, listener=_Listener())
        done.wait(timeout=timeout)
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
# Step 3 — the chain
# ---------------------------------------------------------------------------


def discover(timeout: float = 2.0) -> DiscoveryResult:
    """Run the full discovery fallback chain.

    Order is clipboard → mDNS → manual (caller renders an entry field
    when source == "manual"). Each step that comes back empty falls
    through to the next; a hit short-circuits the chain.
    """
    link = scan_clipboard()
    if link is not None:
        return DiscoveryResult(source="clipboard", link=link)

    candidates = scan_mdns(timeout=timeout)
    if candidates:
        return DiscoveryResult(source="mdns", candidates=candidates)

    return DiscoveryResult(source="manual")


__all__ = [
    "ControlEndpoint", "DiscoveryResult",
    "MESH_SERVICE_TYPE",
    "scan_clipboard", "scan_mdns", "discover",
]
