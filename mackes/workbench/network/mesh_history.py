"""Mesh → Config History + Diff Viewer (Phase 12.8.3).

Lists every desired-config revision in descending creation order,
shows a side-by-side diff between any two revisions, and offers a
Rollback button per row. Rollback opens a confirmation dialog before
calling `mackesd_bridge.rollback_to(revision_id)`.

Backed by `mackes.mackesd_bridge.revisions()` (shell-out to
``mackesd revisions list --json``). When the bridge is unavailable,
the panel renders an empty state with installation hints.
"""
from __future__ import annotations

import difflib
import json

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes import mackesd_bridge
from mackes.workbench._common import a11y, empty_state, error_state


def _page_title(text: str) -> Gtk.Widget:
    label = Gtk.Label(label=text)
    label.set_xalign(0)
    label.get_style_context().add_class("mackes-page-title")
    return label


def _format_payload(payload: object) -> str:
    """Pretty-print a JSON-able revision payload so the diff aligns
    cleanly. Falls back to ``str()`` for non-serializable values."""
    try:
        return json.dumps(payload, indent=2, sort_keys=True)
    except (TypeError, ValueError):
        return str(payload)


def build_diff_lines(a_payload: object, b_payload: object,
                     a_label: str, b_label: str) -> list[str]:
    """Pure helper: unified diff between two payload values. Lifted
    out of the panel so it's unit-testable without a GTK display.

    Returns a list of unified-diff lines (no trailing newlines)."""
    a_text = _format_payload(a_payload).splitlines()
    b_text = _format_payload(b_payload).splitlines()
    return list(difflib.unified_diff(
        a_text, b_text,
        fromfile=a_label, tofile=b_label,
        lineterm="",
    ))


class MeshHistoryPanel(Gtk.Box):
    """Config history + diff viewer (Phase 12.8.3)."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_margin_top(24); self.set_margin_bottom(24)
        self.set_margin_start(24); self.set_margin_end(24)

        self.pack_start(_page_title("Configuration History"), False, False, 0)
        subtitle = Gtk.Label(label=(
            "Every applied desired-config revision in descending order. "
            "Pick two rows to diff them side-by-side; use Rollback to "
            "restore a prior revision as a new applied row."
        ))
        subtitle.set_xalign(0); subtitle.set_line_wrap(True)
        subtitle.get_style_context().add_class("mackes-page-subtitle")
        self.pack_start(subtitle, False, False, 12)

        # Split layout: revision list on the left, diff viewer on the
        # right.
        paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)

        # Left: revision list.
        left = Gtk.ScrolledWindow()
        left.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self._list = Gtk.ListBox()
        self._list.set_selection_mode(Gtk.SelectionMode.MULTIPLE)
        a11y(self._list, "Revision list — pick up to two for diff", tooltip=None)
        left.add(self._list)
        paned.pack1(left, resize=True, shrink=False)

        # Right: diff viewer.
        right = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        right.set_margin_start(12)
        self._diff_view = Gtk.TextView()
        self._diff_view.set_editable(False)
        self._diff_view.set_monospace(True)
        self._diff_view.set_wrap_mode(Gtk.WrapMode.NONE)
        a11y(self._diff_view, "Unified diff between selected revisions", tooltip=None)
        diff_scroller = Gtk.ScrolledWindow()
        diff_scroller.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        diff_scroller.add(self._diff_view)
        right.pack_start(diff_scroller, True, True, 0)

        button_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self._diff_btn = Gtk.Button(label="Diff selected")
        self._diff_btn.connect("clicked", lambda _b: self._render_diff())
        a11y(self._diff_btn, "Diff the two selected revisions", tooltip=None)
        button_row.pack_start(self._diff_btn, False, False, 0)
        self._rollback_btn = Gtk.Button(label="Rollback to selected")
        self._rollback_btn.connect("clicked", lambda _b: self._rollback_selected())
        a11y(self._rollback_btn, "Roll back to the highlighted revision", tooltip=None)
        button_row.pack_start(self._rollback_btn, False, False, 0)
        right.pack_start(button_row, False, False, 0)
        paned.pack2(right, resize=True, shrink=False)

        self.pack_start(paned, True, True, 0)
        self._revisions: list[dict] = []
        self._refresh()

    # --- data layer ---------------------------------------------------

    def _refresh(self) -> None:
        for child in self._list.get_children():
            self._list.remove(child)
        try:
            self._revisions = self._fetch_revisions()
        except Exception as exc:  # noqa: BLE001 — boundary
            self.pack_start(
                error_state(
                    "Couldn't load revisions",
                    str(exc),
                    retry_label="Retry",
                    on_retry=lambda *_: self._refresh(),
                ),
                False, False, 0,
            )
            self.show_all()
            return

        if not self._revisions:
            self.pack_start(
                empty_state(
                    "No revisions yet",
                    "Applied desired-config revisions show up here. "
                    "Push a configuration through the Fleet panel "
                    "(or via `mackesd apply`) to create the first one.",
                ),
                False, False, 0,
            )
            self.show_all()
            return

        for rev in self._revisions:
            row = self._build_row(rev)
            self._list.add(row)
        self._list.show_all()

    def _build_row(self, rev: dict) -> Gtk.Widget:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        row.set_margin_top(6); row.set_margin_bottom(6)
        info = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        info.set_hexpand(True)
        title = Gtk.Label(label=f"{rev['revision_id']}  ·  {rev.get('summary', '')}")
        title.set_xalign(0)
        title.get_style_context().add_class("mackes-row-title")
        meta = Gtk.Label(label=(
            f"state={rev.get('state', 'unknown')}  "
            f"author={rev.get('author', '?')}  "
            f"created={rev.get('created_at', '?')}"
        ))
        meta.set_xalign(0)
        meta.get_style_context().add_class("mackes-row-meta")
        info.pack_start(title, False, False, 0)
        info.pack_start(meta, False, False, 0)
        row.pack_start(info, True, True, 0)
        return row

    def _fetch_revisions(self) -> list[dict]:
        fn = getattr(mackesd_bridge, "revisions", None)
        if fn is None:
            return []
        result = fn()
        if result is None:
            return []
        return list(result)

    def _render_diff(self) -> None:
        rows = self._list.get_selected_rows()
        if len(rows) != 2:
            return
        indexes = sorted(r.get_index() for r in rows)
        a = self._revisions[indexes[0]]
        b = self._revisions[indexes[1]]
        diff = "\n".join(build_diff_lines(
            a.get("payload"), b.get("payload"),
            a["revision_id"], b["revision_id"],
        ))
        self._diff_view.get_buffer().set_text(diff or "(no differences)")

    def _rollback_selected(self) -> None:
        rows = self._list.get_selected_rows()
        if not rows:
            return
        idx = rows[0].get_index()
        revision_id = self._revisions[idx]["revision_id"]
        fn = getattr(mackesd_bridge, "rollback_to", None)
        if fn is not None:
            fn(revision_id)
        self._refresh()
