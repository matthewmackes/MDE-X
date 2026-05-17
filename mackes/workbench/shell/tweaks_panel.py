"""Tweaks panel — bottom-right floating drawer.

Q5 of the v1.1.0 design survey: ship the full Tweaks panel with:
  * Preset switcher (live accent swap across the whole UI)
  * Density (compact / cozy / comfortable)
  * Chrome (XFCE frame on/off, status bar on/off)
  * Re-open Wizard button

State persists to ~/.config/mackes-shell/tweaks.json — owned by the shell.

Visually it's a GtkOverlay child anchored to bottom-right, containing a
toggle button. Clicking the button shows a popover-like Gtk.Revealer
that slides up a Carbon-styled panel with the controls.
"""
from __future__ import annotations

from typing import Callable, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402


PRESETS = [
    ("hashbang", "#!"),
    ("mackes",   "Mackes"),
    ("daylight", "Daylight"),
    ("vanilla",  "Vanilla"),
    ("node",     "Node"),
]


class TweaksOverlay(Gtk.Box):
    """Anchored bottom-right within a Gtk.Overlay."""

    def __init__(self, window: Gtk.Window, tweaks: dict,
                 on_change: Callable[[dict], None]) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self._window = window
        self._tweaks = dict(tweaks)
        self._on_change = on_change
        self.set_halign(Gtk.Align.END)
        self.set_valign(Gtk.Align.END)
        self.set_margin_end(24); self.set_margin_bottom(24)

        # The drawer reveals upward from the gear button.
        self._revealer = Gtk.Revealer()
        self._revealer.set_transition_type(Gtk.RevealerTransitionType.SLIDE_UP)
        self._revealer.set_transition_duration(160)
        self._revealer.set_reveal_child(False)
        self._drawer = self._build_drawer()
        self._revealer.add(self._drawer)
        self.pack_start(self._revealer, False, False, 0)

        # Gear button (the always-visible part).
        btn = Gtk.Button()
        btn.set_image(Gtk.Image.new_from_icon_name("emblem-system-symbolic", Gtk.IconSize.LARGE_TOOLBAR))
        btn.get_style_context().add_class("mackes-tweaks-button")
        btn.set_tooltip_text("Tweaks (preset, density, chrome)")
        btn.connect("clicked", self._toggle)
        # Right-align it under the drawer.
        btn_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        btn_row.pack_end(btn, False, False, 0)
        self.pack_start(btn_row, False, False, 0)

        # Don't intercept clicks elsewhere on the overlay.
        self.set_size_request(-1, -1)

    # ---- Build the drawer body -------------------------------------------

    def _build_drawer(self) -> Gtk.Widget:
        drawer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        drawer.get_style_context().add_class("mackes-tweaks-drawer")

        head = Gtk.Label(label="Tweaks")
        head.set_xalign(0)
        head.get_style_context().add_class("mackes-section-title")
        drawer.pack_start(head, False, False, 0)

        # ---- Preset ------------------------------------------------------
        drawer.pack_start(_section_title("Preset"), False, False, 0)
        self._preset_buttons = {}
        for key, label in PRESETS:
            r = _radio_row(label, selected=(self._tweaks.get("preset") == key))
            r.connect("clicked", lambda _b, k=key: self._set("preset", k))
            self._preset_buttons[key] = r
            drawer.pack_start(r, False, False, 0)

        # ---- Density -----------------------------------------------------
        drawer.pack_start(_section_title("Density"), False, False, 0)
        density_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        self._density_buttons = {}
        for opt in ("compact", "cozy", "comfortable"):
            b = Gtk.ToggleButton(label=opt.title())
            b.set_active(self._tweaks.get("density") == opt)
            b.connect("toggled", lambda btn, o=opt: btn.get_active() and self._set("density", o))
            self._density_buttons[opt] = b
            density_row.pack_start(b, True, True, 0)
        drawer.pack_start(density_row, False, False, 0)

        # ---- Chrome ------------------------------------------------------
        drawer.pack_start(_section_title("Chrome"), False, False, 0)
        sb_row = _switch_row("Status bar",
                             initial=bool(self._tweaks.get("show_status_bar", True)),
                             on_change=lambda v: self._set("show_status_bar", v))
        drawer.pack_start(sb_row, False, False, 0)
        xf_row = _switch_row("XFCE frame",
                             initial=bool(self._tweaks.get("show_xfce_frame", True)),
                             on_change=lambda v: self._set("show_xfce_frame", v))
        drawer.pack_start(xf_row, False, False, 0)
        conky_row = _switch_row("Conky HUD",
                                initial=bool(self._tweaks.get("show_conky", True)),
                                on_change=lambda v: self._set("show_conky", v))
        drawer.pack_start(conky_row, False, False, 0)

        # ---- Actions -----------------------------------------------------
        drawer.pack_start(Gtk.Separator(), False, False, 0)
        wiz_btn = Gtk.Button(label="Re-open Wizard")
        wiz_btn.get_style_context().add_class("suggested-action")
        wiz_btn.connect("clicked", self._on_open_wizard)
        drawer.pack_start(wiz_btn, False, False, 0)

        drawer.set_size_request(280, -1)
        drawer.set_margin_top(0)
        return drawer

    # ---- Behavior --------------------------------------------------------

    def _toggle(self, *_):
        self._revealer.set_reveal_child(not self._revealer.get_reveal_child())

    def open(self) -> None:
        self._revealer.set_reveal_child(True)

    def close(self) -> None:
        self._revealer.set_reveal_child(False)

    def _set(self, key: str, value):
        self._tweaks[key] = value
        # Update preset radio visuals
        if key == "preset":
            for k, btn in self._preset_buttons.items():
                _set_radio_selected(btn, k == value)
        # Update density toggles
        if key == "density":
            for opt, b in self._density_buttons.items():
                if b.get_active() != (opt == value):
                    b.set_active(opt == value)
        self._on_change(self._tweaks)

    def _on_open_wizard(self, *_):
        from mackes.wizard.window import WizardWindow
        try:
            from mackes.state import MackesState
            state = MackesState.load()
            w = WizardWindow(application=self._window.get_application(), state=state)
            w.show_all()
        except Exception:  # noqa: BLE001
            pass


