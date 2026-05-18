"""System → Displays — multi-monitor arrangement panel (Carbon refresh).

Top-down layout (mirrors `mesh_ssh.MeshSshPanel`):

  Page title + subtitle + breadcrumb
  Section: Layout          — DrawingArea canvas of monitor rectangles
                              the user can drag to reposition. Edge-snap
                              to other monitors and to the (0,0) origin.
  Section: Per-monitor     — Accordion-style expanders, one per output:
                              active toggle, primary radio, resolution,
                              refresh-rate, scale, rotation, wallpaper.
  Section: Profiles        — named-layout combo + Save / Load / Delete.
  Section: Login screen    — LightDM greeter active-monitor (stretch,
                              wired via AdminSession + mackes.displays).
  Apply button             — commits the staged layout to xfconf and
                              opens a 15-second "Keep this layout?"
                              countdown dialog before reverting.

All writes route through `mackes.displays` so the same primitives back
the wizard/CLI surface. The panel keeps a `_staged` mirror of the
xfconf state — every UI change updates `_staged`; only "Apply" pushes
the staged dict to `apply_layout()`.
"""
from __future__ import annotations

import copy
import os
import subprocess
from pathlib import Path
from typing import Optional

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

from mackes import displays as ds
from mackes.carbon import (
    Button, ButtonKind, Tile, Notification, NotificationKind,
)
from mackes.logging import log_action


# ---------------------------------------------------------------------------
# Carbon helpers (mirror mesh_ssh.py — kept local so the file is self-contained)
# ---------------------------------------------------------------------------


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
        lab = Gtk.Label(label=p)
        lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != len(parts) - 1:
            sep = Gtk.Label(label="/")
            sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _section_title(text: str, *, meta: str = "") -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    row.set_margin_top(28); row.set_margin_bottom(8)
    t = Gtk.Label(label=text)
    t.set_xalign(0)
    t.get_style_context().add_class("mackes-section-title")
    row.pack_start(t, True, True, 0)
    if meta:
        m = Gtk.Label(label=meta)
        m.set_xalign(1)
        m.get_style_context().add_class("mackes-section-meta")
        row.pack_end(m, False, False, 0)
    return row


# ---------------------------------------------------------------------------
# Drag-to-position canvas
# ---------------------------------------------------------------------------


# Snap threshold in *virtual* (canvas) pixels. The canvas maps the
# physical output coordinate space onto a fixed pixel canvas; the
# snap threshold is converted back to physical pixels at drag time
# so the snap feel doesn't change as outputs are added/removed.
_SNAP_PHYSICAL_PX = 80
_CANVAS_PAD = 24
_CANVAS_W = 720
_CANVAS_H = 280


