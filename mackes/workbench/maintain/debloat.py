"""Maintain → Debloat levels panel (v1.4.0).

Carbon panel that lets the user pick one of five debloat tiers (L1 Light
→ L5 Viable) and applies the corresponding dnf-remove set plus xfconf
resets. Each tier shows a live preview (packages currently installed vs.
already absent) before the user commits.

Locks v1.4.0 task #95.
"""
from __future__ import annotations

from typing import List, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, Modal, ModalSize,
    Notification, NotificationKind,
)
from mackes.debloat import (
    DebloatLevel, LEVELS, apply_level, describe_level, preview,
)


# ---- shared visual helpers -----------------------------------------------


def _page_title(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text); lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-page-title")
    return lab


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text); lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


def _breadcrumb() -> Gtk.Widget:
    bc = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
    bc.get_style_context().add_class("mackes-breadcrumb")
    for i, p in enumerate(("Mackes Shell", "Maintain", "Debloat levels")):
        lab = Gtk.Label(label=p); lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != 2:
            sep = Gtk.Label(label="/"); sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _section_title(text: str, *, meta: str = "") -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    row.set_margin_top(28); row.set_margin_bottom(8)
    t = Gtk.Label(label=text); t.set_xalign(0)
    t.get_style_context().add_class("mackes-section-title")
    row.pack_start(t, True, True, 0)
    if meta:
        m = Gtk.Label(label=meta); m.set_xalign(1)
        m.get_style_context().add_class("mackes-section-meta")
        row.pack_end(m, False, False, 0)
    return row


