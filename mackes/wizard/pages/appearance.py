"""Wizard screen 4 — Appearance & Desktop (static, read-only).

The platform's appearance defaults are part of the Mackes brand and are
not user-configurable inside the wizard. This page shows what will be
applied so the user knows what they're getting, but offers no controls.

Theme, icon, font, cursor, and wallpaper choices can be changed *after*
first-run via Look & Feel → Appearance in the running app — the wizard
just lays down the platform defaults.
"""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.gtk_common import section_header

# The locked platform defaults. These mirror what apply_appearance,
# apply_themes, and apply_lightdm deploy at apply-time — keep this list
# in sync if those defaults ever change.
_DEFAULTS = (
    ("GTK theme",      "Orchis-Dark"),
    ("Window borders", "Shiki-Statler"),
    ("Icon theme",     "Mackes-Carbon"),
    ("Cursor theme",   "Adwaita (24px)"),
    ("Interface font", "Red Hat Text 11"),
    ("Monospace font", "Red Hat Mono 11"),
    ("Wallpaper",      "Mackes standard wallpaper"),
)


def _row(label_text: str, value_text: str) -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    row.set_margin_top(2); row.set_margin_bottom(2)

    label = Gtk.Label(label=label_text)
    label.set_xalign(0)
    label.set_size_request(160, -1)
    label.get_style_context().add_class("dim-label")
    row.pack_start(label, False, False, 0)

    value = Gtk.Label(label=value_text)
    value.set_xalign(0)
    value.set_selectable(False)
    row.pack_start(value, True, True, 0)
    return row


def build(ctx) -> Gtk.Widget:
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
    box.set_margin_top(28); box.set_margin_bottom(28)
    box.set_margin_start(40); box.set_margin_end(40)

    title = Gtk.Label(label="Appearance & Desktop")
    title.set_xalign(0); title.get_style_context().add_class("title-1")
    box.pack_start(title, False, False, 0)

    blurb = Gtk.Label(label=(
        "These platform defaults are part of the Mackes look — they install "
        "automatically. You can change any of them later in Look & Feel → "
        "Appearance."
    ))
    blurb.set_xalign(0); blurb.set_line_wrap(True)
    blurb.set_max_width_chars(72)
    blurb.get_style_context().add_class("dim-label")
    box.pack_start(blurb, False, False, 0)

    box.pack_start(section_header("Platform defaults"), False, False, 8)
    for label_text, value_text in _DEFAULTS:
        box.pack_start(_row(label_text, value_text), False, False, 0)

    return box