class LayoutCanvas(Gtk.DrawingArea):
    """Visual canvas — one rectangle per output, click+drag to reposition.

    The canvas is the read-write view of `panel._staged`. Updates from
    expanders (resize / activate / deactivate) push state in via
    `refresh()`. Drag releases push out via `on_position_change(name, x, y)`.
    """

    def __init__(self, panel: "DisplaysPanel") -> None:
        super().__init__()
        self._panel = panel
        self.set_size_request(_CANVAS_W, _CANVAS_H)
        self.add_events(
            Gdk.EventMask.BUTTON_PRESS_MASK
            | Gdk.EventMask.BUTTON_RELEASE_MASK
            | Gdk.EventMask.POINTER_MOTION_MASK
        )
        self.connect("draw", self._on_draw)
        self.connect("button-press-event",   self._on_press)
        self.connect("button-release-event", self._on_release)
        self.connect("motion-notify-event",  self._on_motion)
        self._dragging: Optional[str] = None
        self._drag_offset: tuple[int, int] = (0, 0)

    # ---- coordinate mapping ----------------------------------------------

    def _bounds(self) -> tuple[int, int, int, int]:
        """(min_x, min_y, max_x, max_y) of all *active* outputs in physical px."""
        outs = [o for o in self._panel._staged.values()
                if o.get("active") and o.get("resolution", (0, 0))[0] > 0]
        if not outs:
            return (0, 0, 1920, 1080)
        # Pad the inactive rectangle row to the right so off-monitors
        # have a place to be parked visually.
        inactive_extra_w = 0
        for o in self._panel._staged.values():
            if not o.get("active"):
                inactive_extra_w += max(o.get("resolution", (0, 0))[0], 1280)
        min_x = min(o["position"][0] for o in outs)
        min_y = min(o["position"][1] for o in outs)
        max_x = max(o["position"][0] + o["resolution"][0] for o in outs)
        max_x += inactive_extra_w
        max_y = max(o["position"][1] + o["resolution"][1] for o in outs)
        return (min_x, min_y, max_x, max_y)

    def _scale(self) -> float:
        min_x, min_y, max_x, max_y = self._bounds()
        span_w = max(1, max_x - min_x)
        span_h = max(1, max_y - min_y)
        sx = (_CANVAS_W - 2 * _CANVAS_PAD) / span_w
        sy = (_CANVAS_H - 2 * _CANVAS_PAD) / span_h
        return min(sx, sy)

    def _to_canvas(self, px: int, py: int) -> tuple[float, float]:
        min_x, min_y, _, _ = self._bounds()
        s = self._scale()
        return (_CANVAS_PAD + (px - min_x) * s,
                _CANVAS_PAD + (py - min_y) * s)

    def _to_physical(self, cx: float, cy: float) -> tuple[int, int]:
        min_x, min_y, _, _ = self._bounds()
        s = self._scale()
        if s == 0:
            return (0, 0)
        return (int((cx - _CANVAS_PAD) / s + min_x),
                int((cy - _CANVAS_PAD) / s + min_y))

    # ---- drag handling ---------------------------------------------------

    def _hit_test(self, cx: float, cy: float) -> Optional[str]:
        """Topmost output rectangle under (cx, cy)."""
        hit: Optional[str] = None
        for name, props in self._panel._staged.items():
            if not props.get("active"):
                # inactive rectangles are still drawn (parked); hit-test them too
                pass
            w, h = props.get("resolution", (0, 0))
            if w == 0 or h == 0:
                w, h = 1280, 720
            px, py = props.get("position", (0, 0))
            x1, y1 = self._to_canvas(px, py)
            x2, y2 = self._to_canvas(px + w, py + h)
            if x1 <= cx <= x2 and y1 <= cy <= y2:
                hit = name  # last one wins → topmost
        return hit

    def _on_press(self, _w, event):
        if event.button != 1:
            return False
        name = self._hit_test(event.x, event.y)
        if name is None:
            return False
        self._dragging = name
        props = self._panel._staged[name]
        cx, cy = self._to_canvas(*props.get("position", (0, 0)))
        self._drag_offset = (event.x - cx, event.y - cy)
        return True

    def _on_motion(self, _w, event):
        if self._dragging is None:
            return False
        cx = event.x - self._drag_offset[0]
        cy = event.y - self._drag_offset[1]
        new_px, new_py = self._to_physical(cx, cy)
        new_px, new_py = self._snap(self._dragging, new_px, new_py)
        self._panel._staged[self._dragging]["position"] = (new_px, new_py)
        self.queue_draw()
        return True

    def _on_release(self, _w, event):
        if self._dragging is None:
            return False
        name = self._dragging
        self._dragging = None
        # Refresh the position spinbuttons in the per-output expander.
        self._panel._on_position_changed(name)
        return True

    def _snap(self, dragging_name: str, x: int, y: int) -> tuple[int, int]:
        """Snap a dragged output's top-left to neighboring edges + origin.

        Snap targets:
          • The (0,0) origin (so the user can pin the primary back home).
          • Each other active output's left/right/top/bottom edges so
            the dragged rectangle clicks into a butted neighbor.
        """
        threshold = _SNAP_PHYSICAL_PX
        dragged = self._panel._staged.get(dragging_name, {})
        w, h = dragged.get("resolution", (1280, 720))
        if w == 0:
            w, h = 1280, 720

        # Snap to origin
        if abs(x) < threshold:
            x = 0
        if abs(y) < threshold:
            y = 0

        for name, props in self._panel._staged.items():
            if name == dragging_name or not props.get("active"):
                continue
            ow, oh = props.get("resolution", (0, 0))
            if ow == 0:
                continue
            ox, oy = props.get("position", (0, 0))
            # Vertical edges
            for tx in (ox - w, ox + ow, ox, ox + ow - w):
                if abs(x - tx) < threshold:
                    x = tx
                    break
            # Horizontal edges
            for ty in (oy - h, oy + oh, oy, oy + oh - h):
                if abs(y - ty) < threshold:
                    y = ty
                    break
        return x, y

    # ---- draw ------------------------------------------------------------

    def _on_draw(self, _w, cr) -> bool:
        # Background
        cr.set_source_rgb(0.094, 0.094, 0.094)  # Gray 100
        cr.rectangle(0, 0, _CANVAS_W, _CANVAS_H)
        cr.fill()

        # Subtle grid
        cr.set_source_rgba(1, 1, 1, 0.04)
        cr.set_line_width(1)
        for gx in range(0, _CANVAS_W, 40):
            cr.move_to(gx, 0); cr.line_to(gx, _CANVAS_H)
        for gy in range(0, _CANVAS_H, 40):
            cr.move_to(0, gy); cr.line_to(_CANVAS_W, gy)
        cr.stroke()

        # Rectangles
        for name, props in self._panel._staged.items():
            w, h = props.get("resolution", (0, 0))
            if w == 0 or h == 0:
                w, h = 1280, 720
            px, py = props.get("position", (0, 0))
            x1, y1 = self._to_canvas(px, py)
            x2, y2 = self._to_canvas(px + w, py + h)
            rw = max(40, x2 - x1)
            rh = max(28, y2 - y1)

            active = props.get("active", False)
            primary = props.get("primary", False)
            dragging = (name == self._dragging)

            # Fill
            if active and primary:
                cr.set_source_rgba(0.945, 0.522, 0.239, 0.42)   # accent (mackes orange)
            elif active:
                cr.set_source_rgba(0.20, 0.20, 0.22, 0.95)
            else:
                cr.set_source_rgba(0.12, 0.12, 0.14, 0.85)
            cr.rectangle(x1, y1, rw, rh)
            cr.fill()

            # Border
            if dragging:
                cr.set_source_rgb(1.0, 1.0, 1.0)
                cr.set_line_width(2)
            elif primary and active:
                cr.set_source_rgb(0.945, 0.522, 0.239)
                cr.set_line_width(2)
            elif active:
                cr.set_source_rgba(1, 1, 1, 0.30)
                cr.set_line_width(1)
            else:
                cr.set_source_rgba(1, 1, 1, 0.15)
                cr.set_line_width(1)
            cr.rectangle(x1, y1, rw, rh)
            cr.stroke()

            # Label
            label_lines = [
                name + ("  ★" if primary and active else ""),
                f"{w}×{h}" if active else "off",
            ]
            friendly = props.get("friendly_name", "")
            if friendly:
                label_lines.insert(1, friendly[:28])
            cr.set_source_rgba(1, 1, 1, 0.92 if active else 0.55)
            cr.select_font_face("Red Hat Text",
                                0, 0)  # normal / normal
            cr.set_font_size(12)
            for i, line in enumerate(label_lines):
                cr.move_to(x1 + 8, y1 + 18 + i * 14)
                cr.show_text(line)
        return False


