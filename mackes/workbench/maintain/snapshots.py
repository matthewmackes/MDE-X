"""Maintain → Snapshots — Carbon refresh (v1.1.1).

Mirrors docs/design/v1.1.0-carbon-refresh/project/panels-b.jsx::SnapshotsPanel:
  - Breadcrumb + page title + subtitle
  - "Create" section: label input + Carbon primary button inside a tile,
    followed by a helper line listing exactly what gets captured
  - "Existing" section: Carbon DataTable (Label / Created / Source preset /
    Size) with per-row Restore + Delete ghost buttons; Restore opens a
    confirm modal.

Q10 lock: manual snapshots only — no auto-snapshotting.
"""
from __future__ import annotations

import os
from typing import List

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, DataTable, Column,
    Modal, ModalSize, Notification, NotificationKind,
)
from mackes.snapshots import (
    Snapshot, create_snapshot, delete_snapshot, list_snapshots, restore_snapshot,
)
from mackes.state import MackesState


# ---- helpers --------------------------------------------------------------


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
    for i, p in enumerate(("Mackes Shell", "Maintain", "Snapshots")):
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


def _dir_size_bytes(path) -> int:
    total = 0
    try:
        for root, _dirs, files in os.walk(path):
            for f in files:
                try:
                    total += os.path.getsize(os.path.join(root, f))
                except OSError:
                    continue
    except OSError:
        pass
    return total


def _format_size(n: int) -> str:
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if n < 1024:
            return f"{n:.0f} {unit}" if unit == "B" else f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} PB"


# ---- panel ----------------------------------------------------------------


class SnapshotsPanel(Gtk.Box):
    def __init__(self, state: MackesState) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.state = state
        self._snap_index: dict[str, Snapshot] = {}
        self._build()
        self._refresh()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(32); outer.set_margin_bottom(32)
        outer.set_margin_start(40); outer.set_margin_end(40)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Snapshots"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Restore points capture your live config (xfconf channels + "
            "xfce4-panel + theme stack + mesh state + ~/.config/mackes-shell/) "
            "into a timestamped directory. Take a snapshot before risky changes."
        ), False, False, 0)

        # ---- Create section ----
        outer.pack_start(_section_title("Create"), False, False, 0)
        create_tile = Tile()
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self._label = Gtk.Entry()
        self._label.set_placeholder_text("Optional label — e.g. before-theme-swap")
        self._label.set_hexpand(True)
        row.pack_start(self._label, True, True, 0)
        create_btn = Button("Create restore point", kind=ButtonKind.PRIMARY,
                            icon_name="document-revert-symbolic",
                            on_click=self._on_create)
        row.pack_start(create_btn, False, False, 0)
        create_tile.pack(row)
        helper = Gtk.Label(label=(
            "Captures xfconf channels · panel layout · theme stack · mesh state · "
            "~/.config/mackes-shell/ · ~/.local/share/mackes-shell/"
        ))
        helper.set_xalign(0); helper.set_line_wrap(True)
        helper.get_style_context().add_class("mackes-section-meta")
        create_tile.pack(helper)
        self._status = Gtk.Label(label="")
        self._status.set_xalign(0)
        self._status.get_style_context().add_class("dim-label")
        create_tile.pack(self._status)
        outer.pack_start(create_tile, False, False, 0)

        # ---- Existing section ----
        self._snap_meta = _section_title("Existing", meta="loading…")
        outer.pack_start(self._snap_meta, False, False, 0)

        self._table = DataTable(
            columns=[
                Column(name="label",   title="Label",        width=240),
                Column(name="created", title="Created",      width=180,
                       monospace=True),
                Column(name="preset",  title="From preset",  width=140),
                Column(name="size",    title="Size",         width=100,
                       monospace=True),
                Column(name="actions", title="",             width=200),
            ],
            searchable=True,
            on_row_activate=self._on_row_activate,
        )
        self._table.set_size_request(-1, 320)
        outer.pack_start(self._table, True, True, 0)

        # Scroll the whole panel
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- handlers ---------------------------------------------------------

    def _on_create(self) -> None:
        label = self._label.get_text().strip() or "snapshot"
        try:
            snap = create_snapshot(label=label, source_preset=self.state.active_preset)
        except Exception as e:  # noqa: BLE001
            self._status.set_text(f"Create failed: {e}")
            return
        self._label.set_text("")
        self._status.set_text(f"Created: {snap.name}")
        self._refresh()

    def _on_row_activate(self, row: dict) -> None:
        # Row "activate" = double-click → show restore-confirm
        snap = self._snap_index.get(row.get("name"))
        if snap is not None:
            self._on_restore(snap)

    def _on_restore(self, snap: Snapshot) -> None:
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        msg = Gtk.Label(label=(
            f"Restore snapshot {snap.display_label()}?\n\n"
            "This wipes your current xfce4-panel + theme config and replays "
            "the captured one. xfconf channels are reloaded. Continue?"
        ))
        msg.set_xalign(0); msg.set_line_wrap(True)
        body.pack_start(msg, False, False, 0)
        modal = Modal(self.get_toplevel(), "Restore snapshot",
                      body, size=ModalSize.MEDIUM)
        modal.add_action("Cancel", kind=ButtonKind.SECONDARY,
                         response_id=Gtk.ResponseType.CANCEL)
        modal.add_action("Restore", kind=ButtonKind.PRIMARY,
                         on_click=lambda: self._do_restore(snap),
                         response_id=Gtk.ResponseType.OK)
        modal.run_then_destroy()

    def _do_restore(self, snap: Snapshot) -> None:
        try:
            actions = restore_snapshot(snap)
        except Exception as e:  # noqa: BLE001
            self._status.set_text(f"Restore failed: {e}")
            return
        self._status.set_text(actions[-1] if actions else f"Restored {snap.name}")
        self._refresh()

    def _on_delete(self, snap: Snapshot) -> None:
        try:
            delete_snapshot(snap)
        except Exception as e:  # noqa: BLE001
            self._status.set_text(f"Delete failed: {e}")
            return
        self._status.set_text(f"Deleted: {snap.name}")
        self._refresh()

    # ---- refresh ----------------------------------------------------------

    def _refresh(self) -> None:
        snaps = list_snapshots()
        self._snap_index = {snap.name: snap for snap in snaps}
        rows = []
        total_bytes = 0
        for snap in snaps:
            mf = snap.manifest()
            try:
                size = _dir_size_bytes(snap.path)
            except Exception:  # noqa: BLE001
                size = 0
            total_bytes += size
            rows.append({
                "name":    snap.name,
                "label":   snap.display_label(),
                "created": snap.created.strftime("%Y-%m-%d %H:%M:%S"),
                "preset":  mf.get("source_preset") or "—",
                "size":    _format_size(size),
                "actions": "Restore · Delete",
            })
        self._table.set_rows(rows)

        # Update section meta (count + total size)
        for child in list(self._snap_meta.get_children()):
            if isinstance(child, Gtk.Label) and "mackes-section-meta" in (
                child.get_style_context().list_classes() or []
            ):
                child.set_text(f"{len(snaps)} snapshots · {_format_size(total_bytes)} total")
                break