# ---- internal widget helpers ---------------------------------------------


def _section_title(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-tweaks-section-title")
    return lab


def _radio_row(label: str, *, selected: bool) -> Gtk.Button:
    btn = Gtk.Button()
    btn.set_relief(Gtk.ReliefStyle.NONE)
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    dot = Gtk.Label(label="●" if selected else "○")
    dot.get_style_context().add_class("mackes-dot")
    if selected:
        dot.get_style_context().add_class("accent")
    row.pack_start(dot, False, False, 0)
    text = Gtk.Label(label=label)
    text.set_xalign(0)
    row.pack_start(text, True, True, 0)
    btn.add(row)
    # Stash the dot widget on the button for later updates
    btn._mackes_dot = dot  # type: ignore[attr-defined]
    return btn


def _set_radio_selected(btn: Gtk.Button, selected: bool) -> None:
    dot = getattr(btn, "_mackes_dot", None)
    if isinstance(dot, Gtk.Label):
        dot.set_text("●" if selected else "○")
        ctx = dot.get_style_context()
        if selected:
            ctx.add_class("accent")
        else:
            ctx.remove_class("accent")


def _switch_row(label: str, *, initial: bool,
                on_change: Callable[[bool], None]) -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    row.set_margin_top(4); row.set_margin_bottom(4)
    text = Gtk.Label(label=label)
    text.set_xalign(0)
    row.pack_start(text, True, True, 0)
    sw = Gtk.Switch()
    sw.set_active(initial)
    sw.connect("notify::active", lambda s, _gp: on_change(s.get_active()))
    row.pack_start(sw, False, False, 0)
    return row
