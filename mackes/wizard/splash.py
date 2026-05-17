"""Wizard boot splash — plays branding/MACKES-XFCE-LOGO.mp4 before the wizard.

v1.4.0 follow-up.

Design:
  - Plays in a borderless GtkWindow at the video's native 1280×720 (or
    the screen, whichever is smaller) centered on the primary monitor.
  - Skippable: click anywhere, press Escape, or press any key.
  - Auto-dismisses on end-of-stream or pipeline error.
  - When the splash closes, calls `on_done` so the caller can show the
    real wizard window.

Robustness:
  - If GStreamer (`Gst`/`GstVideo`) isn't importable, skips splash and
    invokes `on_done` immediately.
  - If the MP4 file doesn't exist on disk, same.
  - If no H.264 decoder is available, plays whatever GStreamer manages
    or silently falls through.

GStreamer ↔ GTK embed pattern: we use playbin + autovideosink + the
VideoOverlay interface's `set_window_handle()` driven by the bus's
`sync-message::element` signal. This is the standard X11 embed path on
Fedora 44 stock GStreamer (gtksink is not packaged).
"""
from __future__ import annotations

from pathlib import Path
from typing import Callable, Optional

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402


# Splash dimensions (clamped to monitor size below)
_DEFAULT_W = 1280
_DEFAULT_H = 720


def video_path() -> Optional[Path]:
    """Resolve the bundled MP4 — installed path first, source-tree fallback."""
    for p in (
        Path("/usr/share/mackes-shell/branding/MACKES-XFCE-LOGO.mp4"),
        Path(__file__).resolve().parent.parent.parent / "branding"
            / "MACKES-XFCE-LOGO.mp4",
    ):
        if p.is_file():
            return p
    return None


def _gstreamer_available() -> bool:
    try:
        gi.require_version("Gst", "1.0")
        gi.require_version("GstVideo", "1.0")
        from gi.repository import Gst  # noqa: F401
        return True
    except Exception:  # noqa: BLE001
        return False


def show_splash(application: Gtk.Application,
                on_done: Callable[[], None]) -> bool:
    """Show the splash. Calls `on_done` when it dismisses for any reason.

    Returns True if the splash was opened (and on_done will fire later).
    Returns False if we couldn't start it (caller should run on_done
    synchronously after).
    """
    path = video_path()
    if path is None:
        return False
    if not _gstreamer_available():
        return False
    try:
        return _SplashWindow(application, path, on_done).open()
    except Exception:  # noqa: BLE001
        return False


# --------------------------------------------------------------------------
# Implementation
# --------------------------------------------------------------------------


class _SplashWindow:
    """Internal — wraps the GtkWindow + GStreamer pipeline."""

    def __init__(self, application: Gtk.Application, path: Path,
                 on_done: Callable[[], None]) -> None:
        self._application = application
        self._path = path
        self._on_done = on_done
        self._fired = False

        from gi.repository import Gst
        Gst.init(None)
        self._Gst = Gst

        # Window
        self._window = Gtk.ApplicationWindow(application=application)
        self._window.set_decorated(False)
        self._window.set_skip_taskbar_hint(True)
        self._window.set_skip_pager_hint(True)
        self._window.set_position(Gtk.WindowPosition.CENTER)
        self._window.set_modal(True)
        # Clamp dimensions to the active monitor
        w, h = self._compute_size()
        self._window.set_default_size(w, h)
        self._window.set_size_request(w, h)
        # Apply Carbon-black background
        self._window.get_style_context().add_class("mackes-app-window")
        self._window.override_background_color(
            Gtk.StateFlags.NORMAL, Gdk.RGBA(0.086, 0.086, 0.086, 1.0))

        # The DrawingArea that GStreamer will paint into
        self._darea = Gtk.DrawingArea()
        self._darea.set_double_buffered(False)
        self._darea.set_size_request(w, h)
        self._darea.override_background_color(
            Gtk.StateFlags.NORMAL, Gdk.RGBA(0.086, 0.086, 0.086, 1.0))
        self._window.add(self._darea)

        # Dismiss bindings
        self._window.add_events(
            Gdk.EventMask.BUTTON_PRESS_MASK | Gdk.EventMask.KEY_PRESS_MASK
        )
        self._window.connect("button-press-event", lambda *_: self._dismiss())
        self._window.connect("key-press-event",   lambda *_: self._dismiss())
        self._window.connect("destroy",           lambda *_: self._dismiss())

        # GStreamer pipeline (built lazily inside open() so realize-related
        # things land at the right time).
        self._pipeline = None

    def _compute_size(self) -> tuple[int, int]:
        try:
            display = Gdk.Display.get_default()
            mon = display.get_primary_monitor() or display.get_monitor(0)
            geom = mon.get_geometry()
            w = min(_DEFAULT_W, max(640, int(geom.width * 0.9)))
            h = min(_DEFAULT_H, max(360, int(geom.height * 0.9)))
            return w, h
        except Exception:  # noqa: BLE001
            return _DEFAULT_W, _DEFAULT_H

    def open(self) -> bool:
        # Build the pipeline AFTER the window is realized, so we have a
        # valid XID to hand the video sink.
        self._window.show_all()
        # Realize forces the GdkWindow to be created.
        self._window.realize()
        self._darea.realize()
        GLib.idle_add(self._start_pipeline)
        return True

    def _start_pipeline(self) -> bool:
        Gst = self._Gst
        from gi.repository import GstVideo  # noqa: F401  (ensures bindings loaded)

        playbin = Gst.ElementFactory.make("playbin", "playbin")
        if playbin is None:
            self._dismiss()
            return False
        playbin.set_property("uri", f"file://{self._path}")
        # Mute splash audio — not what users want at boot.
        try:
            playbin.set_property("mute", True)
        except Exception:  # noqa: BLE001
            pass

        bus = playbin.get_bus()
        bus.enable_sync_message_emission()
        bus.connect("sync-message::element", self._on_sync_message)
        bus.add_signal_watch()
        bus.connect("message::eos",   lambda *_: self._dismiss())
        bus.connect("message::error", self._on_error)

        playbin.set_state(Gst.State.PLAYING)
        self._pipeline = playbin
        return False  # idle_add one-shot

    def _on_sync_message(self, _bus, msg):
        """Wire the DrawingArea's XID into the video sink."""
        if msg.get_structure() is None:
            return
        if msg.get_structure().get_name() != "prepare-window-handle":
            return
        try:
            xid = self._darea.get_window().get_xid()
            msg.src.set_property("force-aspect-ratio", True)
            msg.src.set_window_handle(xid)
        except Exception:  # noqa: BLE001
            self._dismiss()

    def _on_error(self, _bus, msg) -> None:
        try:
            err, _dbg = msg.parse_error()
            from mackes.logging import log_action
            log_action(f"splash: GStreamer error {err.message} — skipping")
        except Exception:  # noqa: BLE001
            pass
        self._dismiss()

    def _dismiss(self) -> bool:
        if self._fired:
            return False
        self._fired = True
        try:
            if self._pipeline is not None:
                self._pipeline.set_state(self._Gst.State.NULL)
        except Exception:  # noqa: BLE001
            pass
        try:
            self._window.destroy()
        except Exception:  # noqa: BLE001
            pass
        try:
            self._on_done()
        except Exception:  # noqa: BLE001
            pass
        return False
