"""Fleet → Playbooks panel (v1.3.0).

Grid of Carbon tiles, one per role under QNM-Shared/.qnm-sync/playbooks/.
Each tile:
  - name + description
  - last-run summary (timestamp + ok/fail + changed count)
  - YAML preview (read-only, monospace, scrollable)
  - Run-now button (local pull)
  - Open-in-editor button (xdg-open the tasks/main.yml)
"""
from __future__ import annotations

import time
from typing import List, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, Notification, NotificationKind,
)
from mackes.fleet import (
    Playbook, current_peer_name, list_playbooks, list_runs,
    open_playbook_in_editor, run_local_pull,
)


# ---- shared helpers ------------------------------------------------------


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
    for i, p in enumerate(("Mackes Shell", "Fleet", "Playbooks")):
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


def _tag(text: str, kind: str = "neutral") -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.get_style_context().add_class("mackes-tag")
    lab.get_style_context().add_class(kind)
    return lab


def _section_description(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-section-description")
    return lab


def _format_age(ts: Optional[float]) -> str:
    if ts is None:
        return "never"
    delta = int(time.time() - ts)
    if delta < 60: return f"{delta}s ago"
    if delta < 3600: return f"{delta // 60}m ago"
    if delta < 86400: return f"{delta // 3600}h ago"
    return f"{delta // 86400}d ago"


# ---- panel ----------------------------------------------------------------


class FleetPlaybooksPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._build()
        self._refresh()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Playbooks"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Ready-made scripts that configure your mesh peers. Run "
            "one against a peer to apply its changes, or edit the "
            "underlying file to customize what it does."
        ), False, False, 0)
        outer.pack_start(_section_description(
            "Playbooks live in a shared folder visible to every peer, "
            "so an edit on one machine reaches them all."
        ), False, False, 0)

        # Top notification — playbook source location
        self._top_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.pack_start(self._top_box, False, False, 0)

        outer.pack_start(_section_title("Available playbooks",
                                       meta="from QNM-Shared bucket"),
                         False, False, 0)
        self._grid = Gtk.FlowBox()
        self._grid.set_valign(Gtk.Align.START)
        self._grid.set_max_children_per_line(2)
        self._grid.set_min_children_per_line(1)
        self._grid.set_selection_mode(Gtk.SelectionMode.NONE)
        self._grid.set_homogeneous(True)
        self._grid.set_column_spacing(12); self._grid.set_row_spacing(12)
        outer.pack_start(self._grid, True, True, 0)

        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- refresh ---------------------------------------------------------

    def _refresh(self) -> None:
        for c in list(self._top_box.get_children()):
            self._top_box.remove(c)
        playbooks = list_playbooks()
        if not playbooks:
            self._top_box.pack_start(Notification(
                "No playbooks found",
                body="QNM-Shared/.qnm-sync/playbooks/roles/ is empty. The "
                     "wizard's Fleet management birthright step should have "
                     "seeded it — re-run via Maintain → Reset to Preset.",
                kind=NotificationKind.WARNING, dismissible=False,
            ), False, False, 0)
        else:
            self._top_box.pack_start(Notification(
                f"{len(playbooks)} playbook(s) available",
                body="Tag-gated playbooks (never) only fire when explicitly "
                     "requested via the Run-now button or Inventory → SSH push.",
                kind=NotificationKind.INFO, dismissible=False,
            ), False, False, 0)
        self._top_box.show_all()

        # Build run-history index for last-run summary
        recent_by_pb: dict[str, list] = {}
        me = current_peer_name()
        for r in list_runs(peer=me, limit=200):
            recent_by_pb.setdefault(r.playbook, []).append(r)

        for child in list(self._grid.get_children()):
            self._grid.remove(child)
        for pb in playbooks:
            self._grid.add(self._make_card(pb, recent_by_pb.get(pb.name, [])))
        self._grid.show_all()

    def _make_card(self, pb: Playbook, recent: list) -> Gtk.Widget:
        tile = Tile()

        # Header row: name + tag(s)
        head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        name = Gtk.Label(label=pb.name); name.set_xalign(0)
        name.get_style_context().add_class("mackes-app-name")
        head.pack_start(name, True, True, 0)
        for t in pb.tags:
            head.pack_end(_tag(t, "accent" if t == "default" else "neutral"),
                          False, False, 0)
        tile.pack(head)

        # Description
        desc = Gtk.Label(label=pb.description)
        desc.set_xalign(0); desc.set_line_wrap(True)
        desc.set_max_width_chars(80)
        desc.get_style_context().add_class("mackes-app-desc")
        tile.pack(desc)

        # Last-run summary
        if recent:
            last = recent[0]
            kind = "success" if last.exit_code == 0 else "error"
            summary = (
                f"Last run: {_format_age(last.timestamp)}  ·  "
                f"changed={last.changed}  ok={last.ok}  failed={last.failed}"
            )
            run_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            sum_lbl = Gtk.Label(label=summary); sum_lbl.set_xalign(0)
            sum_lbl.get_style_context().add_class("mackes-section-meta")
            run_row.pack_start(sum_lbl, True, True, 0)
            run_row.pack_end(_tag("ok" if last.exit_code == 0 else "fail", kind),
                             False, False, 0)
            tile.pack(run_row)
        else:
            meta = Gtk.Label(label="Last run: never on this peer")
            meta.set_xalign(0)
            meta.get_style_context().add_class("mackes-section-meta")
            tile.pack(meta)

        # YAML preview (read-only, first 16 lines)
        preview_text = self._read_yaml_preview(pb)
        if preview_text:
            preview = Gtk.TextView()
            preview.set_editable(False); preview.set_monospace(True)
            preview.get_style_context().add_class("mackes-code")
            preview.get_buffer().set_text(preview_text)
            preview_scroll = Gtk.ScrolledWindow()
            preview_scroll.set_min_content_height(120)
            preview_scroll.add(preview)
            tile.pack(preview_scroll)

        # Actions
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        bar.set_margin_top(8)
        bar.pack_start(Button("▶ Run now", kind=ButtonKind.PRIMARY,
                               on_click=lambda p=pb: self._on_run(p)),
                        False, False, 0)
        bar.pack_start(Button("Open in editor", kind=ButtonKind.TERTIARY,
                               icon_name="document-edit-symbolic",
                               on_click=lambda p=pb: self._on_edit(p)),
                        False, False, 0)
        tile.pack(bar)

        return tile

    @staticmethod
    def _read_yaml_preview(pb: Playbook) -> str:
        try:
            txt = pb.main_task_path.read_text(encoding="utf-8")
        except OSError:
            return ""
        lines = txt.splitlines()[:16]
        if len(txt.splitlines()) > 16:
            lines.append(f"... ({len(txt.splitlines()) - 16} more lines)")
        return "\n".join(lines)

    # ---- handlers --------------------------------------------------------

    def _on_run(self, pb: Playbook) -> None:
        import threading
        def runner() -> None:
            tags = pb.tags if pb.tags else [pb.name]
            run_local_pull(tags=tags, triggered_by="manual")
            GLib.idle_add(self._refresh)
        threading.Thread(target=runner, daemon=True).start()

    def _on_edit(self, pb: Playbook) -> None:
        open_playbook_in_editor(pb)
