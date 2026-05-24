"""First-run wizard — Carbon-native window (v1.4.0).

Replaces the v1.0-era Gtk.Assistant with a custom Carbon shell:

  +--------------------------------------------------------------+
  |  Mackes Shell — Setup                                  _ □ × |
  +--------------------------------------------------------------+
  | [1] Welcome    [2] Scan    [3] Preset   ...    [9] Apply     |  ← step strip
  +--------------------------------------------------------------+
  |                                                              |
  |   <page content — scrollable, Carbon-styled>                 |
  |                                                              |
  +--------------------------------------------------------------+
  | ‹ Back                                       Cancel · Next › |
  +--------------------------------------------------------------+

The existing page builder modules (welcome / env_scan / preset_pick /
appearance / hardware / network / snapshot / review / apply) drop in
unchanged — they were already Carbon-styled inside. Only the outer
chrome moves to a Carbon Gtk.ApplicationWindow.
"""
from __future__ import annotations

from typing import List, Tuple

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

from mackes.presets import list_presets
from mackes.state import MackesState
from mackes.wizard.context import WizardContext
from mackes.wizard.pages import (
    appearance, apply, env_scan, hardware, legacy_import,
    mesh_passcode, network, preset_pick, review, snapshot, welcome,
)


def _primary_monitor_size() -> tuple[int, int]:
    """Detect the primary monitor pixel size for fit-to-resolution windows."""
    try:
        display = Gdk.Display.get_default()
        if display is None:
            return (1280, 800)
        mon = display.get_primary_monitor() or display.get_monitor(0)
        geom = mon.get_geometry()
        return (max(1024, geom.width), max(700, geom.height))
    except Exception:  # noqa: BLE001
        return (1280, 800)


# Step kinds shape the bottom-bar behavior — they mirror the v1.0 page
# types but stay inside our own code (no GtkAssistantPageType dependency).
_STEP_CONTENT  = "content"
_STEP_CONFIRM  = "confirm"
_STEP_PROGRESS = "progress"
_STEP_SUMMARY  = "summary"


