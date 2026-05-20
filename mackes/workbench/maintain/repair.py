"""Maintain → Repair.

Common recovery operations:
  - Re-apply the active preset (fixes drift)
  - Rebuild the menu folder (re-hide xfce4-settings entries)
  - Restore xfce4-settings menu entries (un-hide; for when Mackes is leaving)
  - Re-install the mackes-shell .desktop entry
"""
from __future__ import annotations

from pathlib import Path

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

# v2.0.0 Phase F.10 — mackes.menu_integration deleted (XFCE settings
# panels no longer installed). The repair actions that used to call
# into it (re-hide / restore xfce4-settings menu entries, re-install
# Mackes menu entry) are now no-ops that log an informational line.
from mackes.presets import apply_preset, load_preset
from mackes.state import MackesState
from mackes.workbench._common import (
    a11y, info_label, panel_box, section_description, section_header, title_label,
)


SHIPPED_DESKTOP_CANDIDATES = [
    Path("/usr/share/applications/mackes-shell.desktop"),
    Path(__file__).resolve().parent.parent.parent.parent / "data" / "applications" / "mackes-shell.desktop",
]


def _ship_desktop() -> Path | None:
    for c in SHIPPED_DESKTOP_CANDIDATES:
        if c.exists():
            return c
    return None


class RepairPanel(Gtk.Box):
    def __init__(self, state: MackesState) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.state = state
        self._build()

    def _build(self) -> None:
        box = panel_box()
        box.pack_start(title_label("Repair"), False, False, 0)
        box.pack_start(info_label(
            "Safe, one-click fixes for common problems with the "
            "desktop, panel, and Mackes itself. Try these before "
            "anything more drastic."
        ), False, False, 0)
        box.pack_start(section_description(
            "Each repair runs on its own — none of them will wipe your "
            "personal files."
        ), False, False, 0)

        box.pack_start(section_header("Output"), False, False, 0)
        self._output = Gtk.TextView()
        self._output.set_editable(False); self._output.set_monospace(True)
        self._output.set_size_request(-1, 220)
        scroll = Gtk.ScrolledWindow(); scroll.add(self._output)
        scroll.set_size_request(-1, 220)
        box.pack_start(scroll, True, True, 0)

        box.pack_start(section_header("Actions"), False, False, 0)
        grid = Gtk.Grid(column_spacing=8, row_spacing=8, column_homogeneous=True)

        ops = [
            ("Re-apply active preset",        self._reapply_preset,
             "Write every active-preset value back to its target setting"),
            ("Re-hide xfce4-settings menu",   self._rehide_menus,
             "Re-hide the XFCE settings menu entries that Mackes replaces"),
            ("Restore xfce4-settings menu",   self._restore_menus,
             "Un-hide the XFCE settings menu entries (e.g. before uninstall)"),
            ("Re-install Mackes menu entry",  self._reinstall_entry,
             "Re-install the mackes-shell.desktop launcher into the menu"),
        ]
        for i, (label, fn, desc) in enumerate(ops):
            btn = Gtk.Button(label=label)
            btn.connect("clicked", lambda _b, f=fn: self._run(f))
            a11y(btn, name=desc, tooltip=desc)
            grid.attach(btn, i % 2, i // 2, 1, 1)
        box.pack_start(grid, False, False, 0)

        self.add(box)
        self._append("Ready. Click an action above.\n")

    def _append(self, text: str) -> None:
        buf = self._output.get_buffer()
        end = buf.get_end_iter()
        buf.insert(end, text)
        end = buf.get_end_iter()
        self._output.scroll_to_iter(end, 0, False, 0, 1)

    def _run(self, fn) -> None:
        self._append(f"\n--- {fn.__doc__ or fn.__name__} ---\n")
        try:
            for line in fn():
                self._append(line + "\n")
        except Exception as e:  # noqa: BLE001
            self._append(f"ERROR: {e}\n")

    # ----- operations -----------------------------------------------------

    def _reapply_preset(self) -> list[str]:
        """Re-apply the active preset"""
        if not self.state.active_preset:
            return ["No active preset set in state.json."]
        preset = load_preset(self.state.active_preset)
        if preset is None:
            return [f"Preset {self.state.active_preset!r} not found."]
        return apply_preset(preset)

    def _rehide_menus(self) -> list[str]:
        """v2.0.0 Phase F.10 — XFCE settings panels no longer
        installed; nothing to hide."""
        return [
            "Re-hide menus: no-op on v2.0.0 (mackes.menu_integration "
            "retired; XFCE settings panels no longer installed)."
        ]

    def _restore_menus(self) -> list[str]:
        """v2.0.0 Phase F.10 — XFCE settings panels no longer
        installed; nothing to restore."""
        return [
            "Restore menus: no-op on v2.0.0 (mackes.menu_integration "
            "retired; XFCE settings panels no longer installed)."
        ]

    def _reinstall_entry(self) -> list[str]:
        """v2.0.0 Phase F.10 — MDE ships its own .desktop entry
        via the RPM; the v1.x re-install path is no longer needed
        because the entry is owned by `data/applications/mde.desktop`
        and refreshed on every `dnf upgrade`."""
        return [
            "Re-install entry: no-op on v2.0.0 (data/applications/ "
            "mde.desktop is package-owned; reinstall the RPM to refresh)."
        ]
