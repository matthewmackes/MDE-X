"""Wizard screen 6 — Hardware (display, audio, power)."""
from __future__ import annotations

import subprocess

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.workbench._common import labeled_row, section_header
from mackes.audio import _list_sinks, _default_sink, _set_default_sink


POWER_PROFILES = ["performance", "balanced", "power-saver"]


def _xrandr_summary() -> str:
    try:
        out = subprocess.check_output(["xrandr", "--query"], text=True, stderr=subprocess.DEVNULL)
    except (FileNotFoundError, subprocess.CalledProcessError):
        return "xrandr not available."
    lines = []
    for line in out.splitlines():
        if " connected" in line or " disconnected" in line:
            lines.append(line.split(" (")[0])
    return "\n".join(lines) or "No displays detected."


def build(ctx) -> Gtk.Widget:
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
    box.set_margin_top(28); box.set_margin_bottom(28)
    box.set_margin_start(40); box.set_margin_end(40)

    title = Gtk.Label(label="Hardware")
    title.set_xalign(0); title.get_style_context().add_class("title-1")
    box.pack_start(title, False, False, 0)

    overrides = ctx.overrides.setdefault("devices", {})
    preset_dev = ctx.selected_preset.devices if ctx.selected_preset else {}

    box.pack_start(section_header("Displays detected"), False, False, 0)
    view = Gtk.TextView(); view.set_editable(False); view.set_monospace(True)
    view.get_buffer().set_text(_xrandr_summary())
    view.set_size_request(-1, 90)
    box.pack_start(view, False, False, 0)
    launch = Gtk.Button(label="Open xfce4-display-settings (arrange now)")
    def on_launch(_):
        subprocess.Popen(["xfce4-display-settings"], stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL)
    launch.connect("clicked", on_launch)
    box.pack_start(launch, False, False, 0)

    box.pack_start(section_header("Default audio sink"), False, False, 0)
    sinks = _list_sinks()
    sink_combo = Gtk.ComboBoxText()
    for _, label in sinks:
        sink_combo.append_text(label)
    cur = _default_sink()
    names = [n for n, _ in sinks]
    if cur in names:
        sink_combo.set_active(names.index(cur))
    elif sinks:
        sink_combo.set_active(0)
    def on_sink(c):
        txt = c.get_active_text()
        if txt:
            _set_default_sink(txt)
            overrides["audio_default_sink"] = txt
    sink_combo.connect("changed", on_sink)
    box.pack_start(labeled_row("Sink", sink_combo), False, False, 0)

    box.pack_start(section_header("Power profile"), False, False, 0)
    power = Gtk.ComboBoxText()
    for p in POWER_PROFILES:
        power.append_text(p)
    initial = preset_dev.get("power_profile", "balanced")
    power.set_active(POWER_PROFILES.index(initial) if initial in POWER_PROFILES else 1)
    def on_power(c):
        txt = c.get_active_text()
        if txt:
            overrides["power_profile"] = txt
    power.connect("changed", on_power)
    box.pack_start(labeled_row("Profile", power), False, False, 0)
    on_power(power)  # seed the override

    return box
