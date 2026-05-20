"""System → Removable Media (v2.0.0 Phase F.2 — switched to MDE bridge).

The v1.x panel exposed 13 thunar-volman knobs (per-device-class
auto-mount + auto-run + auto-play). The v2.0.0 MDE schema collapses
that down to the 3 keys mdersession + udisks2 actually honor:

  * automount.on_insert    — auto-mount removable media + drives
  * automount.open_on_mount — auto-open the file manager on mount
  * automount.autorun      — honor autorun.sh / autorun.inf
                              (default off for safety)

Per-device-class toggles (iPod, camera, scanner, audio CD, DVD,
graphics tablet, etc.) live with the application that handles them
on the v2.0.0 line — KDE Connect / GNOME Photos / cdplayer-equivalent
each own their own integration. The Workbench panel no longer
duplicates the configuration.
"""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes import mde_settings_bridge as _b
from mackes.workbench._common import (
    a11y, info_label, labeled_row, panel_box, section_header, title_label,
)


_SWITCHES: list[tuple[str, str]] = [
    ("automount.on_insert",
     "Auto-mount drives and removable media on connect"),
    ("automount.open_on_mount",
     "Auto-open the file manager when media mounts"),
    ("automount.autorun",
     "Honor autorun.sh / autorun.inf on inserted media "
     "(off by default for safety)"),
]


class RemovablePanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._switches: dict[str, Gtk.Switch] = {}
        self.add(self._build())

    def _build(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(title_label("Removable Media"), False, False, 0)
        box.pack_start(info_label(
            "What your machine should do when you plug in a USB drive "
            "or insert removable media. Per-device-class auto-actions "
            "(camera, scanner, audio CD, etc.) live with the matching "
            "application on the v2.0.0 line."
        ), False, False, 0)

        box.pack_start(section_header("Auto-mount"), False, False, 0)

        for key, label in _SWITCHES:
            sw = Gtk.Switch()
            sw.set_active(bool(_b.get_setting(key)))

            def _on_active(s, _gp, _k=key):
                _b.set_setting(_k, bool(s.get_active()))

            sw.connect("notify::active", _on_active)
            a11y(sw, name=f"Removable-media auto-action: {label}",
                 tooltip=f"Toggle the {key} setting")
            self._switches[key] = sw
            box.pack_start(labeled_row(label, sw), False, False, 0)

        return box
