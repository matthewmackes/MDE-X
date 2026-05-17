"""Fleet → Inventory panel (v1.3.0).

Live roster of every mesh peer. Each row shows:
  - status dot (online / offline)
  - peer name + mesh IP
  - last pull timestamp + ok/fail
  - pulls in the last 24h
  - per-row checkbox for multi-select ad-hoc runs

The action bar offers "Run playbook on selection" (Q10 lock — SSH push)
plus "Run on this peer only" (local pull, no SSH).
"""
from __future__ import annotations

import time
from typing import List, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, DataTable, Column,
    Modal, ModalSize, Notification, NotificationKind,
)
from mackes.fleet import (
    FleetPeer, Playbook, build_inventory, current_peer_name,
    list_playbooks, run_local_pull, run_push,
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
    for i, p in enumerate(("Mackes Shell", "Fleet", "Inventory")):
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


def _format_age(ts: Optional[float]) -> str:
    if ts is None:
        return "never"
    delta = int(time.time() - ts)
    if delta < 60:
        return f"{delta}s ago"
    if delta < 3600:
        return f"{delta // 60}m ago"
    if delta < 86400:
        return f"{delta // 3600}h ago"
    return f"{delta // 86400}d ago"


# ---- panel ----------------------------------------------------------------


class FleetInventoryPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._selected: set[str] = set()
        self._build()
        self._refresh()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(32); outer.set_margin_bottom(32)
        outer.set_margin_start(40); outer.set_margin_end(40)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Inventory"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Every peer in the mesh and its fleet-management posture. "
            "Each peer runs ansible-pull on a 30-min timer; this panel adds "
            "ad-hoc SSH-push to selected peers for one-off runs."
        ), False, False, 0)

        # ---- Live status notification ----
        self._status_notif_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL,
                                          spacing=0)
        outer.pack_start(self._status_notif_box, False, False, 0)

        # ---- Action row ----
        action_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        action_row.set_margin_top(16); action_row.set_margin_bottom(8)
        action_row.pack_start(
            Button("Run playbook on selection", kind=ButtonKind.PRIMARY,
                   icon_name="media-playback-start-symbolic",
                   on_click=self._on_push_selection),
            False, False, 0)
        action_row.pack_start(
            Button("Pull on this peer", kind=ButtonKind.TERTIARY,
                   icon_name="view-refresh-symbolic",
                   on_click=self._on_pull_local),
            False, False, 0)
        action_row.pack_start(
            Button("Select all online", kind=ButtonKind.GHOST,
                   on_click=self._on_select_all),
            False, False, 0)
        action_row.pack_start(
            Button("Clear", kind=ButtonKind.GHOST,
                   on_click=self._on_clear_selection),
            False, False, 0)
        outer.pack_start(action_row, False, False, 0)

        outer.pack_start(_section_title("Peers"), False, False, 0)

        # ListBox so we can put a real checkbox per row (DataTable doesn't
        # support row-level Gtk widgets cleanly).
        self._listbox = Gtk.ListBox()
        self._listbox.set_selection_mode(Gtk.SelectionMode.NONE)
        self._listbox.get_style_context().add_class("mackes-dt")
        list_scroll = Gtk.ScrolledWindow()
        list_scroll.set_min_content_height(380)
        list_scroll.add(self._listbox)
        outer.pack_start(list_scroll, True, True, 0)

        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- refresh ---------------------------------------------------------

    def _refresh(self) -> None:
        peers = build_inventory()

        # Status notification
        for c in list(self._status_notif_box.get_children()):
            self._status_notif_box.remove(c)
        online = sum(1 for p in peers if p.online)
        ok_24h = sum(1 for p in peers if p.last_pull_ok is True)
        if online == len(peers) and ok_24h > 0:
            self._status_notif_box.pack_start(Notification(
                f"Fleet live — {online}/{len(peers)} peers online · "
                f"{ok_24h} successful pulls in window",
                body="Every peer is converging via ansible-pull. Use the "
                     "ad-hoc 'Run on selection' button for one-off drift "
                     "correction.",
                kind=NotificationKind.SUCCESS, dismissible=False,
            ), False, False, 0)
        else:
            self._status_notif_box.pack_start(Notification(
                f"Fleet degraded — {online}/{len(peers)} peers reachable",
                body="Offline peers will catch up on their next pull.",
                kind=NotificationKind.WARNING, dismissible=False,
            ), False, False, 0)
        self._status_notif_box.show_all()

        # Refill peer list
        for child in list(self._listbox.get_children()):
            self._listbox.remove(child)
        me = current_peer_name()
        for peer in peers:
            self._listbox.add(self._make_peer_row(peer, is_self=(peer.name == me)))
        self._listbox.show_all()
        self._peers = peers

    def _make_peer_row(self, peer: FleetPeer, *, is_self: bool) -> Gtk.Widget:
        row = Gtk.ListBoxRow()
        row.get_style_context().add_class("mackes-side-nav-item")

        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        box.set_margin_top(8); box.set_margin_bottom(8)
        box.set_margin_start(16); box.set_margin_end(16)

        # Multi-select checkbox
        chk = Gtk.CheckButton()
        chk.set_active(peer.name in self._selected)
        chk.connect("toggled", self._on_check_toggled, peer.name)
        box.pack_start(chk, False, False, 0)

        # Status dot
        dot = Gtk.Label(label="●" if peer.online else "○")
        dot.get_style_context().add_class("mackes-dot")
        dot.get_style_context().add_class("ok" if peer.online else "fail")
        box.pack_start(dot, False, False, 0)

        # Name + (this peer)
        text_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        name_line = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        name = Gtk.Label(label=peer.name); name.set_xalign(0)
        name.get_style_context().add_class("mackes-section-title")
        name_line.pack_start(name, False, False, 0)
        if is_self:
            name_line.pack_start(_tag("this peer", "accent"), False, False, 0)
        text_col.pack_start(name_line, False, False, 0)
        meta = Gtk.Label(label=f"{peer.mesh_ip or '—'}  ·  "
                               f"last pull {_format_age(peer.last_pull_at)}  ·  "
                               f"{peer.pulls_24h} pulls/24h")
        meta.set_xalign(0)
        meta.get_style_context().add_class("mackes-section-meta")
        text_col.pack_start(meta, False, False, 0)
        box.pack_start(text_col, True, True, 0)

        # Status tag
        if peer.last_pull_ok is True:
            box.pack_end(_tag("ok", "success"), False, False, 0)
        elif peer.last_pull_ok is False:
            box.pack_end(_tag("failed", "error"), False, False, 0)
        else:
            box.pack_end(_tag("never run", "neutral"), False, False, 0)

        row.add(box)
        return row

    # ---- handlers --------------------------------------------------------

    def _on_check_toggled(self, btn: Gtk.CheckButton, peer_name: str) -> None:
        if btn.get_active():
            self._selected.add(peer_name)
        else:
            self._selected.discard(peer_name)

    def _on_select_all(self) -> None:
        self._selected = {p.name for p in self._peers if p.online}
        self._refresh()

    def _on_clear_selection(self) -> None:
        self._selected = set()
        self._refresh()

    def _on_pull_local(self) -> None:
        # Modal: choose playbook tags (or full site.yml)
        self._show_playbook_picker(
            title="Run on this peer",
            sub="Triggers a local ansible-pull cycle. Equivalent to letting "
                "the 30-min timer fire now.",
            run_cb=self._do_local_pull,
        )

    def _on_push_selection(self) -> None:
        if not self._selected:
            return
        targets_str = ", ".join(sorted(self._selected))
        self._show_playbook_picker(
            title=f"Run on selection ({len(self._selected)} peers)",
            sub=f"Targets: {targets_str}\n\n"
                "SSH-push via Tailscale-SSH identity. Both push and pull "
                "paths write to the same run-history bucket.",
            run_cb=self._do_push_selection,
        )

    def _show_playbook_picker(self, *, title: str, sub: str, run_cb) -> None:
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        msg = Gtk.Label(label=sub); msg.set_xalign(0); msg.set_line_wrap(True)
        body.pack_start(msg, False, False, 0)
        pb_combo = Gtk.ComboBoxText()
        pb_combo.append_text("(default site.yml — drift correction)")
        for pb in list_playbooks():
            pb_combo.append_text(pb.name)
        pb_combo.set_active(0)
        body.pack_start(pb_combo, False, False, 0)

        modal = Modal(self.get_toplevel(), title, body, size=ModalSize.MEDIUM)
        def _go() -> None:
            sel = pb_combo.get_active_text() or ""
            if sel.startswith("(default"):
                tags = None
            else:
                tags = _tags_for_playbook(sel)
            run_cb(tags)
        modal.add_action("Cancel", kind=ButtonKind.SECONDARY,
                         response_id=Gtk.ResponseType.CANCEL)
        modal.add_action("Run", kind=ButtonKind.PRIMARY,
                         on_click=_go,
                         response_id=Gtk.ResponseType.OK)
        modal.run_then_destroy()

    def _do_local_pull(self, tags: Optional[List[str]]) -> None:
        import threading
        def runner() -> None:
            run_local_pull(tags=tags, triggered_by="manual")
            GLib.idle_add(self._refresh)
        threading.Thread(target=runner, daemon=True).start()

    def _do_push_selection(self, tags: Optional[List[str]]) -> None:
        if not self._selected:
            return
        peers = sorted(self._selected)
        import threading
        def runner() -> None:
            run_push(peers, tags=tags)
            GLib.idle_add(self._refresh)
        threading.Thread(target=runner, daemon=True).start()


def _tags_for_playbook(name: str) -> List[str]:
    """Map a playbook display name to its `--tags` value."""
    from mackes.fleet import _tags_for
    return _tags_for(name)
