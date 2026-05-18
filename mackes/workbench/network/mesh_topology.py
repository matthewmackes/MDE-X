"""Mesh VPN topology widget — Cairo-drawn live graph (v1.1.0).

Renders peers as a control-centered ring with animated edge pulses, dashed
DERP relay edges, and click-to-detail. Designed against the prototype in
docs/design/v1.1.0-carbon-refresh/project/panels-a.jsx::MeshTopology.

Public surface:
  MeshTopologyArea(Gtk.DrawingArea)
    .set_peers(peers: list[TopoPeer])
    .set_selected(name: str | None)
    .connect("peer-clicked", handler(area, peer_name))

The widget owns the animation loop (~30fps) and is safe to leave running —
it pauses redraws when the widget is not realized.
"""
from __future__ import annotations

import math
import time
from dataclasses import dataclass
from typing import List, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, GObject, Gtk  # noqa: E402


# Carbon Gray 100 palette (mirrors tokens.css)
_BG_LAYER_01     = (0x26 / 255, 0x26 / 255, 0x26 / 255)
_BG_GRAY_100     = (0x16 / 255, 0x16 / 255, 0x16 / 255)
_GRAY_70         = (0x52 / 255, 0x52 / 255, 0x52 / 255)
_GRAY_60         = (0x6f / 255, 0x6f / 255, 0x6f / 255)
_GRAY_30         = (0xc6 / 255, 0xc6 / 255, 0xc6 / 255)
_TEXT_PRIMARY    = (0xf4 / 255, 0xf4 / 255, 0xf4 / 255)
_TEXT_HELPER     = (0xa8 / 255, 0xa8 / 255, 0xa8 / 255)
_SUPPORT_SUCCESS = (0x42 / 255, 0xbe / 255, 0x65 / 255)
_SUPPORT_WARNING = (0xf1 / 255, 0xc2 / 255, 0x1b / 255)
_SUPPORT_ERROR   = (0xfa / 255, 0x4d / 255, 0x56 / 255)


@dataclass
class TopoPeer:
    name: str
    ip: str = ""
    role: str = "peer"        # "control" | "peer"
    status: str = "ok"        # "ok" | "warn" | "fail" | "offline"
    via_derp: bool = False    # if True, edge to control is drawn dashed
    rx_kbps: float = 0.0
    tx_kbps: float = 0.0


# ---------------------------------------------------------------------------
# Widget
# ---------------------------------------------------------------------------


