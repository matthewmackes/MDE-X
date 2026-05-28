"""Wizard screen 8 — Snapshot Policy."""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.gtk_common import info_label, labeled_row, section_header


def build(ctx) -> Gtk.Widget:
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
    box.set_margin_top(28); box.set_margin_bottom(28)
    box.set_margin_start(40); box.set_margin_end(40)

    title = Gtk.Label(label="Initial Restore Point")
    title.set_xalign(0); title.get_style_context().add_class("title-1")
    box.pack_start(title, False, False, 0)

    box.pack_start(info_label(
        "Mackes can capture your current config (xfconf channels + Polybar / "
        "Plank / Rofi config trees) into a timestamped snapshot before applying "
        "the preset. Restore from Maintain → Snapshots if you change your mind."
    ), False, False, 0)

    box.pack_start(section_header("Snapshot"), False, False, 0)

    create_switch = Gtk.Switch(); create_switch.set_active(ctx.create_initial_snapshot)
    def on_create(s, _g):
        ctx.create_initial_snapshot = s.get_active()
        label_entry.set_sensitive(s.get_active())
    create_switch.connect("notify::active", on_create)
    box.pack_start(labeled_row("Create initial restore point", create_switch),
                   False, False, 0)

    label_entry = Gtk.Entry()
    default_label = ctx.snapshot_label
    if ctx.selected_preset and ctx.selected_preset.snapshot.get("initial_snapshot_name"):
        default_label = str(ctx.selected_preset.snapshot["initial_snapshot_name"])
    label_entry.set_text(default_label)
    label_entry.connect("changed", lambda e: setattr(ctx, "snapshot_label", e.get_text()))
    box.pack_start(labeled_row("Label", label_entry), False, False, 0)
    ctx.snapshot_label = default_label

    return box
