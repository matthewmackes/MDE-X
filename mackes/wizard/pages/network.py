"""Wizard screen 7 — Network."""
from __future__ import annotations

import socket
from pathlib import Path

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.gtk_common import info_label, labeled_row, section_header


FIREWALL_ZONES = ["FedoraWorkstation", "public", "home", "work", "trusted", "block"]


# ─────────────────────────────────────────────────────────────────
# NF-14.3 (v2.5) — Nebula preflight check.
# ─────────────────────────────────────────────────────────────────
#
# Pre-flight verifies the operator's firewall + ISP doesn't block
# the two ports the v2.5 Nebula fabric needs:
#
#   UDP/4242  — native Nebula transport (direct UDP)
#   TCP/443   — covert HTTPS-tunnel fallback (NF-1.x)
#
# Failure surfaces an actionable "Open these ports in your
# firewall" page with a one-click `firewalld` rule for the common
# Fedora setup. Quick check: bind a socket locally; if bind fails
# with EADDRINUSE that's fine (something else is bound — likely
# the port is open + reachable). If bind fails with EACCES, the
# Linux capability layer + the operator's selinux profile are
# blocking us — surface that distinctly.

PREFLIGHT_PORTS = (
    (4242, "udp"),   # Nebula native
    (443, "tcp"),    # covert HTTPS tunnel
)


def nebula_preflight() -> list[dict]:
    """Pure-ish helper — try to bind each PREFLIGHT_PORTS entry
    locally. Returns one dict per port:

        {"port": int, "proto": str, "ok": bool, "detail": str}

    ok=True when the bind succeeded (port is free + we have the
    capability to use it). ok=False with a human-readable detail
    when something rejects the bind. The wizard's apply step
    surfaces the failures in a banner; firewall.py's "Allow
    Nebula" preset (NF-17.1) is the one-click fix.
    """
    out: list[dict] = []
    for port, proto in PREFLIGHT_PORTS:
        sock_type = socket.SOCK_DGRAM if proto == "udp" else socket.SOCK_STREAM
        s = socket.socket(socket.AF_INET, sock_type)
        try:
            s.bind(("0.0.0.0", port))
        except PermissionError:
            out.append({
                "port": port,
                "proto": proto,
                "ok": False,
                "detail": (
                    "Permission denied — privileged port requires "
                    "CAP_NET_BIND_SERVICE. The Nebula systemd units "
                    "grant this automatically; this preflight check "
                    "runs unprivileged and will fail on ports < 1024."
                ),
            })
        except OSError as e:
            if e.errno == 98:  # EADDRINUSE
                # Port is in use by something else — that's
                # actually GOOD news (it means the firewall isn't
                # blocking us; we just collide with another bound
                # socket). Treat as ok.
                out.append({
                    "port": port,
                    "proto": proto,
                    "ok": True,
                    "detail": "(already bound by another process — port is reachable)",
                })
            else:
                out.append({
                    "port": port,
                    "proto": proto,
                    "ok": False,
                    "detail": f"bind failed: {e}",
                })
        else:
            out.append({
                "port": port,
                "proto": proto,
                "ok": True,
                "detail": "bind succeeded",
            })
        finally:
            s.close()
    return out


def preflight_summary(rows: list[dict]) -> str:
    """Pure helper — one-line summary suitable for the wizard's
    inline status text. Returns "All Nebula ports reachable"
    when every row passed; "1 port blocked: UDP/4242" style
    otherwise.
    """
    failed = [r for r in rows if not r["ok"]]
    if not failed:
        return "All Nebula ports reachable"
    parts = [f"{r['proto'].upper()}/{r['port']}" for r in failed]
    return f"{len(failed)} port{'s' if len(failed) != 1 else ''} blocked: {', '.join(parts)}"


def build(ctx) -> Gtk.Widget:
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
    box.set_margin_top(28); box.set_margin_bottom(28)
    box.set_margin_start(40); box.set_margin_end(40)

    title = Gtk.Label(label="Network")
    title.set_xalign(0); title.get_style_context().add_class("title-1")
    box.pack_start(title, False, False, 0)

    box.pack_start(section_header("Quick Network Mesh"), False, False, 0)
    qnm = Gtk.Switch(); qnm.set_active(ctx.enable_qnm)
    qnm.connect("notify::active",
                lambda s, _g: setattr(ctx, "enable_qnm", s.get_active()))
    box.pack_start(labeled_row("Enable QNM", qnm), False, False, 0)
    box.pack_start(info_label("QNM is a standalone daemon Mackes proxies in the Network tab."),
                   False, False, 0)

    box.pack_start(section_header("Firewall"), False, False, 0)
    fw = Gtk.ComboBoxText()
    for z in FIREWALL_ZONES:
        fw.append_text(z)
    fw.set_active(FIREWALL_ZONES.index(ctx.firewall_zone)
                  if ctx.firewall_zone in FIREWALL_ZONES else 0)
    def on_zone(c):
        txt = c.get_active_text()
        if txt:
            ctx.firewall_zone = txt
    fw.connect("changed", on_zone)
    box.pack_start(labeled_row("Default zone", fw), False, False, 0)

    box.pack_start(section_header("VPN (optional)"), False, False, 0)
    path_label = Gtk.Label(label="(none)"); path_label.set_xalign(0)
    path_label.get_style_context().add_class("dim-label")
    import_btn = Gtk.Button(label="Import .ovpn / .conf …")
    def on_import(_):
        chooser = Gtk.FileChooserNative.new(
            "Import VPN config", None,
            Gtk.FileChooserAction.OPEN, "_Open", "_Cancel",
        )
        if chooser.run() == Gtk.ResponseType.ACCEPT:
            f = chooser.get_filename()
            if f and Path(f).exists():
                ctx.imported_vpn_path = f
                path_label.set_text(f)
        chooser.destroy()
    import_btn.connect("clicked", on_import)
    box.pack_start(labeled_row("Config", import_btn), False, False, 0)
    box.pack_start(path_label, False, False, 0)

    return box
