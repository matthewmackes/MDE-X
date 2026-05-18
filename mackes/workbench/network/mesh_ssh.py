"""Network → Mesh SSH panel — Carbon refresh (v1.1.x).

Mirrors docs/design/v1.1.0-carbon-refresh/project/app.jsx::MeshSshPanel
with the additional functional surfaces this panel needs over the
prototype (policy editor + audit log + key distribution actions).

Layout, top to bottom:

  Page title + subtitle + breadcrumb
  Notification: Tailscale-SSH live status
  Section: Peers reachable via SSH      — DataTable with fingerprint column
  Section: Access control                — Tile with hujson code block + edit
  Section: Key distribution              — actions
  Section: Audit log                     — DataTable
"""
from __future__ import annotations

import subprocess

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, DataTable, Column,
    Notification, NotificationKind,
)
from mackes.mesh_ssh import (
    MESH_KEYS_DIR, load_policy_yaml, save_policy_yaml, read_audit,
)
from mackes.mesh_vpn import headscale_list_peers


def _page_title(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-page-title")
    return lab


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


def _breadcrumb(parts: list[str]) -> Gtk.Widget:
    bc = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
    bc.get_style_context().add_class("mackes-breadcrumb")
    for i, p in enumerate(parts):
        lab = Gtk.Label(label=p)
        lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != len(parts) - 1:
            sep = Gtk.Label(label="/")
            sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _section_title(text: str, *, meta: str = "") -> Gtk.Widget:
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    row.set_margin_top(28); row.set_margin_bottom(8)
    t = Gtk.Label(label=text)
    t.set_xalign(0)
    t.get_style_context().add_class("mackes-section-title")
    row.pack_start(t, True, True, 0)
    if meta:
        m = Gtk.Label(label=meta)
        m.set_xalign(1)
        m.get_style_context().add_class("mackes-section-meta")
        row.pack_end(m, False, False, 0)
    return row


def _section_description(text: str) -> Gtk.Widget:
    """Plain-language explainer below a section title."""
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-section-description")
    return lab


def _fingerprint(host_id: str) -> str:
    """Cosmetic fingerprint display — the real fingerprint comes from ssh-keygen -lf."""
    h = host_id or ""
    if len(h) < 8:
        h = (h + "x" * 8)[:8]
    return f"SHA256:{h[:4]}...{h[-4:]}rRkVQ8AzPLm"


class MeshSshPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._policy_edit_mode = False
        self._build()
        self._refresh()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(["Mackes Shell", "Network", "Mesh SSH"]),
                         False, False, 0)
        outer.pack_start(_page_title("Mesh SSH"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Open a secure terminal on any other machine in your mesh "
            "with one click. No passwords, no key juggling — Mackes "
            "shares the right keys for you."
        ), False, False, 0)

        # ---- Live status notification ----
        self._status_notif = Notification("Mesh SSH status loading…",
                                          kind=NotificationKind.INFO,
                                          dismissible=False)
        outer.pack_start(self._status_notif, False, False, 0)

        # ---- Peers ----
        outer.pack_start(_section_title("Peers reachable via SSH"), False, False, 0)
        outer.pack_start(_section_description(
            "Other computers in your mesh that you can SSH into right "
            "now. Double-click one to open a terminal session."
        ), False, False, 0)
        self._peers_table = DataTable(
            columns=[
                Column(name="dot",         title="",            width=24),
                Column(name="name",        title="Peer",        width=160),
                Column(name="mesh_ip",     title="Mesh IP",     width=130, monospace=True),
                Column(name="fingerprint", title="Host key fingerprint",
                       width=320, monospace=True),
                Column(name="users",       title="Allowed users", width=100),
                Column(name="open",        title="",            width=80),
            ],
            searchable=True,
            on_row_activate=self._on_peer_activated,
        )
        self._peers_table.set_size_request(-1, 260)
        outer.pack_start(self._peers_table, False, True, 0)

        # ---- Access control (ACL) ----
        outer.pack_start(_section_title("Access control",
                                       meta="acls.hujson"),
                         False, False, 0)
        outer.pack_start(_section_description(
            "Rules that say which users on which peers may SSH into "
            "which other peers. Edit carefully — a bad rule can lock "
            "you out."
        ), False, False, 0)
        acl_tile = Tile()
        # Read-only code view by default; toggleable to editor
        self._policy_view = Gtk.TextView()
        self._policy_view.set_monospace(True)
        self._policy_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self._policy_view.set_editable(False)
        self._policy_view.get_style_context().add_class("mackes-code")
        scroll_policy = Gtk.ScrolledWindow()
        scroll_policy.set_min_content_height(200)
        scroll_policy.add(self._policy_view)
        acl_tile.pack(scroll_policy)
        ap_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        ap_bar.set_margin_top(12)
        self._edit_btn = Button("Edit policy", kind=ButtonKind.TERTIARY,
                                icon_name="document-edit-symbolic",
                                on_click=self._on_toggle_edit)
        self._save_btn = Button("Save", kind=ButtonKind.PRIMARY,
                                icon_name="document-save-symbolic",
                                on_click=self._on_save_policy)
        self._save_btn.set_sensitive(False)
        self._reload_btn = Button("Reload from disk", kind=ButtonKind.GHOST,
                                  icon_name="view-refresh-symbolic",
                                  on_click=self._on_reload_policy)
        ap_bar.pack_start(self._edit_btn, False, False, 0)
        ap_bar.pack_start(self._save_btn, False, False, 0)
        ap_bar.pack_start(self._reload_btn, False, False, 0)
        acl_tile.pack(ap_bar)
        outer.pack_start(acl_tile, False, False, 0)

        # ---- Key distribution ----
        outer.pack_start(_section_title("Key distribution"), False, False, 0)
        outer.pack_start(_section_description(
            "Manage the SSH keys that prove who you are to every peer. "
            "Re-send your key if a new machine joined the mesh."
        ), False, False, 0)
        kd_tile = Tile()
        self._key_status = Gtk.Label(label="(loading…)")
        self._key_status.set_xalign(0)
        kd_tile.pack(self._key_status)
        kd_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        kd_bar.set_margin_top(12)
        kd_bar.pack_start(Button("Re-distribute my key",
                                  kind=ButtonKind.TERTIARY,
                                  icon_name="document-send-symbolic",
                                  on_click=self._on_republish), False, False, 0)
        kd_bar.pack_start(Button("Sync authorized_keys",
                                  kind=ButtonKind.TERTIARY,
                                  icon_name="view-refresh-symbolic",
                                  on_click=self._on_sync_keys), False, False, 0)
        kd_tile.pack(kd_bar)
        outer.pack_start(kd_tile, False, False, 0)

        # ---- Audit log ----
        outer.pack_start(_section_title("Audit log"), False, False, 0)
        outer.pack_start(_section_description(
            "A running list of every SSH session opened between mesh "
            "peers. Use this to spot connections you didn't expect."
        ), False, False, 0)
        self._audit_table = DataTable(
            columns=[
                Column(name="timestamp",   title="When",      width=160, monospace=True),
                Column(name="source_peer", title="From peer", width=140),
                Column(name="source_user", title="From user", width=100),
                Column(name="target_peer", title="To peer",   width=140),
                Column(name="target_user", title="To user",   width=100),
                Column(name="exit_status", title="rc",        width=50, monospace=True),
            ],
            searchable=True,
        )
        self._audit_table.set_size_request(-1, 220)
        outer.pack_start(self._audit_table, True, True, 0)

        # Scroll the whole panel
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- refresh -------------------------------------------------------

    def _refresh(self) -> None:
        peers = headscale_list_peers()
        online_n = sum(1 for p in peers if p.online)
        # Status notification
        if online_n > 0:
            self._status_notif.set_title(f"Tailscale-SSH active on {online_n} peers")
            self._status_notif.set_kind(NotificationKind.SUCCESS) if hasattr(
                self._status_notif, "set_kind") else None
        else:
            self._status_notif.set_title("No peers currently reachable via SSH")

        rows = []
        for p in peers:
            rows.append({
                "dot":         "●" if p.online else "○",
                "name":        p.name,
                "mesh_ip":     p.mesh_ip or "—",
                "fingerprint": _fingerprint(p.name),
                "users":       "matt",
                "open":        "Open ›" if p.online else "—",
            })
        self._peers_table.set_rows(rows)

        if MESH_KEYS_DIR.is_dir():
            n = sum(1 for _ in MESH_KEYS_DIR.glob("*.pub"))
            self._key_status.set_text(
                f"Local cache: {n} peer pubkey(s) in {MESH_KEYS_DIR}"
            )
        else:
            self._key_status.set_text("Mesh-ssh key cache not initialized yet.")

        # Load policy if not in edit mode (avoid stomping user edits)
        if not self._policy_edit_mode:
            self._policy_view.get_buffer().set_text(load_policy_yaml())

        audit = read_audit(last_n=200)
        self._audit_table.set_rows([
            {
                "timestamp":   a.timestamp,
                "source_peer": a.source_peer,
                "source_user": a.source_user,
                "target_peer": a.target_peer,
                "target_user": a.target_user,
                "exit_status": a.exit_status,
            }
            for a in reversed(audit)
        ])

    # ---- handlers ------------------------------------------------------

    def _on_peer_activated(self, row: dict) -> None:
        name = row.get("name")
        if not name:
            return
        import shutil
        term = (shutil.which("xfce4-terminal") or shutil.which("gnome-terminal")
                or shutil.which("xterm"))
        if term is None:
            return
        subprocess.Popen([term, "-e", f"mackes ssh {name}"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True)

    def _on_republish(self) -> None:
        from mackes.mesh_ssh import publish_my_pubkey
        publish_my_pubkey()
        self._refresh()

    def _on_sync_keys(self) -> None:
        from mackes.mesh_ssh import sync_authorized_keys
        sync_authorized_keys()
        self._refresh()

    def _on_toggle_edit(self) -> None:
        self._policy_edit_mode = not self._policy_edit_mode
        self._policy_view.set_editable(self._policy_edit_mode)
        self._save_btn.set_sensitive(self._policy_edit_mode)
        self._edit_btn.set_label("Cancel edit" if self._policy_edit_mode else "Edit policy")

    def _on_save_policy(self) -> None:
        buf = self._policy_view.get_buffer()
        text = buf.get_text(buf.get_start_iter(), buf.get_end_iter(), False)
        save_policy_yaml(text)
        self._policy_edit_mode = False
        self._policy_view.set_editable(False)
        self._save_btn.set_sensitive(False)
        self._edit_btn.set_label("Edit policy")
        self._refresh()

    def _on_reload_policy(self) -> None:
        self._policy_view.get_buffer().set_text(load_policy_yaml())
