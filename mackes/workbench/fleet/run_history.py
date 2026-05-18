"""Fleet → Run history panel (v1.3.0).

Carbon DataTable of every ansible-pull / push run across the mesh.
Filterable by peer, playbook, and time window. Click any row → full
JSON in a Carbon Modal.

Data source: QNM-Shared/.qnm-sync/ansible-runs/<peer>/<ts>.json
Retention:   30 days (managed by mackes.fleet.prune_runs)
"""
from __future__ import annotations

import json
import time
from typing import Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, DataTable, Column,
    Modal, ModalSize, Notification, NotificationKind,
)
from mackes.fleet import (
    RunRecord, build_inventory, list_playbooks, list_runs, prune_runs,
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
    for i, p in enumerate(("Mackes Shell", "Fleet", "Run history")):
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


# ---- panel ----------------------------------------------------------------


class FleetRunHistoryPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._filter_peer: Optional[str] = None
        self._filter_playbook: Optional[str] = None
        self._records: list[RunRecord] = []
        self._build()
        self._refresh()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Run history"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Every playbook that ran on every peer, with whether it "
            "succeeded and what it changed. Click a row for the full "
            "report."
        ), False, False, 0)
        outer.pack_start(_section_description(
            "Old records are cleaned up after 30 days to keep things "
            "tidy."
        ), False, False, 0)

        # Top stats
        self._stats_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL,
                                   spacing=8)
        self._stats_box.set_margin_top(16)
        outer.pack_start(self._stats_box, False, False, 0)

        # Filters
        filter_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        filter_row.set_margin_top(16); filter_row.set_margin_bottom(8)

        lbl1 = Gtk.Label(label="Peer:"); lbl1.set_xalign(0)
        lbl1.get_style_context().add_class("form-label")
        filter_row.pack_start(lbl1, False, False, 0)
        self._peer_combo = Gtk.ComboBoxText()
        self._peer_combo.append_text("All peers")
        self._peer_combo.set_active(0)
        self._peer_combo.connect("changed", self._on_filter_peer)
        filter_row.pack_start(self._peer_combo, False, False, 0)

        lbl2 = Gtk.Label(label="Playbook:"); lbl2.set_xalign(0)
        lbl2.get_style_context().add_class("form-label")
        filter_row.pack_start(lbl2, False, False, 0)
        self._pb_combo = Gtk.ComboBoxText()
        self._pb_combo.append_text("All playbooks")
        self._pb_combo.set_active(0)
        self._pb_combo.connect("changed", self._on_filter_playbook)
        filter_row.pack_start(self._pb_combo, False, False, 0)

        filter_row.pack_end(Button("Prune now", kind=ButtonKind.GHOST,
                                    icon_name="user-trash-symbolic",
                                    on_click=self._on_prune),
                             False, False, 0)
        filter_row.pack_end(Button("Refresh", kind=ButtonKind.GHOST,
                                    icon_name="view-refresh-symbolic",
                                    on_click=self._refresh),
                             False, False, 0)
        outer.pack_start(filter_row, False, False, 0)

        outer.pack_start(_section_title("Runs", meta="newest first"),
                         False, False, 0)
        self._table = DataTable(
            columns=[
                Column(name="status",    title="",          width=24),
                Column(name="when",      title="When",      width=160,
                       monospace=True),
                Column(name="peer",      title="Peer",      width=120),
                Column(name="playbook",  title="Playbook",  width=200),
                Column(name="changed",   title="Changed",   width=72),
                Column(name="ok",        title="OK",        width=60),
                Column(name="failed",    title="Failed",    width=64),
                Column(name="trigger",   title="Trigger",   width=80),
                Column(name="exit",      title="rc",        width=50,
                       monospace=True),
            ],
            searchable=True,
            on_row_activate=self._on_row_activate,
        )
        self._table.set_size_request(-1, 420)
        outer.pack_start(self._table, True, True, 0)

        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- refresh ---------------------------------------------------------

    def _refresh(self) -> None:
        # Re-populate combo boxes from live data
        peers = build_inventory()
        self._reset_combo(self._peer_combo, ["All peers"] + [p.name for p in peers],
                          select=self._filter_peer or "All peers")
        playbooks = list_playbooks()
        self._reset_combo(self._pb_combo, ["All playbooks"] +
                          ["site"] + [p.name for p in playbooks],
                          select=self._filter_playbook or "All playbooks")

        # Load filtered runs
        self._records = list_runs(
            peer=self._filter_peer,
            playbook=self._filter_playbook,
            limit=500,
        )

        # Stats tiles
        for c in list(self._stats_box.get_children()):
            self._stats_box.remove(c)
        total = len(self._records)
        ok = sum(1 for r in self._records if r.exit_code == 0)
        fail = sum(1 for r in self._records if r.exit_code != 0)
        changed = sum(r.changed for r in self._records)
        for label, value, kind in (
            ("Total runs", str(total), None),
            ("Successful", str(ok), "success"),
            ("Failed", str(fail), "error" if fail else None),
            ("Changes applied", str(changed), None),
        ):
            tile = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
            tile.get_style_context().add_class("mackes-stat-tile")
            if kind == "error" and fail:
                tile.get_style_context().add_class("accent")
            tile.set_size_request(-1, 88); tile.set_hexpand(True)
            lab = Gtk.Label(label=label.upper())
            lab.set_xalign(0)
            lab.get_style_context().add_class("mackes-stat-label")
            tile.pack_start(lab, False, False, 0)
            val = Gtk.Label(label=value); val.set_xalign(0)
            val.get_style_context().add_class("mackes-stat-value")
            tile.pack_start(val, True, True, 0)
            self._stats_box.pack_start(tile, True, True, 0)
        self._stats_box.show_all()

        # Table rows
        rows = []
        for r in self._records:
            when = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(r.timestamp))
            rows.append({
                "id":       f"{r.peer}_{int(r.timestamp)}",
                "status":   "●",
                "when":     when,
                "peer":     r.peer,
                "playbook": r.playbook,
                "changed":  str(r.changed),
                "ok":       str(r.ok),
                "failed":   str(r.failed),
                "trigger":  r.triggered_by,
                "exit":     str(r.exit_code),
            })
        self._table.set_rows(rows)
        self._row_index = {f"{r.peer}_{int(r.timestamp)}": r for r in self._records}

    @staticmethod
    def _reset_combo(combo: Gtk.ComboBoxText, items: list[str],
                     *, select: str) -> None:
        combo.handler_block_by_func(None) if False else None  # noop guard
        combo.remove_all()
        idx = 0
        for i, val in enumerate(items):
            combo.append_text(val)
            if val == select:
                idx = i
        combo.set_active(idx)

    # ---- handlers --------------------------------------------------------

    def _on_filter_peer(self, combo: Gtk.ComboBoxText) -> None:
        val = combo.get_active_text() or ""
        self._filter_peer = None if val in ("", "All peers") else val
        self._refresh()

    def _on_filter_playbook(self, combo: Gtk.ComboBoxText) -> None:
        val = combo.get_active_text() or ""
        self._filter_playbook = None if val in ("", "All playbooks") else val
        self._refresh()

    def _on_prune(self) -> None:
        n = prune_runs(days=30)
        self._refresh()

    def _on_row_activate(self, row: dict) -> None:
        rec = self._row_index.get(row.get("id"))
        if rec is None:
            return
        # Show full JSON in a Carbon modal
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        header = Gtk.Label(label=f"{rec.peer}  ·  {rec.playbook}")
        header.set_xalign(0)
        header.get_style_context().add_class("mackes-section-title")
        body.pack_start(header, False, False, 0)
        meta = Gtk.Label(label=(
            f"When: {time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(rec.timestamp))}\n"
            f"Triggered by: {rec.triggered_by}\n"
            f"Exit code: {rec.exit_code}\n"
            f"Duration: {rec.duration_s:.1f}s\n"
            f"Changed: {rec.changed}  ·  OK: {rec.ok}  ·  Failed: {rec.failed}"
        ))
        meta.set_xalign(0)
        meta.get_style_context().add_class("mackes-page-subtitle")
        body.pack_start(meta, False, False, 0)

        log_view = Gtk.TextView()
        log_view.set_editable(False); log_view.set_monospace(True)
        log_view.get_style_context().add_class("mackes-code")
        log_view.get_buffer().set_text(rec.log_tail or "(no log captured)")
        scroll = Gtk.ScrolledWindow()
        scroll.set_min_content_height(280); scroll.set_min_content_width(600)
        scroll.add(log_view)
        body.pack_start(scroll, True, True, 0)

        modal = Modal(self.get_toplevel(), "Run details", body, size=ModalSize.LARGE)
        modal.add_action("Close", kind=ButtonKind.SECONDARY,
                         response_id=Gtk.ResponseType.CLOSE)
        modal.run_then_destroy()
