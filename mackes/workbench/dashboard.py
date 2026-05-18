"""Dashboard — Carbon refresh (v1.1.0).

Layout, top to bottom (matches docs/design/v1.1.0-carbon-refresh/project/panels-a.jsx:6):

  Page title + subtitle
  Stat tiles row (4)         — mesh peers / services / sshd / drift
  Service health grid
  Drift card                 — conditional, Carbon notification (warning)
  Hardware summary           — 2x2 stat tiles
  Recent activity
  Quick actions
"""
from __future__ import annotations

from typing import Callable, List, Optional, Tuple

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from pathlib import Path

from mackes.presets import active_preset_drift
from mackes.snapshots import create_snapshot
from mackes.state import (
    LOG_DIR,
    MackesState,
    hardware_summary,
    last_snapshot,
    service_health,
)


def _hero_logo_path() -> Optional[Path]:
    candidates = [
        Path("/usr/share/mackes-shell/branding/MACKES-XFCE-LOGO.png"),
        Path(__file__).resolve().parents[2] / "branding" / "MACKES-XFCE-LOGO.png",
    ]
    for p in candidates:
        if p.exists():
            return p
    return None


_STATUS_TAG_KIND = {
    "ok": "success",
    "warn": "warning",
    "fail": "error",
    "missing": "neutral",
}


# ---------------------------------------------------------------------------
# Carbon component helpers
# ---------------------------------------------------------------------------


def _page_title(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-page-title")
    return lab


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0)
    lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


def _section_title(text: str) -> Gtk.Widget:
    head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
    head.set_margin_top(12); head.set_margin_bottom(12)
    title = Gtk.Label(label=text)
    title.set_xalign(0)
    title.get_style_context().add_class("mackes-section-title")
    head.pack_start(title, True, True, 0)
    return head


def _section_description(text: str) -> Gtk.Widget:
    """Plain-language explainer below a section title."""
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-section-description")
    return lab


def _stat_tile(label: str, value: str, foot: str = "", *,
               accent: bool = False) -> Gtk.Widget:
    """Match docs/design/.../tokens.css .stat-tile."""
    tile = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
    tile.get_style_context().add_class("mackes-stat-tile")
    if accent:
        tile.get_style_context().add_class("accent")
    tile.set_margin_top(0); tile.set_margin_bottom(0)
    tile.set_size_request(-1, 110)

    lab = Gtk.Label(label=label.upper())
    lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-stat-label")
    tile.pack_start(lab, False, False, 0)

    val = Gtk.Label(label=str(value))
    val.set_xalign(0)
    val.get_style_context().add_class("mackes-stat-value")
    tile.pack_start(val, True, True, 0)

    foot_lbl = Gtk.Label(label=foot)
    foot_lbl.set_xalign(0)
    foot_lbl.get_style_context().add_class("mackes-stat-foot")
    tile.pack_start(foot_lbl, False, False, 0)
    return tile


def _tag(text: str, kind: str = "neutral") -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.get_style_context().add_class("mackes-tag")
    lab.get_style_context().add_class(kind)
    return lab


def _notification(kind: str, title: str, body: str,
                  actions: Optional[List[Tuple[str, Callable[[], None]]]] = None) -> Gtk.Widget:
    notif = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    notif.get_style_context().add_class("mackes-notif")
    notif.get_style_context().add_class(kind)

    t = Gtk.Label(label=title)
    t.set_xalign(0)
    t.get_style_context().add_class("mackes-notif-title")
    notif.pack_start(t, False, False, 0)

    b = Gtk.Label(label=body)
    b.set_xalign(0); b.set_line_wrap(True)
    b.get_style_context().add_class("mackes-notif-body")
    notif.pack_start(b, False, False, 0)

    if actions:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        row.set_margin_top(8)
        for label, fn in actions:
            btn = Gtk.Button(label=label)
            btn.get_style_context().add_class("cds-button-tertiary")
            btn.connect("clicked", lambda _b, f=fn: f())
            row.pack_start(btn, False, False, 0)
        notif.pack_start(row, False, False, 0)
    return notif


# ---------------------------------------------------------------------------
# Dashboard view
# ---------------------------------------------------------------------------


