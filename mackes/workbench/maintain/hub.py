"""Maintain hub — Carbon 12-card grid (v1.1.1).

Mirrors docs/design/v1.1.0-carbon-refresh/project/panels-b.jsx::MaintainPanel.

Renders a 12-tile grid. Each tile is a Carbon-styled card (.mackes-app-card
style — reused, since it gives the right "tile with icon, title, body, foot"
look). Clicking a tile pushes the corresponding sub-panel into the parent
Gtk.Stack via the on_open callback. A "← Back to Maintain" link returns to
the hub.
"""
from __future__ import annotations

from typing import Callable, List, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.state import MackesState


def _page_title(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text); lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-page-title")
    return lab


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text); lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


def _breadcrumb() -> Gtk.Widget:
    bc = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
    bc.get_style_context().add_class("mackes-breadcrumb")
    for i, p in enumerate(("Mackes Shell", "Maintain")):
        lab = Gtk.Label(label=p); lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != 1:
            sep = Gtk.Label(label="/"); sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _tag(text: str, kind: str = "neutral") -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.get_style_context().add_class("mackes-tag")
    lab.get_style_context().add_class(kind)
    return lab


# Card descriptors. icon is a freedesktop icon name; meta/tag color let
# specific cards stand out (warn for drift, error for uninstall).
_CARDS = [
    ("snapshots",    "Snapshots",       "document-revert-symbolic",
     "Capture and restore xfconf/panel/theme state.", ""),
    ("drift",        "Drift",           "dialog-warning-symbolic",
     "Items diverging from the active preset.", "warn"),
    ("update",       "System update",   "system-software-update-symbolic",
     "dnf upgrade and Mackes self-update.", ""),
    ("fonts",        "Fonts",           "preferences-desktop-font-symbolic",
     "Install IBM Plex, JetBrains Mono, Nerd Fonts.", ""),
    ("power",        "Power profiles",  "weather-clear-symbolic",
     "Balanced / Performance / Power-saver.", ""),
    ("resources",    "Resources",       "utilities-system-monitor-symbolic",
     "CPU, RAM, GPU, IO live snapshot.", ""),
    ("health",       "Health check",    "emblem-ok-symbolic",
     "11 checks: services, mounts, mesh, RPM signature.", ""),
    ("deps",         "Dependencies",    "preferences-system-symbolic",
     "Verify Mackes RPM provides + recommends.", ""),
    ("logs",         "Logs",            "text-x-generic-symbolic",
     "mackes.log, journalctl filtered.", ""),
    ("repair",       "Repair",          "preferences-system-symbolic",
     "Re-bootstrap xfce4-panel from preset.", ""),
    ("reset",        "Reset to preset", "view-refresh-symbolic",
     "Wipe back to chosen preset's declared state.", ""),
    ("uninstall",    "Uninstall",       "user-trash-symbolic",
     "Complete removal + final snapshot tarball.", "danger"),
    ("debloat",      "Debloat levels",  "edit-clear-all-symbolic",
     "5 cumulative tiers of XFCE-desktop slimming.", "warn"),
]


class MaintainHub(Gtk.Box):
    """The hub view itself — just the 12-tile grid."""

    def __init__(self,
                 on_open: Callable[[str], None],
                 *, state: Optional[MackesState] = None) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._on_open = on_open
        self._state = state
        self._build()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Maintain"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Tools for keeping your machine healthy: back up your "
            "settings, install updates, see what's running, and undo "
            "anything that goes wrong."
        ), False, False, 0)

        # 12-tile grid (3 cols)
        grid = Gtk.Grid(column_spacing=12, row_spacing=12, column_homogeneous=True)
        grid.set_margin_top(16)
        for i, (key, title, icon, desc, kind) in enumerate(_CARDS):
            grid.attach(self._make_card(key, title, icon, desc, kind),
                        i % 3, i // 3, 1, 1)
        outer.pack_start(grid, False, False, 0)

        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    def _make_card(self, key: str, title: str, icon: str,
                   desc: str, kind: str) -> Gtk.Widget:
        # Make the whole card clickable via Gtk.Button (no relief)
        btn = Gtk.Button()
        btn.set_relief(Gtk.ReliefStyle.NONE)
        btn.connect("clicked", lambda *_: self._on_open(key))

        card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        card.get_style_context().add_class("mackes-app-card")
        card.set_size_request(-1, 140)

        # Top: icon + tag
        top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        ico = Gtk.Image.new_from_icon_name(icon, Gtk.IconSize.LARGE_TOOLBAR)
        ctx = ico.get_style_context()
        ctx.add_class("mackes-dot")
        if kind == "warn":
            ctx.add_class("warn")
        elif kind == "danger":
            ctx.add_class("fail")
        else:
            ctx.add_class("accent")
        top.pack_start(ico, False, False, 0)
        if kind == "warn":
            top.pack_end(_tag("attention", "warning"), False, False, 0)
        elif kind == "danger":
            top.pack_end(_tag("destructive", "error"), False, False, 0)
        card.pack_start(top, False, False, 0)

        # Title
        t = Gtk.Label(label=title); t.set_xalign(0)
        t.get_style_context().add_class("mackes-app-name")
        card.pack_start(t, False, False, 0)

        # Description
        d = Gtk.Label(label=desc); d.set_xalign(0); d.set_line_wrap(True)
        d.set_max_width_chars(40)
        d.get_style_context().add_class("mackes-app-desc")
        card.pack_start(d, True, True, 0)

        btn.add(card)
        return btn