class WizardWindow(Gtk.ApplicationWindow):
    """Carbon-native replacement for Gtk.Assistant.

    Constructor signature is unchanged so callers in mackes.app keep
    working.
    """

    def __init__(
        self,
        application: Gtk.Application,
        state: MackesState,
        *,
        reconfigure: bool = False,
    ) -> None:
        super().__init__(application=application)
        from mackes.workbench._common import versioned_title
        # v2.0.0 Phase 0.11 — "Mackes Shell" → "Setup" in titlebar;
        # versioned_title prepends the "MDE <version>" suffix.
        # NF-7.4 (v2.5, 2026-05-23): reconfigure entries (Workbench
        # → Mesh panel → "Reset and rejoin") flip the titlebar to
        # "Mesh setup" so the operator knows the welcome step gets
        # skipped.
        title_kind = "Mesh setup" if reconfigure else "Setup"
        self.set_title(versioned_title(title_kind))
        self._reconfigure = reconfigure
        # v1.4.2 — Fit the workstation resolution perfectly. Open at the
        # primary monitor's exact size and maximize on realize so the
        # WM finishes the job.
        mon_w, mon_h = _primary_monitor_size()
        self.set_default_size(mon_w, mon_h)
        self.connect("realize", lambda *_: self.maximize())
        self.state = state
        self.ctx = WizardContext()
        self.get_style_context().add_class("mackes-app-window")
        if state.active_preset:
            self.get_style_context().add_class(f"preset-{state.active_preset}")

        # Q2 lock — auto-select when only one GUI-visible preset ships.
        shipped_presets = [p for p in list_presets() if p.name != "node"]
        single_preset = len(shipped_presets) == 1
        if single_preset:
            self.ctx.selected_preset = shipped_presets[0]

        # Escape closes the wizard — standard dialog idiom that users
        # expect on Linux. Press handler is wired below in _on_key_press.
        self.connect("key-press-event", self._on_key_press)

        # ---- Build page widgets + step model ------------------------------
        self._apply_page = apply.ApplyPage(self.ctx)

        steps: List[Tuple[str, str, Gtk.Widget]] = []
        # NF-7.4 — reconfigure flow skips the welcome step (the
        # operator has already been through this wizard at least
        # once + clicked "Reset and rejoin" knowing what comes
        # next). First-boot keeps the welcome page so users who
        # land here for the first time get the orientation.
        if not self._reconfigure:
            steps.append(
                ("Welcome", _STEP_CONTENT, welcome.build(self.ctx))
            )
        steps += [
            ("Scan",        _STEP_CONTENT,  env_scan.build(self.ctx)),
            # Legacy import sits between Scan and Preset so 2.x users
            # see what's being preserved BEFORE they pick a (new)
            # preset. Page is self-detecting; on a fresh install it
            # renders a single "nothing to import" line and the user
            # clicks through. (Phase 10.2; v3.0.0 Q49.)
            ("Import",      _STEP_CONTENT,  legacy_import.build(self.ctx)),
        ]
        if not single_preset:
            steps.append(
                ("Preset",  _STEP_CONTENT,  preset_pick.build(self.ctx))
            )
        steps.extend([
            ("Appearance & Desktop", _STEP_CONTENT, appearance.build(self.ctx)),
            ("Hardware",   _STEP_CONTENT,  hardware.build(self.ctx)),
            ("Network",    _STEP_CONTENT,  network.build(self.ctx)),
            # Phase 12.8.4 — capture or generate the shared 16-char
            # mesh passcode before Apply mints the mackesd identity.
            ("Mesh passcode", _STEP_CONTENT, mesh_passcode.build(self.ctx)),
            ("Snapshot",   _STEP_CONTENT,  snapshot.build(self.ctx)),
            ("Review",     _STEP_CONFIRM,  review.build(self.ctx)),
            ("Apply",      _STEP_PROGRESS, self._apply_page),
            ("Welcome to Mackes", _STEP_SUMMARY, self._build_summary()),
        ])
        self._steps = steps
        self._current = 0
        self._apply_started = False

        # ---- Layout ------------------------------------------------------
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._step_strip = self._build_step_strip()
        root.pack_start(self._step_strip, False, False, 0)

        self._content_stack = Gtk.Stack()
        self._content_stack.set_transition_type(Gtk.StackTransitionType.CROSSFADE)
        self._content_stack.set_transition_duration(120)
        for i, (title, kind, widget) in enumerate(steps):
            scroller = Gtk.ScrolledWindow()
            scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
            scroller.add(widget)
            self._content_stack.add_named(scroller, f"step-{i}")
        root.pack_start(self._content_stack, True, True, 0)

        self._bottom_bar = self._build_bottom_bar()
        root.pack_start(self._bottom_bar, False, False, 0)

        self.add(root)
        self._update_for_current_step()

    # ---- step strip ------------------------------------------------------

    def _build_step_strip(self) -> Gtk.Widget:
        outer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        outer.set_margin_top(20); outer.set_margin_bottom(20)
        outer.set_margin_start(40); outer.set_margin_end(40)
        outer.get_style_context().add_class("mackes-shell-header")
        self._step_widgets: List[Gtk.Widget] = []
        for i, (title, _kind, _w) in enumerate(self._steps):
            cell = self._make_step_cell(i, title)
            self._step_widgets.append(cell)
            outer.pack_start(cell, False, False, 0)
        return outer

    def _make_step_cell(self, idx: int, title: str) -> Gtk.Widget:
        cell = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        cell.get_style_context().add_class("mackes-wizard-step")
        num = Gtk.Label(label=str(idx + 1))
        num.get_style_context().add_class("num")
        num.set_size_request(20, 20)
        cell.pack_start(num, False, False, 0)
        lbl = Gtk.Label(label=title)
        lbl.set_xalign(0)
        cell.pack_start(lbl, False, False, 0)
        return cell

    # ---- bottom bar ------------------------------------------------------

    def _build_bottom_bar(self) -> Gtk.Widget:
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        bar.set_margin_top(16); bar.set_margin_bottom(24)
        bar.set_margin_start(40); bar.set_margin_end(40)

        # Back is leftmost
        self._back_btn = Gtk.Button(label="‹ Back")
        self._back_btn.get_style_context().add_class("cds-button-tertiary")
        self._back_btn.connect("clicked", lambda *_: self._navigate(-1))
        self._back_btn.set_tooltip_text("Return to the previous step")
        ax = self._back_btn.get_accessible()
        if ax is not None:
            ax.set_name("Back to previous step")
        bar.pack_start(self._back_btn, False, False, 0)

        # Spacer
        bar.pack_start(Gtk.Box(), True, True, 0)

        # Right cluster: Cancel / Apply / Next
        self._cancel_btn = Gtk.Button(label="Cancel")
        self._cancel_btn.get_style_context().add_class("cds-button-ghost")
        self._cancel_btn.connect("clicked", lambda *_: self.destroy())
        self._cancel_btn.set_tooltip_text("Exit the wizard (Esc)")
        ax = self._cancel_btn.get_accessible()
        if ax is not None:
            ax.set_name("Cancel setup wizard")
        bar.pack_end(self._cancel_btn, False, False, 0)

        self._next_btn = Gtk.Button(label="Next ›")
        self._next_btn.get_style_context().add_class("suggested-action")
        self._next_btn.get_style_context().add_class("cds-button-primary")
        self._next_btn.connect("clicked", lambda *_: self._navigate(+1))
        self._next_btn.set_tooltip_text("Continue to the next step (Enter)")
        self._next_btn.set_can_default(True)
        ax = self._next_btn.get_accessible()
        if ax is not None:
            ax.set_name("Continue to next step")
        bar.pack_end(self._next_btn, False, False, 0)

        return bar

    # ---- navigation ------------------------------------------------------

    def _navigate(self, direction: int) -> None:
        target = self._current + direction
        if target < 0 or target >= len(self._steps):
            return
        self._current = target
        self._content_stack.set_visible_child_name(f"step-{target}")
        self._update_for_current_step()

    def _update_for_current_step(self) -> None:
        title, kind, _ = self._steps[self._current]

        # Step strip — active indicator
        for i, cell in enumerate(self._step_widgets):
            ctx = cell.get_style_context()
            if i == self._current:
                ctx.add_class("active")
            else:
                ctx.remove_class("active")

        # Bottom-bar labels per kind
        self._back_btn.set_sensitive(self._current > 0)
        self._cancel_btn.set_sensitive(kind != _STEP_SUMMARY)

        if kind == _STEP_CONFIRM:
            self._next_btn.set_label("Apply ›")
        elif kind == _STEP_PROGRESS:
            self._next_btn.set_label("Working…")
            self._next_btn.set_sensitive(False)
            if not self._apply_started:
                self._apply_started = True
                # Run on idle so the page is realized before we start writing.
                GLib.idle_add(self._run_apply)
        elif kind == _STEP_SUMMARY:
            self._next_btn.set_label("Finish")
            self._next_btn.set_sensitive(True)
            # On the final step Next = destroy the window.
            self._wire_finish_button()
        else:
            self._next_btn.set_label("Next ›")
            self._next_btn.set_sensitive(True)
            self._unwire_finish_button()

    def _wire_finish_button(self) -> None:
        # Replace the "Next" handler with a finish handler.
        try:
            if hasattr(self, "_next_handler_id"):
                self._next_btn.disconnect(self._next_handler_id)
        except Exception:  # noqa: BLE001
            pass
        self._next_handler_id = self._next_btn.connect(
            "clicked", lambda *_: self.destroy())

    def _unwire_finish_button(self) -> None:
        # Ensure the "Next" button has the standard navigate handler.
        try:
            if hasattr(self, "_next_handler_id"):
                self._next_btn.disconnect(self._next_handler_id)
                del self._next_handler_id
        except Exception:  # noqa: BLE001
            pass

    # ---- apply pipeline --------------------------------------------------

    # ---- keyboard --------------------------------------------------------

    def _on_key_press(self, _widget, event) -> bool:
        # Escape — cancel the wizard from anywhere except mid-install.
        # During Apply the Cancel button on the page itself handles
        # cancellation so we don't tear down the running pipeline.
        if event.keyval == Gdk.KEY_Escape:
            _title, kind, _w = self._steps[self._current]
            if kind != _STEP_PROGRESS:
                self.destroy()
                return True
        return False

    def _run_apply(self) -> bool:
        # `run()` spawns a daemon thread and returns immediately; the
        # callback fires on the GTK main thread once every step actually
        # finishes (or the user cancels mid-run). Until then, Next stays
        # disabled so the user can't skip ahead of the installer.
        self._apply_page.run(on_complete=self._on_apply_complete)
        return False  # one-shot idle

    def _on_apply_complete(self) -> None:
        self._next_btn.set_label("Continue ›")
        self._next_btn.set_sensitive(True)

    # ---- summary page ----------------------------------------------------

    def _build_summary(self) -> Gtk.Widget:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        box.set_margin_top(32); box.set_margin_bottom(32)
        box.set_margin_start(40); box.set_margin_end(40)

        title = Gtk.Label(label="Welcome to Mackes")
        title.set_xalign(0)
        title.get_style_context().add_class("mackes-page-title")
        box.pack_start(title, False, False, 0)

        body = Gtk.Label(label=(
            "Your machine is provisioned. Mackes will open the Dashboard next."
        ))
        body.set_xalign(0); body.set_line_wrap(True)
        body.get_style_context().add_class("mackes-page-subtitle")
        box.pack_start(body, False, False, 0)

        tips_title = Gtk.Label(label="Where to go next")
        tips_title.set_xalign(0)
        tips_title.set_margin_top(20)
        tips_title.get_style_context().add_class("mackes-section-title")
        box.pack_start(tips_title, False, False, 0)

        for line in (
            "Maintain → Snapshots — create restore points before risky changes.",
            "Maintain → Reset to Preset — reapply the preset if drift accumulates.",
            "Tweaks (bottom-right gear) → Re-open Wizard — return to this flow.",
            "Fleet → Inventory — drive ansible playbooks across the mesh.",
            "Mesh Remote — open https://media.mesh/desktop/ in any peer's browser.",
        ):
            row = Gtk.Label(label=f"· {line}")
            row.set_xalign(0); row.set_line_wrap(True)
            row.get_style_context().add_class("mackes-app-desc")
            box.pack_start(row, False, False, 0)

        return box