class DashboardView(Gtk.Box):
    def __init__(self, state: MackesState,
                 navigate: Optional[Callable[[str], None]] = None) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.state = state
        self.navigate = navigate or (lambda _t: None)

        # Scrolled content with Carbon page padding (32 / 40)
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        inner.set_margin_top(12); inner.set_margin_bottom(12)
        inner.set_margin_start(16); inner.set_margin_end(16)
        scroller.add(inner)
        self.pack_start(scroller, True, True, 0)

        self._inner = inner
        self._render()

    def _render(self) -> None:
        for child in list(self._inner.get_children()):
            self._inner.remove(child)

        self._inner.pack_start(_page_title("Dashboard"), False, False, 0)
        sub = (
            f"Preset: {(self.state.active_preset or '—').title()}.  "
            "Everything you control today."
        )
        self._inner.pack_start(_page_subtitle(sub), False, False, 0)
        self._inner.pack_start(_section_description(
            "This page shows how healthy your machine is at a glance. "
            "Green means everything is working; yellow or red means "
            "something needs your attention."
        ), False, False, 0)

        # ---- Stat tiles row -------------------------------------------
        self._inner.pack_start(self._stat_row(), False, False, 0)

        # ---- Service health grid --------------------------------------
        self._inner.pack_start(_section_title("Service health"), False, False, 0)
        self._inner.pack_start(_section_description(
            "The background programs your machine needs to stay healthy. "
            "If any are stopped, related panels won't work right."
        ), False, False, 0)
        self._inner.pack_start(self._service_grid(), False, False, 0)

        # ---- Drift (conditional notification) -------------------------
        drift = self._drift_card()
        if drift is not None:
            self._inner.pack_start(drift, False, False, 0)

        # ---- Hardware -------------------------------------------------
        self._inner.pack_start(_section_title("This machine"), False, False, 0)
        self._inner.pack_start(_section_description(
            "A quick look at the hardware Mackes is running on."
        ), False, False, 0)
        self._inner.pack_start(self._hardware_grid(), False, False, 0)

        # ---- Quick actions --------------------------------------------
        self._inner.pack_start(_section_title("Quick actions"), False, False, 0)
        self._inner.pack_start(_section_description(
            "Shortcuts to the panels you'll reach for most often."
        ), False, False, 0)
        self._inner.pack_start(self._quick_actions(), False, False, 0)

        # ---- Recent activity ------------------------------------------
        self._inner.pack_start(_section_title("Recent activity"), False, False, 0)
        self._inner.pack_start(_section_description(
            "The last few things Mackes did on your behalf — useful when "
            "you're tracking down what changed."
        ), False, False, 0)
        self._inner.pack_start(self._recent_actions(), False, False, 0)

        self._inner.show_all()

    # ---- Stat tiles -------------------------------------------------------

    def _stat_row(self) -> Gtk.Widget:
        row = Gtk.Grid(column_spacing=8, row_spacing=8, column_homogeneous=True)
        row.set_margin_top(16)
        sh = service_health()
        ok_services = sum(1 for v in sh.values() if v == "ok")
        total_services = len(sh)
        try:
            from mackes.mesh_vpn import tailscale_status
            mesh_n = len(tailscale_status().get("peers", []) or [])
        except Exception:  # noqa: BLE001
            mesh_n = 0
        try:
            _preset, drift_items = active_preset_drift()
            drift_n = len(drift_items or [])
        except Exception:  # noqa: BLE001
            drift_n = 0
        sshd = sh.get("sshd", "missing")

        row.attach(_stat_tile("Mesh peers",  str(mesh_n),
                              "16 max · cap from spec", accent=True), 0, 0, 1, 1)
        row.attach(_stat_tile("Services",    f"{ok_services} / {total_services}",
                              "ok / total"), 1, 0, 1, 1)
        row.attach(_stat_tile("sshd",        "running" if sshd == "ok" else "down",
                              "see Services"), 2, 0, 1, 1)
        row.attach(_stat_tile("Drift",       str(drift_n),
                              "items differ from preset"), 3, 0, 1, 1)
        return row

    # ---- Service health grid ---------------------------------------------

    def _service_grid(self) -> Gtk.Widget:
        grid = Gtk.Grid(column_spacing=8, row_spacing=8, column_homogeneous=True)
        services = service_health()
        items = list(services.items())
        for i, (name, status) in enumerate(items):
            cell = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
            cell.get_style_context().add_class("mackes-stat-tile")
            cell.set_margin_top(0); cell.set_margin_bottom(0)
            cell.set_size_request(-1, 56)
            name_lbl = Gtk.Label(label=name)
            name_lbl.set_xalign(0)
            cell.pack_start(name_lbl, True, True, 0)
            cell.pack_end(_tag(status, _STATUS_TAG_KIND.get(status, "neutral")),
                          False, False, 0)
            grid.attach(cell, i % 4, i // 4, 1, 1)
        return grid

    # ---- Drift card -------------------------------------------------------

    def _drift_card(self) -> Optional[Gtk.Widget]:
        if not self.state.active_preset:
            return None
        try:
            preset, items = active_preset_drift()
        except Exception:  # noqa: BLE001
            return None
        if preset is None or not items:
            return None
        body_lines = [
            f"  • {it.section}.{it.field}: preset={it.expected!r}  live={it.actual!r}"
            for it in items[:6]
        ]
        if len(items) > 6:
            body_lines.append(f"  …and {len(items) - 6} more")
        body = "\n".join(body_lines)
        wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        wrap.set_margin_top(16)
        wrap.pack_start(_notification(
            "warning",
            f"{len(items)} item(s) drifted from preset \"{preset.display_name}\"",
            body,
            actions=[
                ("Open Maintain → Reset", lambda: self.navigate("maintain")),
                ("Snapshot first", self._on_snapshot),
            ],
        ), False, False, 0)
        return wrap

    # ---- Hardware grid ---------------------------------------------------

    def _hardware_grid(self) -> Gtk.Widget:
        grid = Gtk.Grid(column_spacing=8, row_spacing=8, column_homogeneous=True)
        info = hardware_summary()
        items = [
            ("Hostname", info.get("hostname", "—"), ""),
            ("OS",       info.get("os", "—"),       "version"),
            ("CPU",      info.get("cpu", "—"),      "cores"),
            ("RAM",      info.get("ram", "—"),      "installed"),
        ]
        for i, (k, v, foot) in enumerate(items):
            grid.attach(_stat_tile(k, str(v), foot), i % 4, i // 4, 1, 1)
        return grid

    # ---- Quick actions ----------------------------------------------------

    def _quick_actions(self) -> Gtk.Widget:
        wrap = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        wrap.set_margin_top(0)
        actions: List[Tuple[str, Callable[[], None]]] = [
            ("Appearance",     lambda: self.navigate("look_and_feel")),
            ("Display",        lambda: self.navigate("devices")),
            ("Network",        lambda: self.navigate("wifi")),
            ("Create snapshot", self._on_snapshot),
            ("Health check",   lambda: self.navigate("maintain")),
            ("Open log",       lambda: self.navigate("maintain")),
        ]
        for label, fn in actions:
            btn = Gtk.Button(label=label)
            btn.get_style_context().add_class("cds-button-tertiary")
            btn.set_size_request(-1, 40)
            btn.connect("clicked", lambda _b, f=fn: f())
            wrap.pack_start(btn, True, True, 0)
        return wrap

    def _on_snapshot(self) -> None:
        try:
            create_snapshot("dashboard-quick-snapshot",
                            source_preset=self.state.active_preset)
        except Exception:  # noqa: BLE001
            pass
        self._render()

    # ---- Recent activity -------------------------------------------------

    def _recent_actions(self) -> Gtk.Widget:
        wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        wrap.get_style_context().add_class("mackes-code")
        wrap.set_margin_top(0)

        log = LOG_DIR / "mackes.log"
        lines: List[str] = []
        if log.exists():
            try:
                lines = log.read_text(encoding="utf-8", errors="ignore").splitlines()[-8:]
            except OSError:
                pass
        if not lines:
            lines = ["(no activity yet)"]
        for line in lines:
            lbl = Gtk.Label(label=line)
            lbl.set_xalign(0); lbl.set_line_wrap(False)
            lbl.set_max_width_chars(140); lbl.set_ellipsize(__import__("gi").repository.Pango.EllipsizeMode.END)
            wrap.pack_start(lbl, False, False, 0)
        return wrap
