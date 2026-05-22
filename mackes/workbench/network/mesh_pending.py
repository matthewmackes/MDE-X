"""Mesh → Pending Changes inbox (Phase 12.8.2).

Surfaces every desired-config revision whose `state` column is
``draft`` (awaiting operator approval). The reconcile worker only
applies revisions in state `approved` or later, so this panel is the
operator's "what's queued?" view.

Backed by `mackes.mackesd_bridge.pending_changes()` (shell-out to
``mackesd pending-changes --json``). The bridge degrades gracefully
when mackesd isn't installed yet — the panel renders an empty state
explaining why.
"""
from __future__ import annotations

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes import mackesd_bridge  # noqa: F401 — used at runtime via getattr
from mackes.workbench._common import a11y, empty_state, error_state


def _section_header(text: str) -> Gtk.Widget:
    label = Gtk.Label(label=text)
    label.set_xalign(0)
    label.get_style_context().add_class("mackes-section-title")
    return label


def _page_title(text: str) -> Gtk.Widget:
    label = Gtk.Label(label=text)
    label.set_xalign(0)
    label.get_style_context().add_class("mackes-page-title")
    return label


def _pill(text: str, css_class: str) -> Gtk.Widget:
    label = Gtk.Label(label=text)
    label.set_xalign(0.5)
    label.get_style_context().add_class(css_class)
    return label


def _row(revision_id: str, author: str, summary: str, created_at: str,
         on_approve, on_reject) -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    row.set_margin_top(8); row.set_margin_bottom(8)
    row.get_style_context().add_class("mackes-row")

    left = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
    left.set_hexpand(True)
    title = Gtk.Label(label=f"{revision_id}  ·  {summary}")
    title.set_xalign(0)
    title.get_style_context().add_class("mackes-row-title")
    meta = Gtk.Label(label=f"by {author} on {created_at}")
    meta.set_xalign(0)
    meta.get_style_context().add_class("mackes-row-meta")
    left.pack_start(title, False, False, 0)
    left.pack_start(meta, False, False, 0)
    row.pack_start(left, True, True, 0)

    row.pack_start(_pill("DRAFT", "mackes-pill-neutral"), False, False, 0)

    approve_btn = Gtk.Button(label="Approve")
    approve_btn.get_style_context().add_class("mackes-button-primary")
    approve_btn.connect("clicked", lambda _btn: on_approve(revision_id))
    a11y(approve_btn, f"Approve revision {revision_id}", tooltip=None)
    row.pack_start(approve_btn, False, False, 0)

    reject_btn = Gtk.Button(label="Reject")
    reject_btn.get_style_context().add_class("mackes-button-secondary")
    reject_btn.connect("clicked", lambda _btn: on_reject(revision_id))
    a11y(reject_btn, f"Reject revision {revision_id}", tooltip=None)
    row.pack_start(reject_btn, False, False, 0)

    return row


class MeshPendingPanel(Gtk.Box):
    """Pending-changes inbox panel (Phase 12.8.2)."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_margin_top(24); self.set_margin_bottom(24)
        self.set_margin_start(24); self.set_margin_end(24)

        self.pack_start(_page_title("Pending Changes"), False, False, 0)
        subtitle = Gtk.Label(label=(
            "Configuration drafts waiting on operator approval. "
            "Approved revisions enter the reconcile queue and apply "
            "on every peer within one tick."
        ))
        subtitle.set_xalign(0); subtitle.set_line_wrap(True)
        subtitle.get_style_context().add_class("mackes-page-subtitle")
        self.pack_start(subtitle, False, False, 12)

        self._list = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        self.pack_start(self._list, True, True, 0)

        self._refresh()

    # --- data layer ----------------------------------------------------

    def _refresh(self) -> None:
        for child in self._list.get_children():
            self._list.remove(child)
        try:
            entries = self._fetch_pending()
        except Exception as exc:  # noqa: BLE001 — boundary
            self._list.pack_start(
                error_state(
                    "Couldn't load pending changes",
                    str(exc),
                    retry_label="Retry",
                    on_retry=lambda *_: self._refresh(),
                ),
                False, False, 0,
            )
            self._list.show_all()
            return

        if not entries:
            self._list.pack_start(
                empty_state(
                    "No pending changes",
                    "All configuration revisions are applied. New "
                    "drafts will appear here for approval.",
                ),
                False, False, 0,
            )
            self._list.show_all()
            return

        for entry in entries:
            self._list.pack_start(
                _row(
                    entry["revision_id"],
                    entry["author"],
                    entry["summary"],
                    entry["created_at"],
                    self._approve,
                    self._reject,
                ),
                False, False, 0,
            )
        self._list.show_all()

    def _fetch_pending(self) -> list[dict[str, str]]:
        """Returns the pending-changes list from mackesd, or [] when
        the bridge is unavailable. Falls back to an empty list rather
        than raising so the UI renders the empty state."""
        fetch = getattr(mackesd_bridge, "pending_changes", None)
        if fetch is None:
            return []
        result = fetch()
        if result is None:
            return []
        return list(result)

    def _approve(self, revision_id: str) -> None:
        fn = getattr(mackesd_bridge, "approve_revision", None)
        if fn is not None:
            fn(revision_id)
        GLib.idle_add(self._refresh)

    def _reject(self, revision_id: str) -> None:
        fn = getattr(mackesd_bridge, "reject_revision", None)
        if fn is not None:
            fn(revision_id)
        GLib.idle_add(self._refresh)