# ---------------------------------------------------------------------------
# Main panel
# ---------------------------------------------------------------------------


class DisplaysPanel(Gtk.Box):
    """System → Displays Carbon panel."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._outputs: list[ds.Output] = []
        self._staged: dict[str, dict] = {}
        self._original: dict[str, dict] = {}
        self._expander_widgets: dict[str, dict] = {}
        self._refreshing = False
        self._build()
        self._refresh_from_xfconf()

    # ---- build -----------------------------------------------------------

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)
        self._outer = outer

        outer.pack_start(_breadcrumb(["Mackes Shell", "System", "Displays"]),
                         False, False, 0)
        outer.pack_start(_page_title("Displays"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Arrange your monitors, set a primary, and switch resolutions "
            "or scaling per screen. Changes preview live; you'll get a "
            "15-second window to keep or revert."
        ), False, False, 0)

        # Wayland / xfconf availability notification
        if ds.is_wayland():
            outer.pack_start(Notification(
                "Wayland session — XFCE displays config is X11-only. "
                "Switch to the X11 session to use this panel.",
                kind=NotificationKind.WARNING, dismissible=False,
            ), False, False, 0)
        elif not ds._have_xfconf():
            outer.pack_start(Notification(
                "xfconf-query not found. Install the `xfconf` package "
                "to manage displays from Mackes.",
                kind=NotificationKind.ERROR, dismissible=False,
            ), False, False, 0)

        # ---- Layout canvas ----
        outer.pack_start(_section_title("Layout",
                                       meta="drag to arrange"),
                         False, False, 0)
        canvas_tile = Tile()
        self._canvas = LayoutCanvas(self)
        canvas_tile.pack(self._canvas)
        canvas_help = Gtk.Label()
        canvas_help.set_markup(
            "<small>Click and drag a monitor to position it. Edges snap "
            "to neighboring monitors and to the (0,0) origin.</small>"
        )
        canvas_help.set_xalign(0); canvas_help.set_margin_top(8)
        canvas_help.get_style_context().add_class("mackes-page-subtitle")
        canvas_tile.pack(canvas_help)
        outer.pack_start(canvas_tile, False, False, 0)

        # ---- Per-monitor settings ----
        outer.pack_start(_section_title("Per-monitor settings"),
                         False, False, 0)
        self._per_monitor_host = Gtk.Box(orientation=Gtk.Orientation.VERTICAL,
                                          spacing=8)
        outer.pack_start(self._per_monitor_host, False, False, 0)

        # ---- Profiles ----
        outer.pack_start(_section_title("Profiles",
                                       meta="named layouts"),
                         False, False, 0)
        profiles_tile = Tile()
        prof_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self._profile_combo = Gtk.ComboBoxText()
        self._profile_combo.set_size_request(220, -1)
        prof_row.pack_start(self._profile_combo, False, False, 0)
        prof_row.pack_start(Button("Load",
                                    kind=ButtonKind.TERTIARY,
                                    icon_name="document-open-symbolic",
                                    on_click=self._on_load_profile),
                            False, False, 0)
        prof_row.pack_start(Button("Save as…",
                                    kind=ButtonKind.TERTIARY,
                                    icon_name="document-save-as-symbolic",
                                    on_click=self._on_save_profile),
                            False, False, 0)
        prof_row.pack_start(Button("Delete",
                                    kind=ButtonKind.GHOST,
                                    icon_name="edit-delete-symbolic",
                                    on_click=self._on_delete_profile),
                            False, False, 0)
        profiles_tile.pack(prof_row)
        outer.pack_start(profiles_tile, False, False, 0)

        # ---- Login screen (LightDM greeter active-monitor) ----
        outer.pack_start(_section_title("Login screen",
                                       meta="LightDM greeter"),
                         False, False, 0)
        login_tile = Tile()
        login_help = Gtk.Label()
        login_help.set_markup(
            "<small>Choose which monitor the LightDM login screen should "
            "appear on. \"All monitors\" mirrors the greeter across every "
            "active output (the default).</small>"
        )
        login_help.set_xalign(0); login_help.set_line_wrap(True)
        login_help.get_style_context().add_class("mackes-page-subtitle")
        login_tile.pack(login_help)

        self._greeter_all_radio = Gtk.RadioButton.new_with_label_from_widget(
            None, "Show on all monitors")
        self._greeter_primary_radio = Gtk.RadioButton.new_with_label_from_widget(
            self._greeter_all_radio, "Show on primary only")
        self._greeter_specific_radio = Gtk.RadioButton.new_with_label_from_widget(
            self._greeter_all_radio, "Show on a specific monitor:")
        self._greeter_specific_combo = Gtk.ComboBoxText()
        self._greeter_specific_combo.set_size_request(200, -1)
        login_tile.pack(self._greeter_all_radio)
        login_tile.pack(self._greeter_primary_radio)
        spec_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        spec_row.pack_start(self._greeter_specific_radio, False, False, 0)
        spec_row.pack_start(self._greeter_specific_combo, False, False, 0)
        login_tile.pack(spec_row)
        greeter_apply = Button("Apply login-screen setting",
                                kind=ButtonKind.SECONDARY,
                                icon_name="document-save-symbolic",
                                on_click=self._on_apply_greeter)
        login_tile.pack(greeter_apply)
        self._greeter_status = Gtk.Label(label="")
        self._greeter_status.set_xalign(0)
        self._greeter_status.get_style_context().add_class("mackes-page-subtitle")
        login_tile.pack(self._greeter_status)
        outer.pack_start(login_tile, False, False, 0)

        # ---- Apply / Revert bar (sticky at bottom of the page) ----
        outer.pack_start(_section_title("Commit"), False, False, 0)
        commit_tile = Tile()
        commit_help = Gtk.Label()
        commit_help.set_markup(
            "<small>Apply pushes the staged layout to xfconf. A 15-second "
            "countdown lets you revert if something looks wrong — useful "
            "when activating a monitor that may not be receiving signal.</small>"
        )
        commit_help.set_xalign(0); commit_help.set_line_wrap(True)
        commit_help.get_style_context().add_class("mackes-page-subtitle")
        commit_tile.pack(commit_help)
        commit_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        commit_bar.pack_start(Button("Apply",
                                      kind=ButtonKind.PRIMARY,
                                      icon_name="emblem-ok-symbolic",
                                      on_click=self._on_apply),
                              False, False, 0)
        commit_bar.pack_start(Button("Revert",
                                      kind=ButtonKind.GHOST,
                                      icon_name="view-refresh-symbolic",
                                      on_click=self._on_revert),
                              False, False, 0)
        commit_bar.pack_start(Button("Refresh from xfconf",
                                      kind=ButtonKind.GHOST,
                                      icon_name="view-refresh-symbolic",
                                      on_click=self._refresh_from_xfconf),
                              False, False, 0)
        commit_tile.pack(commit_bar)
        outer.pack_start(commit_tile, False, False, 0)

        # Scroll the whole thing
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- state -----------------------------------------------------------

    def _refresh_from_xfconf(self) -> None:
        """Pull live xfconf state into `_staged` + rebuild the expanders."""
        self._refreshing = True
        try:
            self._outputs = ds.list_outputs()
            self._staged = {}
            for o in self._outputs:
                self._staged[o.name] = {
                    "name":          o.name,
                    "friendly_name": o.friendly_name,
                    "active":        o.active,
                    "primary":       o.primary,
                    "resolution":    o.resolution,
                    "position":      o.position,
                    "scale":         o.scale,
                    "rotation":      o.rotation,
                    "refresh_rate":  o.refresh_rate,
                    "supported_modes": o.supported_modes,
                }
            self._original = copy.deepcopy(self._staged)
            self._rebuild_expanders()
            self._refresh_profiles_combo()
            self._refresh_greeter_section()
            self._canvas.queue_draw()
        finally:
            self._refreshing = False

    # ---- per-monitor expanders ------------------------------------------

    def _rebuild_expanders(self) -> None:
        for child in list(self._per_monitor_host.get_children()):
            self._per_monitor_host.remove(child)
        self._expander_widgets.clear()

        if not self._staged:
            empty = Notification(
                "No outputs found. Check that you're in an X11 session "
                "and that xfconf has populated the displays channel.",
                kind=NotificationKind.WARNING, dismissible=False)
            self._per_monitor_host.pack_start(empty, False, False, 0)
            return

        for name, props in self._staged.items():
            exp = Gtk.Expander()
            label = self._expander_label(name, props)
            exp.set_label_widget(label)
            exp.set_expanded(props.get("active", False))
            body = self._build_expander_body(name)
            exp.add(body)
            self._per_monitor_host.pack_start(exp, False, False, 0)
            self._expander_widgets[name]["_expander_label"] = label
            self._expander_widgets[name]["_expander"] = exp

        self._per_monitor_host.show_all()

    def _expander_label(self, name: str, props: dict) -> Gtk.Widget:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        row.set_margin_top(2); row.set_margin_bottom(2)
        dot = Gtk.Label(label=("●" if props.get("active") else "○"))
        dot.get_style_context().add_class(
            "mackes-dot-active" if props.get("active") else "mackes-dot")
        row.pack_start(dot, False, False, 0)

        title = Gtk.Label()
        friendly = props.get("friendly_name") or ""
        markup = f"<b>{GLib.markup_escape_text(name)}</b>"
        if friendly:
            markup += f"  <span alpha='70%'>{GLib.markup_escape_text(friendly)}</span>"
        if props.get("primary"):
            markup += "  <span weight='600'>★ primary</span>"
        if props.get("active"):
            res = props.get("resolution", (0, 0))
            if res[0] > 0:
                markup += f"  <span alpha='80%'>{res[0]}×{res[1]}</span>"
        title.set_markup(markup)
        title.set_xalign(0)
        row.pack_start(title, True, True, 0)
        return row

    def _build_expander_body(self, name: str) -> Gtk.Widget:
        props = self._staged[name]
        widgets: dict = {}
        self._expander_widgets[name] = widgets

        grid = Gtk.Grid()
        grid.set_column_spacing(16); grid.set_row_spacing(8)
        grid.set_margin_top(12); grid.set_margin_bottom(12)
        grid.set_margin_start(16); grid.set_margin_end(16)

        # 1. Active toggle
        active_lbl = Gtk.Label(label="Active"); active_lbl.set_xalign(0)
        active_sw = Gtk.Switch()
        active_sw.set_active(props.get("active", False))
        active_sw.set_halign(Gtk.Align.START)
        active_sw.connect("notify::active",
                          lambda s, *_: self._on_active_toggled(name, s.get_active()))
        widgets["active"] = active_sw
        grid.attach(active_lbl, 0, 0, 1, 1)
        grid.attach(active_sw, 1, 0, 1, 1)

        # 2. Primary
        primary_lbl = Gtk.Label(label="Primary"); primary_lbl.set_xalign(0)
        primary_btn = Gtk.CheckButton(label="Set as primary monitor")
        primary_btn.set_active(props.get("primary", False))
        primary_btn.connect("toggled",
                            lambda b: self._on_primary_toggled(name, b.get_active()))
        widgets["primary"] = primary_btn
        grid.attach(primary_lbl, 0, 1, 1, 1)
        grid.attach(primary_btn, 1, 1, 1, 1)

        # 3. Resolution combo
        res_lbl = Gtk.Label(label="Resolution"); res_lbl.set_xalign(0)
        res_combo = Gtk.ComboBoxText()
        seen = set()
        for m in props.get("supported_modes", []):
            key = (m.width, m.height)
            if key in seen:
                continue
            seen.add(key)
            res_combo.append(f"{m.width}x{m.height}",
                             f"{m.width} × {m.height}")
        cur_w, cur_h = props.get("resolution", (0, 0))
        if cur_w and cur_h:
            res_combo.set_active_id(f"{cur_w}x{cur_h}")
        if res_combo.get_active_id() is None and res_combo.get_model().iter_n_children(None) > 0:
            res_combo.set_active(0)
        res_combo.connect("changed",
                          lambda c: self._on_resolution_changed(name, c.get_active_id()))
        widgets["resolution"] = res_combo
        grid.attach(res_lbl, 0, 2, 1, 1)
        grid.attach(res_combo, 1, 2, 1, 1)

        # 4. Refresh rate combo
        rr_lbl = Gtk.Label(label="Refresh"); rr_lbl.set_xalign(0)
        rr_combo = Gtk.ComboBoxText()
        widgets["refresh"] = rr_combo
        self._populate_refresh_combo(name)
        rr_combo.connect("changed",
                         lambda c: self._on_refresh_changed(name, c.get_active_text()))
        grid.attach(rr_lbl, 0, 3, 1, 1)
        grid.attach(rr_combo, 1, 3, 1, 1)

        # 5. Scale
        scale_lbl = Gtk.Label(label="Scale"); scale_lbl.set_xalign(0)
        scale_combo = Gtk.ComboBoxText()
        for s in ds.SCALE_VALUES:
            scale_combo.append(f"{s:.2f}", f"{s:g}×")
        cur_scale = props.get("scale", 1.0) or 1.0
        scale_combo.set_active_id(f"{float(cur_scale):.2f}")
        if scale_combo.get_active_id() is None:
            scale_combo.set_active(0)
        scale_combo.connect("changed",
                            lambda c: self._on_scale_changed(name, c.get_active_id()))
        widgets["scale"] = scale_combo
        grid.attach(scale_lbl, 0, 4, 1, 1)
        grid.attach(scale_combo, 1, 4, 1, 1)

        # 6. Rotation
        rot_lbl = Gtk.Label(label="Rotation"); rot_lbl.set_xalign(0)
        rot_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        rot_buttons: dict[int, Gtk.RadioButton] = {}
        first = None
        cur_rot = int(props.get("rotation", 0)) or 0
        for ang in ds.ROTATION_VALUES:
            btn = Gtk.RadioButton.new_with_label_from_widget(first, f"{ang}°")
            if first is None:
                first = btn
            if ang == cur_rot:
                btn.set_active(True)
            btn.connect("toggled", self._on_rotation_toggled, name, ang)
            rot_buttons[ang] = btn
            rot_row.pack_start(btn, False, False, 0)
        widgets["rotation"] = rot_buttons
        grid.attach(rot_lbl, 0, 5, 1, 1)
        grid.attach(rot_row, 1, 5, 1, 1)

        # 7. Position spinbuttons
        pos_lbl = Gtk.Label(label="Position (x, y)"); pos_lbl.set_xalign(0)
        pos_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        px, py = props.get("position", (0, 0))
        x_spin = Gtk.SpinButton.new_with_range(-32768, 32768, 16)
        y_spin = Gtk.SpinButton.new_with_range(-32768, 32768, 16)
        x_spin.set_value(px); y_spin.set_value(py)
        x_spin.connect("value-changed",
                       lambda s: self._on_position_spin(name, "x", int(s.get_value())))
        y_spin.connect("value-changed",
                       lambda s: self._on_position_spin(name, "y", int(s.get_value())))
        pos_row.pack_start(Gtk.Label(label="x"), False, False, 0)
        pos_row.pack_start(x_spin, False, False, 0)
        pos_row.pack_start(Gtk.Label(label="y"), False, False, 0)
        pos_row.pack_start(y_spin, False, False, 0)
        widgets["pos_x"] = x_spin; widgets["pos_y"] = y_spin
        grid.attach(pos_lbl, 0, 6, 1, 1)
        grid.attach(pos_row, 1, 6, 1, 1)

        # 8. Wallpaper picker (per monitor, per workspace 0)
        wp_lbl = Gtk.Label(label="Wallpaper"); wp_lbl.set_xalign(0)
        wp_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        current = ds.get_wallpaper(name)
        wp_btn = Gtk.FileChooserButton(
            title=f"Wallpaper for {name}",
            action=Gtk.FileChooserAction.OPEN)
        filt = Gtk.FileFilter(); filt.set_name("Images")
        for ext in ("png", "jpg", "jpeg", "webp", "bmp", "svg"):
            filt.add_pattern(f"*.{ext}")
        wp_btn.add_filter(filt)
        if current and current.is_file():
            try:
                wp_btn.set_filename(str(current))
            except Exception:  # noqa: BLE001
                pass
        wp_btn.connect("file-set",
                       lambda fc: self._on_wallpaper_set(name, fc.get_filename()))
        wp_clear = Button("Clear", kind=ButtonKind.GHOST,
                          on_click=lambda: self._on_wallpaper_set(name, None))
        wp_row.pack_start(wp_btn, True, True, 0)
        wp_row.pack_start(wp_clear, False, False, 0)
        widgets["wallpaper"] = wp_btn
        grid.attach(wp_lbl, 0, 7, 1, 1)
        grid.attach(wp_row, 1, 7, 1, 1)

        return grid

    def _populate_refresh_combo(self, name: str) -> None:
        combo: Gtk.ComboBoxText = self._expander_widgets[name]["refresh"]
        props = self._staged[name]
        w, h = props.get("resolution", (0, 0))
        combo.remove_all()
        rates: list[float] = []
        for m in props.get("supported_modes", []):
            if (m.width, m.height) == (w, h) and m.refresh_rate not in rates:
                rates.append(m.refresh_rate)
        if not rates:
            # Fall back to whatever refresh xfconf has so the user sees something.
            cur = float(props.get("refresh_rate", 60.0))
            rates = [cur] if cur > 0 else [60.0]
        rates.sort(reverse=True)
        for r in rates:
            combo.append(f"{r:.3f}", f"{r:.0f} Hz")
        cur_rate = float(props.get("refresh_rate", rates[0]))
        # Match by closest available rate.
        best = min(rates, key=lambda r: abs(r - cur_rate))
        combo.set_active_id(f"{best:.3f}")

    # ---- event handlers --------------------------------------------------

    def _on_active_toggled(self, name: str, active: bool) -> None:
        if self._refreshing:
            return
        self._staged[name]["active"] = active
        self._refresh_expander_label(name)
        self._canvas.queue_draw()

    def _on_primary_toggled(self, name: str, primary: bool) -> None:
        if self._refreshing:
            return
        if primary:
            # Mutually exclusive — only one primary
            for n in self._staged:
                self._staged[n]["primary"] = (n == name)
                btn = self._expander_widgets.get(n, {}).get("primary")
                if btn is not None and n != name:
                    self._refreshing = True
                    try: btn.set_active(False)
                    finally: self._refreshing = False
                self._refresh_expander_label(n)
        else:
            self._staged[name]["primary"] = False
            self._refresh_expander_label(name)
        self._canvas.queue_draw()

    def _on_resolution_changed(self, name: str, res_id: Optional[str]) -> None:
        if self._refreshing or not res_id:
            return
        try:
            w, h = (int(x) for x in res_id.split("x", 1))
        except ValueError:
            return
        self._staged[name]["resolution"] = (w, h)
        self._populate_refresh_combo(name)
        self._refresh_expander_label(name)
        self._canvas.queue_draw()

    def _on_refresh_changed(self, name: str, label: Optional[str]) -> None:
        if self._refreshing or not label:
            return
        combo = self._expander_widgets[name]["refresh"]
        rid = combo.get_active_id()
        if rid is None:
            return
        try:
            self._staged[name]["refresh_rate"] = float(rid)
        except ValueError:
            pass

    def _on_scale_changed(self, name: str, scale_id: Optional[str]) -> None:
        if self._refreshing or not scale_id:
            return
        try:
            self._staged[name]["scale"] = float(scale_id)
        except ValueError:
            pass

    def _on_rotation_toggled(self, btn: Gtk.RadioButton, name: str, ang: int) -> None:
        if self._refreshing or not btn.get_active():
            return
        self._staged[name]["rotation"] = int(ang)

    def _on_position_spin(self, name: str, axis: str, val: int) -> None:
        if self._refreshing:
            return
        x, y = self._staged[name].get("position", (0, 0))
        if axis == "x":
            self._staged[name]["position"] = (val, y)
        else:
            self._staged[name]["position"] = (x, val)
        self._canvas.queue_draw()

    def _on_position_changed(self, name: str) -> None:
        """Called by the canvas after a drag release — sync the spinners."""
        x, y = self._staged[name].get("position", (0, 0))
        self._refreshing = True
        try:
            self._expander_widgets[name]["pos_x"].set_value(x)
            self._expander_widgets[name]["pos_y"].set_value(y)
        finally:
            self._refreshing = False

    def _on_wallpaper_set(self, name: str, path: Optional[str]) -> None:
        if not path:
            return
        try:
            actions = ds.set_wallpaper(name, Path(path), workspace=-1)
            log_action(f"displays: wallpaper {name} → {path}")
            for line in actions:
                log_action(f"displays: {line}")
        except (FileNotFoundError, OSError) as e:
            self._toast(f"Could not set wallpaper: {e}", kind="error")

    def _refresh_expander_label(self, name: str) -> None:
        exp_meta = self._expander_widgets.get(name, {})
        exp = exp_meta.get("_expander")
        if exp is None:
            return
        new_label = self._expander_label(name, self._staged[name])
        exp.set_label_widget(new_label)
        new_label.show_all()
        exp_meta["_expander_label"] = new_label

    # ---- profiles --------------------------------------------------------

    def _refresh_profiles_combo(self) -> None:
        self._profile_combo.remove_all()
        profs = ds.list_profiles()
        active = ds.active_profile()
        for p in profs:
            self._profile_combo.append(p, p + ("  (active)" if p == active else ""))
        if active in profs:
            self._profile_combo.set_active_id(active)
        elif profs:
            self._profile_combo.set_active(0)

    def _on_load_profile(self) -> None:
        prof = self._profile_combo.get_active_id()
        if not prof:
            return
        try:
            ds.load_profile(prof)
            log_action(f"displays: loaded profile {prof}")
        except ValueError as e:
            self._toast(str(e), kind="error")
            return
        self._refresh_from_xfconf()
        self._toast(f"Loaded profile: {prof}", kind="success")

    def _on_save_profile(self) -> None:
        # Prompt for a profile name
        dlg = Gtk.Dialog(title="Save layout as profile",
                         transient_for=self.get_toplevel(),
                         flags=Gtk.DialogFlags.MODAL)
        dlg.add_buttons("Cancel", Gtk.ResponseType.CANCEL,
                        "Save", Gtk.ResponseType.OK)
        box = dlg.get_content_area()
        box.set_margin_top(16); box.set_margin_bottom(16)
        box.set_margin_start(16); box.set_margin_end(16)
        box.set_spacing(8)
        box.pack_start(Gtk.Label(label="Profile name:"), False, False, 0)
        entry = Gtk.Entry(); entry.set_placeholder_text("Office, Couch, Solo…")
        box.pack_start(entry, False, False, 0)
        dlg.show_all()
        resp = dlg.run()
        name = entry.get_text().strip()
        dlg.destroy()
        if resp != Gtk.ResponseType.OK or not name:
            return
        # First push the current staged layout to xfconf, then snapshot it
        # under the new profile name so the saved values match what the
        # user just configured.
        try:
            ds.apply_layout(self._staged_to_layout_dict())
            ds.save_profile(name)
            log_action(f"displays: saved profile {name}")
        except (ValueError, OSError) as e:
            self._toast(f"Save failed: {e}", kind="error")
            return
        self._refresh_profiles_combo()
        self._toast(f"Saved profile: {name}", kind="success")

    def _on_delete_profile(self) -> None:
        prof = self._profile_combo.get_active_id()
        if not prof or prof == "Default":
            self._toast("Cannot delete the Default profile.", kind="warning")
            return
        try:
            ds.delete_profile(prof)
            log_action(f"displays: deleted profile {prof}")
        except ValueError as e:
            self._toast(str(e), kind="error"); return
        self._refresh_profiles_combo()

    # ---- LightDM greeter -------------------------------------------------

    def _refresh_greeter_section(self) -> None:
        # Populate specific-monitor combo with current active outputs
        self._greeter_specific_combo.remove_all()
        for name, props in self._staged.items():
            if props.get("active"):
                label = name
                if props.get("friendly_name"):
                    label = f"{name} — {props['friendly_name']}"
                self._greeter_specific_combo.append(name, label)

        current = ds.lightdm_active_monitor()
        if current is None:
            self._greeter_all_radio.set_active(True)
            self._greeter_status.set_text(
                "Currently: shown on all monitors (LightDM default).")
        elif current.lower() == "primary":
            # `active-monitor = primary` isn't actually a LightDM directive,
            # but we model it as "show on the primary output's name" so the
            # UI maps cleanly. If the user sets this, we resolve it to the
            # actual primary name at write time.
            self._greeter_primary_radio.set_active(True)
            self._greeter_status.set_text("Currently: shown on the primary monitor.")
        else:
            self._greeter_specific_radio.set_active(True)
            self._greeter_specific_combo.set_active_id(current)
            self._greeter_status.set_text(f"Currently: pinned to {current}.")

    def _on_apply_greeter(self) -> None:
        value: Optional[str] = None
        if self._greeter_all_radio.get_active():
            value = None
        elif self._greeter_primary_radio.get_active():
            prim = next((n for n, p in self._staged.items() if p.get("primary")),
                        None)
            if not prim:
                self._toast("No primary monitor set.", kind="error"); return
            value = prim
        elif self._greeter_specific_radio.get_active():
            value = self._greeter_specific_combo.get_active_id()
            if not value:
                self._toast("Pick a monitor first.", kind="warning"); return

        rc, log = ds.set_lightdm_active_monitor(value)
        if rc == 0:
            log_action(f"displays: lightdm active-monitor → {value or '(unset)'}")
            self._toast("Login screen updated.", kind="success")
            self._refresh_greeter_section()
        else:
            self._toast(f"Could not update greeter config: {log}", kind="error")

    # ---- apply / revert --------------------------------------------------

    def _staged_to_layout_dict(self) -> dict[str, dict]:
        layout: dict[str, dict] = {}
        for name, props in self._staged.items():
            layout[name] = {
                "active":       props.get("active"),
                "primary":      props.get("primary"),
                "position":     props.get("position"),
                "scale":        props.get("scale"),
                "rotation":     props.get("rotation"),
                "resolution":   props.get("resolution"),
                "refresh_rate": props.get("refresh_rate"),
            }
        return layout

    def _on_apply(self) -> None:
        # Validate: must have at least one active output marked primary.
        active_outputs = [n for n, p in self._staged.items() if p.get("active")]
        if not active_outputs:
            self._toast("At least one monitor must be active.", kind="error")
            return
        has_primary = any(self._staged[n].get("primary") for n in active_outputs)
        if not has_primary:
            # Auto-promote the first active output to primary
            self._staged[active_outputs[0]]["primary"] = True

        layout = self._staged_to_layout_dict()
        previous = ds.capture_layout()
        try:
            actions = ds.apply_layout(layout)
            log_action(f"displays: applied layout ({len(actions)} keys)")
        except (ValueError, OSError) as e:
            self._toast(f"Apply failed: {e}", kind="error"); return

        # v2.2.0 — Conky reconciliation removed (drawer is panel-anchored,
        # follows whatever xfce4-panel is on; no per-monitor reconcile needed).
        self._toast("Layout applied — keep or revert?", kind="info")
        self._open_keep_revert_dialog(previous_layout=previous)

    def _on_revert(self) -> None:
        self._staged = copy.deepcopy(self._original)
        self._rebuild_expanders()
        self._canvas.queue_draw()

    def _open_keep_revert_dialog(self, *, previous_layout: dict[str, dict]) -> None:
        """15-second countdown dialog — standard X resolution-change pattern."""
        dlg = Gtk.Dialog(
            title="Keep this display layout?",
            transient_for=self.get_toplevel(),
            flags=Gtk.DialogFlags.MODAL,
        )
        dlg.add_buttons("Revert", Gtk.ResponseType.NO,
                        "Keep", Gtk.ResponseType.YES)
        dlg.set_default_response(Gtk.ResponseType.NO)
        area = dlg.get_content_area()
        area.set_margin_top(16); area.set_margin_bottom(16)
        area.set_margin_start(16); area.set_margin_end(16)
        area.set_spacing(8)
        msg = Gtk.Label()
        msg.set_xalign(0); msg.set_line_wrap(True)
        msg.set_markup(
            "<b>Reverting in <span foreground='#fa4d56'>15</span> seconds.</b>"
            "\n\nIf the new layout looks right, click <b>Keep</b>. "
            "Otherwise the previous layout will be restored automatically."
        )
        area.pack_start(msg, False, False, 0)
        dlg.show_all()

        remaining = {"s": 15}

        def tick() -> bool:
            remaining["s"] -= 1
            if remaining["s"] <= 0:
                if dlg.get_visible():
                    dlg.response(Gtk.ResponseType.NO)
                return False
            msg.set_markup(
                f"<b>Reverting in <span foreground='#fa4d56'>{remaining['s']}</span> seconds.</b>"
                "\n\nIf the new layout looks right, click <b>Keep</b>. "
                "Otherwise the previous layout will be restored automatically."
            )
            return True

        timer_id = GLib.timeout_add_seconds(1, tick)
        resp = dlg.run()
        GLib.source_remove(timer_id)
        dlg.destroy()

        if resp == Gtk.ResponseType.YES:
            self._original = copy.deepcopy(self._staged)
            self._toast("Layout kept.", kind="success")
        else:
            try:
                ds.apply_layout(previous_layout)
                log_action("displays: reverted layout")
            except (ValueError, OSError) as e:
                self._toast(f"Revert failed: {e}", kind="error")
            self._refresh_from_xfconf()
            self._toast("Layout reverted.", kind="info")

    # ---- toasts ----------------------------------------------------------

    def _toast(self, text: str, *, kind: str = "info") -> None:
        try:
            from mackes.workbench.shell.toasts import toast
            toast(text, kind=kind)
        except Exception:  # noqa: BLE001
            # Last resort — print to stderr so we don't swallow signals
            import sys
            print(f"[displays] {kind}: {text}", file=sys.stderr)


__all__ = ["DisplaysPanel"]
