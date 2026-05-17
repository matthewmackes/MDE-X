"""Toast host — shell-wide non-modal notifications (v1.4.0).

Anchored bottom-right inside the workbench window's Gtk.Overlay.
Toasts auto-dismiss after `duration_ms` and stack vertically.

Usage from anywhere:

  from mackes.workbench.shell.toasts import toast
  toast("Snapshot created", kind="success")
  toast("dnf install failed", kind="error", duration_ms=6000)

The host is wired up by the WorkbenchWindow at startup; calls before
the window exists are silently dropped.
"""
from __future__ import annotations

from typing import Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402


_host: Optional["ToastHost"] = None


class ToastHost(Gtk.Box):
    """A vertical box that holds active toast widgets."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self.set_halign(Gtk.Align.END)
        self.set_valign(Gtk.Align.END)
        self.set_margin_end(24); self.set_margin_bottom(24)

    def show_toast(self, message: str, *, kind: str = "info",
                   duration_ms: int = 3200) -> None:
        toast = self._make_toast(message, kind)
        self.pack_start(toast, False, False, 0)
        toast.show_all()
        GLib.timeout_add(duration_ms, self._dismiss, toast)

    def _dismiss(self, toast: Gtk.Widget) -> bool:
        try:
            self.remove(toast)
        except Exception:  # noqa: BLE001
            pass
        return False  # one-shot

    @staticmethod
    def _make_toast(message: str, kind: str) -> Gtk.Widget:
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        box.get_style_context().add_class("mackes-toast")
        if kind in ("success", "error", "warning", "info"):
            box.get_style_context().add_class(kind)
        # icon-as-dot
        dot = Gtk.Label(label="●")
        dot.get_style_context().add_class("mackes-dot")
        dot.get_style_context().add_class(
            {"success": "ok", "error": "fail",
             "warning": "warn", "info": "accent"}.get(kind, "accent")
        )
        box.pack_start(dot, False, False, 0)
        # text
        msg = Gtk.Label(label=message)
        msg.set_xalign(0); msg.set_line_wrap(True)
        msg.set_max_width_chars(48)
        box.pack_start(msg, True, True, 0)
        # close button
        close = Gtk.Button(label="✕")
        close.set_relief(Gtk.ReliefStyle.NONE)
        close.get_style_context().add_class("mackes-header-action")
        close.connect("clicked", lambda *_: box.get_parent().remove(box)
                                            if box.get_parent() else None)
        box.pack_end(close, False, False, 0)
        box.set_size_request(360, -1)
        return box


def install_host(overlay: Gtk.Overlay) -> ToastHost:
    """Mount a ToastHost on the given overlay. Returns the host instance."""
    global _host
    _host = ToastHost()
    overlay.add_overlay(_host)
    return _host


def toast(message: str, *, kind: str = "info",
          duration_ms: int = 3200) -> None:
    """Show a toast. Silently no-ops if the host isn't installed yet."""
    if _host is None:
        return
    _host.show_toast(message, kind=kind, duration_ms=duration_ms)
