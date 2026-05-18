"""Maintain → Reset to Preset.

Heavyweight recovery: snapshot the current state, then apply the active
preset across every section. Drops the user back to the preset's intent.
"""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.presets import apply_preset, list_presets, load_preset
from mackes.snapshots import create_snapshot
from mackes.state import MackesState
from mackes.workbench._common import (
    error_label, info_label, labeled_row, panel_box, section_description, section_header, title_label,
)


class ResetToPresetPanel(Gtk.Box):
    def __init__(self, state: MackesState) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.state = state
        self._build()

    def _build(self) -> None:
        box = panel_box()
        box.pack_start(title_label("Reset to Preset"), False, False, 0)
        box.pack_start(info_label(
            "Wipe your settings back to one of the built-in presets — "
            "the same look and feel you'd get on a fresh install."
        ), False, False, 0)
        box.pack_start(section_description(
            "Mackes saves a snapshot first, so you can undo the reset "
            "from Maintain → Snapshots if you change your mind."
        ), False, False, 0)

        box.pack_start(section_header("Preset"), False, False, 0)
        presets = list_presets()
        if not presets:
            box.pack_start(error_label(
                "No presets found. Mackes expects YAML files in data/presets/."
            ), False, False, 0)
            self.add(box); return

        names = [p.name for p in presets]
        labels = [f"{p.display_name}  ({p.name})" for p in presets]
        combo = Gtk.ComboBoxText()
        for lbl in labels:
            combo.append_text(lbl)
        active = self.state.active_preset or names[0]
        combo.set_active(names.index(active) if active in names else 0)
        box.pack_start(labeled_row("Apply", combo), False, False, 0)

        self._description = Gtk.Label(label=presets[0].description)
        self._description.set_xalign(0); self._description.set_line_wrap(True)
        self._description.get_style_context().add_class("dim-label")
        box.pack_start(self._description, False, False, 0)
        def on_changed(c, _names=names, _presets=presets):
            i = c.get_active()
            if i >= 0:
                self._description.set_text(_presets[i].description or "(no description)")
        combo.connect("changed", on_changed)

        box.pack_start(section_header("Sections to apply"), False, False, 0)
        self._section_checks: dict[str, Gtk.CheckButton] = {}
        for sect in ("appearance", "shell", "devices", "system", "network"):
            cb = Gtk.CheckButton(label=sect.title())
            cb.set_active(True)
            box.pack_start(cb, False, False, 0)
            self._section_checks[sect] = cb

        self._snapshot_first = Gtk.CheckButton(label="Create a snapshot first (recommended)")
        self._snapshot_first.set_active(True)
        box.pack_start(self._snapshot_first, False, False, 0)

        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        apply_btn = Gtk.Button(label="Apply preset now")
        apply_btn.get_style_context().add_class("destructive-action")
        def _on_apply(_):
            i = combo.get_active()
            if i >= 0:
                self._apply(names[i])
        apply_btn.connect("clicked", _on_apply)
        bar.pack_start(apply_btn, False, False, 0)
        box.pack_start(bar, False, False, 0)

        box.pack_start(section_header("Output"), False, False, 0)
        self._output = Gtk.TextView()
        self._output.set_editable(False); self._output.set_monospace(True)
        self._output.set_size_request(-1, 220)
        scroll = Gtk.ScrolledWindow(); scroll.add(self._output)
        scroll.set_size_request(-1, 220)
        box.pack_start(scroll, True, True, 0)

        self.add(box)

    def _apply(self, preset_name: str) -> None:
        preset = load_preset(preset_name)
        if preset is None:
            self._append(f"preset {preset_name!r} not found\n"); return

        dialog = Gtk.MessageDialog(
            transient_for=self.get_toplevel(), modal=True,
            message_type=Gtk.MessageType.WARNING, buttons=Gtk.ButtonsType.OK_CANCEL,
            text=f"Apply preset '{preset.display_name}'?",
        )
        dialog.format_secondary_text(
            "This overwrites your appearance, shell, and selected system / network "
            "settings to match the preset. A snapshot will be created first "
            "(if checked above)."
        )
        resp = dialog.run(); dialog.destroy()
        if resp != Gtk.ResponseType.OK:
            return

        self._output.get_buffer().set_text("")
        sections = {s for s, cb in self._section_checks.items() if cb.get_active()}

        if self._snapshot_first.get_active():
            snap = create_snapshot(label=f"pre-reset-to-{preset.name}",
                                   source_preset=self.state.active_preset)
            self._append(f"snapshot: {snap.name}\n")

        for line in apply_preset(preset, sections=sections):
            self._append(line + "\n")

        self.state.mark_provisioned(preset.name)
        self._append("\nDone. Active preset updated.\n")

    def _append(self, text: str) -> None:
        buf = self._output.get_buffer()
        end = buf.get_end_iter()
        buf.insert(end, text)
        end = buf.get_end_iter()
        self._output.scroll_to_iter(end, 0, False, 0, 1)
