"""Network → Mesh Services panel — Carbon refresh (v1.1.x).

Mirrors docs/design/v1.1.0-carbon-refresh/project/panels-a.jsx::MeshServicesPanel
with the additional functional surfaces (native clients, CA cert install)
this panel needs over the prototype.

Layout, top to bottom:

  Breadcrumb + page title + subtitle
  Action row    — Scan now / mDNS bridge / live service count
  Peer pills    — filter the service grid
  Section: Discovered services  — Carbon tile grid (3 cols)
  Section: Unified gateway      — tile + toggle + route preview code block
  Section: mDNS bridge          — tile with relayed-type tag chips
"""
from __future__ import annotations

from pathlib import Path
from typing import List

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, ClickableTile,
    Notification, NotificationKind,
)
from mackes.mesh_services import (
    ServiceHit, load_catalog, load_registry, probe_all, url_for, launch,
)
from mackes.mdns_relay import DEFAULT_RELAYED_TYPES, DEFAULT_PRIVATE_TYPES


# ---- shared bits (mirror mesh_ssh.py — keep DRY-but-local) ---------------


def _page_title(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-page-title")
    return lab


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


def _breadcrumb(parts: list[str]) -> Gtk.Widget:
    bc = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
    bc.get_style_context().add_class("mackes-breadcrumb")
    for i, p in enumerate(parts):
        lab = Gtk.Label(label=p); lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != len(parts) - 1:
            sep = Gtk.Label(label="/"); sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _section_title(text: str, *, meta: str = "") -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    row.set_margin_top(28); row.set_margin_bottom(8)
    t = Gtk.Label(label=text); t.set_xalign(0)
    t.get_style_context().add_class("mackes-section-title")
    row.pack_start(t, True, True, 0)
    if meta:
        m = Gtk.Label(label=meta); m.set_xalign(1)
        m.get_style_context().add_class("mackes-section-meta")
        row.pack_end(m, False, False, 0)
    return row


def _tag(text: str, kind: str = "neutral") -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.get_style_context().add_class("mackes-tag")
    lab.get_style_context().add_class(kind)
    return lab


def _pill_button(label: str, *, active: bool, on_click) -> Gtk.Button:
    btn = Gtk.Button(label=label)
    btn.set_relief(Gtk.ReliefStyle.NONE)
    btn.get_style_context().add_class("mackes-tag")
    if active:
        btn.get_style_context().add_class("accent")
    else:
        btn.get_style_context().add_class("neutral")
    btn.connect("clicked", lambda *_: on_click())
    return btn


# ---- panel ----------------------------------------------------------------


class MeshServicesPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._filter = "all"   # "all" | peer name
        self._gateway_on = False
        self._build()
        self._refresh()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(32); outer.set_margin_bottom(32)
        outer.set_margin_start(40); outer.set_margin_end(40)

        outer.pack_start(_breadcrumb(["Mackes Shell", "Network", "Mesh Services"]),
                         False, False, 0)
        outer.pack_start(_page_title("Mesh Services"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Discover HTTP services across every mesh peer. Open in browser, "
            "launch native clients, or expose everything under a single "
            "https://media.mesh URL."
        ), False, False, 0)

        # ---- Action row ----
        action_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        action_row.set_margin_top(8); action_row.set_margin_bottom(16)
        action_row.pack_start(
            Button("Scan now", kind=ButtonKind.PRIMARY,
                   icon_name="view-refresh-symbolic", on_click=self._on_scan_now),
            False, False, 0)
        action_row.pack_start(
            Button("mDNS bridge", kind=ButtonKind.GHOST,
                   icon_name="emblem-system-symbolic",
                   on_click=lambda: None),
            False, False, 0)
        self._service_count = Gtk.Label(label="")
        self._service_count.set_xalign(1)
        self._service_count.get_style_context().add_class("mackes-section-meta")
        action_row.pack_end(self._service_count, False, False, 0)
        outer.pack_start(action_row, False, False, 0)

        # ---- Peer filter pills (FlowBox so they wrap) ----
        self._pills_box = Gtk.FlowBox()
        self._pills_box.set_max_children_per_line(20)
        self._pills_box.set_selection_mode(Gtk.SelectionMode.NONE)
        self._pills_box.set_column_spacing(8)
        self._pills_box.set_row_spacing(8)
        outer.pack_start(self._pills_box, False, False, 0)

        # ---- Discovered services grid ----
        outer.pack_start(_section_title("Discovered services"), False, False, 0)
        self._svc_grid = Gtk.FlowBox()
        self._svc_grid.set_valign(Gtk.Align.START)
        self._svc_grid.set_max_children_per_line(3)
        self._svc_grid.set_min_children_per_line(1)
        self._svc_grid.set_selection_mode(Gtk.SelectionMode.NONE)
        self._svc_grid.set_homogeneous(True)
        self._svc_grid.set_column_spacing(8)
        self._svc_grid.set_row_spacing(8)
        outer.pack_start(self._svc_grid, False, False, 0)

        # ---- Unified gateway ----
        outer.pack_start(_section_title("Unified gateway",
                                       meta="https://media.mesh"),
                         False, False, 0)
        gw_tile = Tile()
        gw_head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        # Body
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        gw_title = Gtk.Label(label="Caddy reverse proxy")
        gw_title.set_xalign(0); gw_title.get_style_context().add_class("mackes-section-title")
        body.pack_start(gw_title, False, False, 0)
        gw_sub = Gtk.Label(label=(
            "Exposes every mesh service at https://media.mesh/<service>/<peer>/ "
            "with auto-renewed certs from a private CA installed into each peer's "
            "trust store."
        ))
        gw_sub.set_xalign(0); gw_sub.set_line_wrap(True)
        gw_sub.get_style_context().add_class("mackes-page-subtitle")
        body.pack_start(gw_sub, False, False, 0)
        gw_head.pack_start(body, True, True, 0)

        # Toggle column
        right = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        right.set_valign(Gtk.Align.CENTER)
        gw_lbl = Gtk.Label(label="Gateway"); gw_lbl.set_xalign(1)
        gw_lbl.get_style_context().add_class("dim-label")
        right.pack_start(gw_lbl, False, False, 0)
        self._gw_switch = Gtk.Switch()
        self._gw_switch.connect("notify::active", self._on_gateway_toggled)
        right.pack_start(self._gw_switch, False, False, 0)
        ca_btn = Button("Install CA", kind=ButtonKind.TERTIARY,
                        on_click=self._on_install_ca)
        right.pack_start(ca_btn, False, False, 0)
        gw_head.pack_end(right, False, False, 0)
        gw_tile.pack(gw_head)

        # Route preview code block (rebuilt on refresh)
        self._gw_routes = Gtk.TextView()
        self._gw_routes.set_monospace(True); self._gw_routes.set_editable(False)
        self._gw_routes.get_style_context().add_class("mackes-code")
        gw_tile.pack(self._gw_routes)
        outer.pack_start(gw_tile, False, False, 0)

        # ---- Bundled native clients (kept from v1.0; folded under Gateway) ----
        nc_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        nc_row.set_margin_top(8)
        nc_row.pack_start(Button("Refresh native client server lists",
                                 kind=ButtonKind.GHOST,
                                 on_click=self._on_refresh_native_clients),
                          False, False, 0)
        self._native_status = Gtk.Label(label="(idle)")
        self._native_status.set_xalign(0); self._native_status.set_line_wrap(True)
        self._native_status.get_style_context().add_class("mackes-section-meta")
        nc_row.pack_start(self._native_status, True, True, 0)
        outer.pack_start(nc_row, False, False, 0)

        # ---- mDNS bridge ----
        outer.pack_start(_section_title("mDNS bridge",
                                       meta="relay announcements across the mesh"),
                         False, False, 0)
        mdns_tile = Tile()
        mdns_head = Gtk.Label(label="Service types currently relayed:")
        mdns_head.set_xalign(0); mdns_head.get_style_context().add_class("dim-label")
        mdns_tile.pack(mdns_head)
        mdns_chips = Gtk.FlowBox()
        mdns_chips.set_max_children_per_line(20)
        mdns_chips.set_selection_mode(Gtk.SelectionMode.NONE)
        mdns_chips.set_column_spacing(6); mdns_chips.set_row_spacing(6)
        mdns_chips.set_margin_top(8)
        for t in DEFAULT_RELAYED_TYPES:
            mdns_chips.add(_tag(t, "info"))
        mdns_tile.pack(mdns_chips)
        priv = Gtk.Label(
            label=("+ %d private types kept local (%s, …)"
                   % (len(DEFAULT_PRIVATE_TYPES), ", ".join(DEFAULT_PRIVATE_TYPES[:3])))
        )
        priv.set_xalign(0); priv.set_line_wrap(True)
        priv.get_style_context().add_class("mackes-section-meta")
        priv.set_margin_top(8)
        mdns_tile.pack(priv)
        outer.pack_start(mdns_tile, False, False, 0)

        # Scroll the whole panel
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- refresh -------------------------------------------------------

    def _refresh(self) -> None:
        hits = load_registry()
        # Filter pills — based on unique peers in current hits
        peer_names = sorted({h.peer for h in hits})
        for c in list(self._pills_box.get_children()):
            self._pills_box.remove(c)
        # "All peers" pill
        all_pill = _pill_button(
            "All peers", active=(self._filter == "all"),
            on_click=lambda: self._set_filter("all"),
        )
        self._pills_box.add(all_pill)
        for name in peer_names:
            self._pills_box.add(_pill_button(
                f"● {name.replace('.mesh', '')}",
                active=(self._filter == name),
                on_click=lambda n=name: self._set_filter(n),
            ))
        self._pills_box.show_all()

        # Filtered service grid
        for c in list(self._svc_grid.get_children()):
            self._svc_grid.remove(c)
        filtered = hits if self._filter == "all" else [h for h in hits if h.peer == self._filter]
        catalog = {d.name: d for d in load_catalog()}
        self._service_count.set_text(
            f"{len(filtered)} service(s) on {len({h.peer for h in filtered})} peer(s)"
        )
        if not filtered:
            empty = Notification(
                "No services discovered" if not hits else "No services for this peer",
                body='Click "Scan now" to probe each mesh peer.'
                     if not hits else "Pick a different peer or All peers.",
                kind=NotificationKind.INFO, dismissible=False,
            )
            self._svc_grid.add(empty)
        else:
            for hit in filtered:
                self._svc_grid.add(_service_card(hit, catalog))
        self._svc_grid.show_all()

        # Gateway routes preview (only if enabled)
        if self._gateway_on:
            lines = []
            for h in filtered:
                kind = (catalog.get(h.service).name if h.service in catalog else h.service)
                lines.append(f"https://media.mesh/{kind}/{h.peer}/    →  {url_for(h)}")
            if not lines:
                lines = ["(no routes — no services discovered)"]
            self._gw_routes.get_buffer().set_text("\n".join(lines))
            self._gw_routes.show()
        else:
            self._gw_routes.get_buffer().set_text("(gateway disabled)")
            self._gw_routes.hide()

    def _set_filter(self, value: str) -> None:
        self._filter = value
        self._refresh()

    # ---- actions -------------------------------------------------------

    def _on_scan_now(self) -> None:
        peers = []
        try:
            from mackes.mesh_vpn import headscale_list_peers
            peers = [p.name for p in headscale_list_peers()]
        except Exception:  # noqa: BLE001
            pass
        if not peers:
            import os
            home = Path(os.path.expanduser("~"))
            mesh_root = home / "QNM-Mesh"
            if mesh_root.exists():
                peers = [d.name for d in mesh_root.iterdir() if d.is_dir()]
        probe_all(peers)
        self._refresh()

    def _on_gateway_toggled(self, switch: Gtk.Switch, _gp) -> None:
        self._gateway_on = switch.get_active()
        if self._gateway_on:
            try:
                from mackes.caddy_gateway import enable_gateway
                enable_gateway()
            except Exception:  # noqa: BLE001
                pass
        self._refresh()

    def _on_install_ca(self) -> None:
        try:
            from mackes.caddy_gateway import install_ca_into_trust_store
            install_ca_into_trust_store()
        except Exception:  # noqa: BLE001
            pass

    def _on_refresh_native_clients(self) -> None:
        try:
            from mackes.native_clients import refresh_all
            results = refresh_all()
        except Exception as e:  # noqa: BLE001
            results = [f"refresh failed: {e}"]
        self._native_status.set_text("  ·  ".join(results)[:200])


# ---- helpers --------------------------------------------------------------


def _service_card(hit: ServiceHit, catalog: dict) -> Gtk.Widget:
    name = (catalog.get(hit.service).display
            if hit.service in catalog and catalog.get(hit.service) else hit.service)
    tile = ClickableTile(on_click=(lambda h=hit: launch(h) and None))
    # Top row: kind tag + status dot
    top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    top.pack_start(_tag(hit.service, "neutral"), False, False, 0)
    status = "ok" if getattr(hit, "status", "ok") == "ok" else "fail"
    dot = Gtk.Label(label="●")
    dot.get_style_context().add_class("mackes-dot")
    dot.get_style_context().add_class(status)
    top.pack_end(dot, False, False, 0)
    tile.pack(top)
    # Service name
    n = Gtk.Label(label=name); n.set_xalign(0)
    n.get_style_context().add_class("mackes-section-title")
    tile.pack(n)
    # Peer subtitle (mono helper)
    peer = Gtk.Label(label=f"on {hit.peer}.mesh"); peer.set_xalign(0)
    peer.get_style_context().add_class("mackes-section-meta")
    tile.pack(peer)
    # URL
    url = Gtk.Label(label=url_for(hit)); url.set_xalign(0)
    url.set_selectable(True); url.set_ellipsize(__import__("gi").repository.Pango.EllipsizeMode.END)
    url.get_style_context().add_class("mackes-tag")
    url.get_style_context().add_class("accent")
    tile.pack(url)
    return tile