class MeshTopologyArea(Gtk.DrawingArea):
    __gsignals__ = {
        "peer-clicked": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
    }

    def __init__(self) -> None:
        super().__init__()
        self.set_size_request(-1, 420)
        self._peers: List[TopoPeer] = []
        self._selected: Optional[str] = None
        self._hover: Optional[str] = None
        self._start_time = time.monotonic()
        self._tick_id: Optional[int] = None
        self._peer_hitboxes: list[tuple[float, float, float, str]] = []  # (cx, cy, r, name)

        self.add_events(
            self.get_events()
            | __import__("gi").repository.Gdk.EventMask.BUTTON_PRESS_MASK
            | __import__("gi").repository.Gdk.EventMask.POINTER_MOTION_MASK
            | __import__("gi").repository.Gdk.EventMask.LEAVE_NOTIFY_MASK
        )
        self.connect("draw", self._on_draw)
        self.connect("realize", self._on_realize)
        self.connect("unrealize", self._on_unrealize)
        self.connect("button-press-event", self._on_button_press)
        self.connect("motion-notify-event", self._on_motion)
        self.connect("leave-notify-event", self._on_leave)

    # ---- public API -------------------------------------------------------

    def set_peers(self, peers: List[TopoPeer]) -> None:
        self._peers = list(peers)
        self.queue_draw()

    def set_selected(self, name: Optional[str]) -> None:
        if self._selected == name:
            return
        self._selected = name
        self.queue_draw()

    # ---- lifecycle: animation loop ---------------------------------------

    def _on_realize(self, *_):
        # Cap at ~30fps for cheap animation; the only thing moving is the
        # pulse phase along the edges.
        self._tick_id = GLib.timeout_add(33, self._tick)

    def _on_unrealize(self, *_):
        if self._tick_id is not None:
            try:
                GLib.source_remove(self._tick_id)
            except Exception:  # noqa: BLE001
                pass
            self._tick_id = None

    def _tick(self) -> bool:
        # Only redraw if visible and mapped — avoid burning CPU when hidden.
        if not self.get_mapped():
            return True
        self.queue_draw()
        return True

    # ---- input handlers ---------------------------------------------------

    def _on_button_press(self, _w, event):
        x, y = event.x, event.y
        hit = self._hit_test(x, y)
        if hit is not None:
            self.set_selected(hit)
            self.emit("peer-clicked", hit)
        return False

    def _on_motion(self, _w, event):
        hit = self._hit_test(event.x, event.y)
        if hit != self._hover:
            self._hover = hit
            cursor_name = "pointer" if hit else "default"
            window = self.get_window()
            if window is not None:
                display = window.get_display()
                from gi.repository import Gdk
                cursor = Gdk.Cursor.new_from_name(display, cursor_name)
                window.set_cursor(cursor)
            self.queue_draw()
        return False

    def _on_leave(self, *_):
        if self._hover is not None:
            self._hover = None
            self.queue_draw()
        return False

    def _hit_test(self, x: float, y: float) -> Optional[str]:
        for cx, cy, r, name in self._peer_hitboxes:
            if (x - cx) ** 2 + (y - cy) ** 2 <= r * r:
                return name
        return None

    # ---- drawing ----------------------------------------------------------

    def _on_draw(self, _w, cr) -> bool:
        alloc = self.get_allocation()
        w, h = alloc.width, alloc.height
        if w <= 0 or h <= 0:
            return False

        # Background frame (Carbon layer-01)
        cr.set_source_rgb(*_BG_LAYER_01)
        cr.rectangle(0, 0, w, h)
        cr.fill()

        # Soft radial highlight
        from cairo import RadialGradient
        radial = RadialGradient(w / 2, h / 2, 0, w / 2, h / 2, max(w, h) * 0.6)
        radial.add_color_stop_rgba(0, 1, 1, 1, 0.025)
        radial.add_color_stop_rgba(1, 0, 0, 0, 0.0)
        cr.set_source(radial)
        cr.rectangle(0, 0, w, h)
        cr.fill()

        # Separate peers into control + peers
        control = next((p for p in self._peers if p.role == "control"), None)
        ring_peers = [p for p in self._peers if p is not control]

        cx, cy = w / 2, h / 2
        radius = max(80.0, min(w, h) * 0.34)

        # ---- 1. Reset hitboxes ------------------------------------------
        self._peer_hitboxes = []

        # ---- 2. Edges ---------------------------------------------------
        elapsed = time.monotonic() - self._start_time
        for i, p in enumerate(ring_peers):
            angle = 2 * math.pi * i / max(1, len(ring_peers)) - math.pi / 2
            px = cx + radius * math.cos(angle)
            py = cy + radius * math.sin(angle)

            # base edge
            if p.via_derp:
                cr.set_dash([6.0, 4.0])
                cr.set_source_rgba(*_GRAY_60, 0.6)
            else:
                cr.set_dash([])
                cr.set_source_rgba(*_GRAY_70, 0.6)
            cr.set_line_width(1.0)
            cr.move_to(cx, cy)
            cr.line_to(px, py)
            cr.stroke()

            # animated pulse — small bright dot traveling control → peer
            if p.status == "ok":
                phase = ((elapsed * 0.6) + (i * 0.13)) % 1.0
                pulse_x = cx + (px - cx) * phase
                pulse_y = cy + (py - cy) * phase
                color = self._accent_color()
                cr.set_dash([])
                cr.set_source_rgba(*color, max(0.0, 1.0 - phase) * 0.85)
                cr.arc(pulse_x, pulse_y, 3.0, 0, 2 * math.pi)
                cr.fill()

        cr.set_dash([])

        # ---- 3. Peer cards ---------------------------------------------
        for i, p in enumerate(ring_peers):
            angle = 2 * math.pi * i / max(1, len(ring_peers)) - math.pi / 2
            px = cx + radius * math.cos(angle)
            py = cy + radius * math.sin(angle)
            self._draw_peer_card(cr, px, py, p, is_control=False)

        # ---- 4. Control node (drawn last so it stays on top) ------------
        if control is not None:
            self._draw_peer_card(cr, cx, cy, control, is_control=True)

        return False

    def _draw_peer_card(self, cr, cx: float, cy: float, peer: TopoPeer, *,
                        is_control: bool) -> None:
        # Card geometry
        if is_control:
            card_w, card_h = 160, 56
        else:
            card_w, card_h = 144, 40
        x = cx - card_w / 2
        y = cy - card_h / 2

        # Fill
        if is_control:
            # gradient from gray-100 → accent-soft
            from cairo import LinearGradient
            r, g, b = self._accent_color()
            grad = LinearGradient(0, y, 0, y + card_h)
            grad.add_color_stop_rgb(0, *_BG_GRAY_100)
            grad.add_color_stop_rgba(1, r, g, b, 0.18)
            cr.set_source(grad)
        else:
            cr.set_source_rgb(*_BG_GRAY_100)
        cr.rectangle(x, y, card_w, card_h)
        cr.fill()

        # Border
        is_selected = (peer.name == self._selected)
        is_hovered = (peer.name == self._hover)
        border_color = self._accent_color() if (is_control or is_selected or is_hovered) else _GRAY_70
        if is_selected:
            cr.set_line_width(2.0)
        else:
            cr.set_line_width(1.0)
        cr.set_source_rgb(*border_color)
        cr.rectangle(x, y, card_w, card_h)
        cr.stroke()

        # Status dot — top-left inside
        dot_color = {
            "ok":   _SUPPORT_SUCCESS,
            "warn": _SUPPORT_WARNING,
            "fail": _SUPPORT_ERROR,
        }.get(peer.status, _GRAY_60)
        cr.set_source_rgb(*dot_color)
        cr.arc(x + 10, y + card_h / 2, 3.5, 0, 2 * math.pi)
        cr.fill()

        # Name + IP — text layout via Pango
        from gi.repository import Pango, PangoCairo
        layout = PangoCairo.create_layout(cr)
        layout.set_font_description(Pango.FontDescription("Red Hat Mono 10"))
        if is_control:
            text = f"<span foreground='#f4f4f4'>{_esc(peer.name)}</span>"
        else:
            text = f"<span foreground='#f4f4f4'>{_esc(peer.name)}</span>"
        layout.set_markup(text, -1)
        cr.set_source_rgb(*_TEXT_PRIMARY)
        cr.move_to(x + 24, y + 4)
        PangoCairo.show_layout(cr, layout)

        # IP (right-aligned, helper color)
        if peer.ip:
            layout2 = PangoCairo.create_layout(cr)
            layout2.set_font_description(Pango.FontDescription("Red Hat Mono 9"))
            layout2.set_text(peer.ip, -1)
            ink, logical = layout2.get_pixel_extents()
            cr.set_source_rgb(*_TEXT_HELPER)
            cr.move_to(x + card_w - logical.width - 8, y + card_h - logical.height - 4)
            PangoCairo.show_layout(cr, layout2)

        # Hitbox bounding circle (diagonal/2 radius)
        hit_r = max(card_w, card_h) / 2 + 4
        self._peer_hitboxes.append((cx, cy, hit_r, peer.name))

    # ---- color helpers ----------------------------------------------------

    def _accent_color(self):
        # Read the accent from the GTK style context if possible; else
        # fall back to Carbon orange (Mackes default).
        try:
            ctx = self.get_style_context()
            ok, rgba = ctx.lookup_color("mackes_accent")
            if ok:
                return (rgba.red, rgba.green, rgba.blue)
        except Exception:  # noqa: BLE001
            pass
        return (0xf1 / 255, 0x85 / 255, 0x3d / 255)


def _esc(s: str) -> str:
    return (s.replace("&", "&amp;")
              .replace("<", "&lt;")
              .replace(">", "&gt;"))
