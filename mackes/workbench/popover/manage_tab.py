"""Manage tab — fleet + screens + boot/login.

v1.6.5: Tweaks sub-tab removed (the TweaksPanel module was deleted
along with the floating overlay). The per-feature toggles it used to
expose (mesh clipboard, maximize-all, Thunar autostart, Conky HUD,
Remmina sync) are still readable + writeable via tweaks.json directly
and via per-module CLIs (`mackes remmina-sync --enable`, etc.).
"""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.workbench.popover._subtabs import build_subtab_container


class ManageTab(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        items = [
            ("fleet",   "", "Fleet",
             "mackes.workbench.fleet.inventory:FleetInventoryPanel"),
            ("screens", "", "Screens",
             "mackes.workbench.system.displays:DisplaysPanel"),
            ("boot",    "", "Boot",
             "mackes.workbench.system.boot_login:BootLoginPanel"),
        ]
        self.pack_start(build_subtab_container(items), True, True, 0)


__all__ = ["ManageTab"]