def _section_description(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-section-description")
    return lab


def _tag(text: str, kind: str = "neutral") -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.get_style_context().add_class("mackes-tag")
    lab.get_style_context().add_class(kind)
    return lab


# ---- panel ----------------------------------------------------------------


class DebloatPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._selected_level: int = 1
        self._build()
        self._refresh()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Debloat levels"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Strip out the apps and helpers Fedora ships with that you "
            "probably don't use. Pick a level to see exactly what comes "
            "off before you commit."
        ), False, False, 0)
        outer.pack_start(_section_description(
            "Higher levels remove more. Anything you remove can be put "
            "back later with one command — nothing here is permanent."
        ), False, False, 0)

        # Warning notification — this is destructive
        outer.pack_start(Notification(
            "Take a snapshot first",
            body="Debloat is destructive. Open Maintain → Snapshots → "
                 "Create restore point before applying any level above L2.",
            kind=NotificationKind.WARNING, dismissible=False,
        ), False, False, 0)

        # ---- Level picker ----
        outer.pack_start(_section_title("Pick a level",
                                       meta="cumulative — L3 includes L1 + L2"),
                         False, False, 0)
        self._level_buttons = {}
        first_radio: Optional[Gtk.RadioButton] = None
        for lvl in LEVELS:
            row = self._make_level_row(lvl, first_radio)
            if first_radio is None and isinstance(row, tuple):
                first_radio = row[1]
                outer.pack_start(row[0], False, False, 0)
            else:
                outer.pack_start(row, False, False, 0)

        # ---- Preview ----
        outer.pack_start(_section_title("Preview", meta="what would happen"),
                         False, False, 0)
        self._preview_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL,
                                     spacing=8)
        outer.pack_start(self._preview_box, False, False, 0)

        # ---- Apply ----
        outer.pack_start(_section_title("Apply"), False, False, 0)
        apply_tile = Tile()
        info = Gtk.Label(label=(
            "Applying will run `dnf remove -y <packages>` via pkexec. "
            "You'll be prompted for the admin password. The action is logged "
            "to ~/.local/share/mackes-shell/logs/mackes.log."
        ))
        info.set_xalign(0); info.set_line_wrap(True)
        info.get_style_context().add_class("mackes-page-subtitle")
        apply_tile.pack(info)
        apply_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        apply_row.pack_start(
            Button("Apply debloat level", kind=ButtonKind.DANGER,
                   icon_name="user-trash-symbolic",
                   on_click=self._on_apply),
            False, False, 0)
        apply_row.pack_start(
            Button("Open snapshots first", kind=ButtonKind.TERTIARY,
                   icon_name="document-revert-symbolic",
                   on_click=self._on_open_snapshots),
            False, False, 0)
        apply_tile.pack(apply_row)
        outer.pack_start(apply_tile, False, False, 0)

        # ---- Log ----
        outer.pack_start(_section_title("Log"), False, False, 0)
        self._log = Gtk.TextView()
        self._log.set_editable(False); self._log.set_monospace(True)
        self._log.get_style_context().add_class("mackes-code")
        self._log.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        log_scroll = Gtk.ScrolledWindow()
        log_scroll.set_min_content_height(140)
        log_scroll.add(self._log)
        outer.pack_start(log_scroll, False, False, 0)

        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    def _make_level_row(self, lvl: DebloatLevel,
                         group: Optional[Gtk.RadioButton]):
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        row.set_margin_top(4); row.set_margin_bottom(4)

        if group is None:
            radio = Gtk.RadioButton.new(None)
        else:
            radio = Gtk.RadioButton.new_from_widget(group)
        if lvl.n == self._selected_level:
            radio.set_active(True)
        radio.connect("toggled", self._on_level_toggled, lvl.n)
        row.pack_start(radio, False, False, 0)

        text_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        title = Gtk.Label(label=f"L{lvl.n}  ·  {lvl.name}")
        title.set_xalign(0)
        title.get_style_context().add_class("mackes-section-title")
        head.pack_start(title, False, False, 0)
        kind = {1: "success", 2: "info", 3: "warning",
                4: "warning", 5: "error"}[lvl.n]
        head.pack_start(_tag(f"{len(lvl.packages)} pkg(s)", kind),
                        False, False, 0)
        text_col.pack_start(head, False, False, 0)
        blurb = Gtk.Label(label=lvl.blurb)
        blurb.set_xalign(0); blurb.set_line_wrap(True)
        blurb.get_style_context().add_class("mackes-app-desc")
        text_col.pack_start(blurb, False, False, 0)
        row.pack_start(text_col, True, True, 0)

        if group is None:
            return row, radio
        return row

    def _refresh(self) -> None:
        # Preview the currently-selected level
        for c in list(self._preview_box.get_children()):
            self._preview_box.remove(c)
        try:
            p = preview(self._selected_level)
        except ValueError:
            return

        lvl: DebloatLevel = p["level"]
        # Description tile
        desc_tile = Tile()
        desc = Gtk.Label(label=lvl.description)
        desc.set_xalign(0); desc.set_line_wrap(True)
        desc.set_max_width_chars(100)
        desc.get_style_context().add_class("mackes-page-subtitle")
        desc_tile.pack(desc)
        if lvl.notes:
            for n in lvl.notes:
                note = Gtk.Label(label=f"·  {n}")
                note.set_xalign(0); note.set_line_wrap(True)
                note.get_style_context().add_class("mackes-section-meta")
                desc_tile.pack(note)
        self._preview_box.pack_start(desc_tile, False, False, 0)

        # Stats row
        stats_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        for label, value, kind in (
            ("Will remove", str(len(p["removable"])), "warning"),
            ("Already absent", str(len(p["absent"])), None),
            ("xfconf resets", str(len(p["xfconf_resets"])), None),
        ):
            tile = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
            tile.get_style_context().add_class("mackes-stat-tile")
            if kind == "warning" and value != "0":
                tile.get_style_context().add_class("accent")
            tile.set_hexpand(True); tile.set_size_request(-1, 80)
            l = Gtk.Label(label=label.upper())
            l.set_xalign(0)
            l.get_style_context().add_class("mackes-stat-label")
            tile.pack_start(l, False, False, 0)
            v = Gtk.Label(label=value); v.set_xalign(0)
            v.get_style_context().add_class("mackes-stat-value")
            tile.pack_start(v, True, True, 0)
            stats_row.pack_start(tile, True, True, 0)
        self._preview_box.pack_start(stats_row, False, False, 0)

        # Package list (in a code-style block)
        if p["removable"]:
            removable_tile = Tile()
            head = Gtk.Label(label="Packages to remove:")
            head.set_xalign(0)
            head.get_style_context().add_class("mackes-section-title")
            removable_tile.pack(head)
            pkgs_view = Gtk.TextView()
            pkgs_view.set_editable(False); pkgs_view.set_monospace(True)
            pkgs_view.get_style_context().add_class("mackes-code")
            pkgs_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
            pkgs_view.get_buffer().set_text("\n".join(p["removable"]))
            sc = Gtk.ScrolledWindow()
            sc.set_min_content_height(120)
            sc.add(pkgs_view)
            removable_tile.pack(sc)
            self._preview_box.pack_start(removable_tile, False, False, 0)

        self._preview_box.show_all()

    # ---- handlers --------------------------------------------------------

    def _on_level_toggled(self, btn: Gtk.RadioButton, n: int) -> None:
        if btn.get_active():
            self._selected_level = n
            self._refresh()

    def _on_open_snapshots(self) -> None:
        win = self.get_toplevel()
        if hasattr(win, "go_to"):
            win.go_to("snapshots")

    def _on_apply(self) -> None:
        lvl = describe_level(self._selected_level)
        p = preview(self._selected_level)
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        msg = Gtk.Label(label=(
            f"Apply L{lvl.n} {lvl.name}?\n\n"
            f"This will run `dnf remove -y` on {len(p['removable'])} "
            f"package(s). The action is logged and the dnf transaction "
            f"is itself reversible via `dnf history undo`.\n\n"
            f"If you haven't taken a snapshot, click Cancel and open "
            f"Maintain → Snapshots first."
        ))
        msg.set_xalign(0); msg.set_line_wrap(True)
        body.pack_start(msg, False, False, 0)
        modal = Modal(self.get_toplevel(),
                      f"Confirm L{lvl.n} debloat", body, size=ModalSize.MEDIUM)
        def _go() -> None:
            self._run_apply()
        modal.add_action("Cancel", kind=ButtonKind.SECONDARY,
                         response_id=Gtk.ResponseType.CANCEL)
        modal.add_action(f"Apply L{lvl.n}", kind=ButtonKind.DANGER,
                         on_click=_go,
                         response_id=Gtk.ResponseType.OK)
        modal.run_then_destroy()

    def _run_apply(self) -> None:
        import threading
        n = self._selected_level
        self._append_log(f"→  Applying L{n}…")
        def runner() -> None:
            try:
                lines = apply_level(n)
            except Exception as e:  # noqa: BLE001
                lines = [f"error: {e}"]
            GLib.idle_add(self._after_apply, lines)
        threading.Thread(target=runner, daemon=True).start()

    def _after_apply(self, lines: List[str]) -> bool:
        for line in lines:
            self._append_log(f"   {line}")
        self._refresh()
        return False

    def _append_log(self, text: str) -> None:
        buf = self._log.get_buffer()
        end = buf.get_end_iter()
        buf.insert(end, text + "\n")
        end = buf.get_end_iter()
        self._log.scroll_to_iter(end, 0, False, 0, 1)
