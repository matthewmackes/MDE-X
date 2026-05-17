"""Network → Mesh Remote panel — full configuration GUI (v1.2.0).

Carbon panel covering the full surface of the v1.2.0 remote-desktop
birthright. Mirrors the patterns established in mesh_vpn.py / mesh_ssh.py
/ mesh_services.py:

  * Breadcrumb + page title + subtitle
  * Live status Notification (success / warning)
  * Section divider (mackes-section-title + mackes-section-meta)
  * Form rows with Switch / ComboBox / Entry + helper text
  * DataTable with per-row action buttons
  * Modals for confirm / rename
  * Tiles for grouped settings

Sections, top to bottom:

  1. Status              — live posture summary + service health grid
  2. Display sharing     — x11vnc daemon controls (live :0 mirror)
  3. RDP server          — xrdp daemon controls (new XFCE session)
  4. Gateway             — Guacamole web app + Open-in-browser button
  5. Connections         — DataTable of auto-discovered RDP/VNC entries
                            with per-row Favorite / Hide / Rename
  6. Auto-discovery      — sync interval, manual resync, last-sync time
  7. Diagnostics         — pkexec'd systemctl status text for each daemon
"""
from __future__ import annotations

import json
import subprocess
import time
from pathlib import Path
from typing import Dict, List, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.carbon import (
    Button, ButtonKind, Tile, DataTable, Column,
    Modal, ModalSize, Notification, NotificationKind,
)
from mackes.remote_desktop import (
    NOAUTH_CONFIG_PATH, Overrides, ResolvedConnection, active_connections,
    load_overrides, rebuild_connections, save_overrides, service_status,
)
from mackes.state import CONFIG_DIR


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
    for i, p in enumerate(("Mackes Shell", "Network", "Mesh Remote")):
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


def _form_row(label: str, *, helper: str = "",
              control: Optional[Gtk.Widget] = None) -> Gtk.Widget:
    """Form row matching the design's .form-row / .form-label / .form-helper."""
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    row.set_margin_top(4); row.set_margin_bottom(8)
    text_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
    lbl = Gtk.Label(label=label); lbl.set_xalign(0)
    lbl.get_style_context().add_class("form-label")
    text_col.pack_start(lbl, False, False, 0)
    if helper:
        h = Gtk.Label(label=helper); h.set_xalign(0); h.set_line_wrap(True)
        h.get_style_context().add_class("form-helper")
        h.get_style_context().add_class("mackes-section-meta")
        text_col.pack_start(h, False, False, 0)
    row.pack_start(text_col, True, True, 0)
    if control is not None:
        control.set_halign(Gtk.Align.END)
        control.set_valign(Gtk.Align.CENTER)
        row.pack_end(control, False, False, 0)
    return row


_STATUS_TAG_KIND = {"ok": "success", "warn": "warning",
                    "fail": "error", "missing": "neutral"}


# ---- settings persistence -------------------------------------------------


SETTINGS_FILE = CONFIG_DIR / "remote-desktop.json"


def _default_settings() -> dict:
    return {
        "x11vnc_display":       ":0",
        "x11vnc_view_only":     False,
        "xrdp_max_sessions":    10,
        "xrdp_session_type":    "Xorg",        # Xorg | Xvnc
        "sync_interval_seconds": 30,
        "open_in_browser_default": True,
    }


def load_settings() -> dict:
    base = _default_settings()
    if not SETTINGS_FILE.exists():
        return base
    try:
        loaded = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return base
    base.update({k: v for k, v in loaded.items() if k in base})
    return base


def save_settings(s: dict) -> None:
    SETTINGS_FILE.parent.mkdir(parents=True, exist_ok=True)
    SETTINGS_FILE.write_text(
        json.dumps(s, indent=2, sort_keys=True),
        encoding="utf-8",
    )


# ---- pkexec helpers (mirror caddy_gateway.py pattern) --------------------


def _run_root(cmd: list[str], *, timeout: int = 30) -> tuple[int, str]:
    """Route through AdminSession (v1.4.0 session-unlock)."""
    from mackes.admin_session import AdminSession
    return AdminSession.instance().run(cmd, timeout=timeout)


def _service_action(unit: str, action: str) -> tuple[int, str]:
    return _run_root(["systemctl", action, unit])


def _service_diag(unit: str) -> str:
    try:
        r = subprocess.run(
            ["systemctl", "status", "--no-pager", "-n", "10", unit],
            capture_output=True, text=True, timeout=8,
        )
        return r.stdout + (r.stderr if r.returncode else "")
    except (OSError, subprocess.TimeoutExpired) as e:
        return f"(diagnostic unavailable: {e})"


# ---- panel ----------------------------------------------------------------


class RemoteDesktopPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._settings = load_settings()
        self._last_sync = "(unknown)"
        self._suppress_writes = False
        self._build()
        self._refresh()

    # ---- build ------------------------------------------------------------

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(32); outer.set_margin_bottom(32)
        outer.set_margin_start(40); outer.set_margin_end(40)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Mesh Remote"), False, False, 0)
        outer.pack_start(_page_subtitle(
            "Drive any mesh peer's desktop from your browser. Each peer runs "
            "xrdp + x11vnc + guacd + Guacamole behind the media.mesh gateway. "
            "Picker is reachable at https://media.mesh/desktop/ from any peer."
        ), False, False, 0)

        # 1) Status notification + service health
        self._status_notif_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL,
                                          spacing=0)
        self._status_notif_box.set_margin_top(8)
        outer.pack_start(self._status_notif_box, False, False, 0)

        action_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        action_row.set_margin_top(8); action_row.set_margin_bottom(8)
        action_row.pack_start(
            Button("Open in browser", kind=ButtonKind.PRIMARY,
                   icon_name="applications-internet-symbolic",
                   on_click=self._on_open_browser),
            False, False, 0)
        action_row.pack_start(
            Button("Resync now", kind=ButtonKind.GHOST,
                   icon_name="view-refresh-symbolic",
                   on_click=self._on_resync),
            False, False, 0)
        action_row.pack_start(
            Button("Restart all services", kind=ButtonKind.GHOST,
                   icon_name="view-refresh-symbolic",
                   on_click=self._on_restart_all),
            False, False, 0)
        outer.pack_start(action_row, False, False, 0)

        outer.pack_start(_section_title("Local services"), False, False, 0)
        self._svc_grid = Gtk.Grid(column_spacing=8, row_spacing=8,
                                   column_homogeneous=True)
        outer.pack_start(self._svc_grid, False, False, 0)

        # 2) Display sharing (x11vnc — live :0 mirror)
        outer.pack_start(_section_title(
            "Display sharing", meta="x11vnc — mirrors your live X session"),
            False, False, 0)
        ds_tile = Tile()

        self._x11vnc_switch = Gtk.Switch()
        self._x11vnc_switch.connect("notify::active", self._on_x11vnc_toggle)
        ds_tile.pack(_form_row(
            "Enable live mirror",
            helper="Streams whatever is on your :0 display to mesh viewers. "
                   "Turning this off stops x11vnc@:0.service.",
            control=self._x11vnc_switch))

        self._x11vnc_display = Gtk.ComboBoxText()
        for d in (":0", ":1"):
            self._x11vnc_display.append_text(d)
        self._x11vnc_display.set_active(0)
        self._x11vnc_display.connect("changed", self._on_x11vnc_display)
        ds_tile.pack(_form_row(
            "X display",
            helper="Which local X display x11vnc captures.",
            control=self._x11vnc_display))

        self._x11vnc_view_only = Gtk.Switch()
        self._x11vnc_view_only.connect("notify::active", self._on_x11vnc_view_only)
        ds_tile.pack(_form_row(
            "View-only mode",
            helper="Remote viewers see your screen but cannot move the mouse "
                   "or send keystrokes.",
            control=self._x11vnc_view_only))

        outer.pack_start(ds_tile, False, False, 0)

        # 3) RDP server (xrdp — new XFCE session)
        outer.pack_start(_section_title(
            "RDP server", meta="xrdp — serves a separate XFCE session"),
            False, False, 0)
        rdp_tile = Tile()

        self._xrdp_switch = Gtk.Switch()
        self._xrdp_switch.connect("notify::active", self._on_xrdp_toggle)
        rdp_tile.pack(_form_row(
            "Enable RDP server",
            helper="Lets RDP clients connect to a NEW XFCE session on port "
                   "3389 (mesh-firewalled).",
            control=self._xrdp_switch))

        self._xrdp_session_type = Gtk.ComboBoxText()
        for v in ("Xorg", "Xvnc"):
            self._xrdp_session_type.append_text(v)
        self._xrdp_session_type.set_active(0)
        self._xrdp_session_type.connect("changed", self._on_xrdp_session_type)
        rdp_tile.pack(_form_row(
            "Session backend",
            helper="Xorg = direct X server (recommended). Xvnc = nest in "
                   "TigerVNC. Xorg supports better performance + hardware.",
            control=self._xrdp_session_type))

        self._xrdp_max = Gtk.SpinButton.new_with_range(1, 50, 1)
        self._xrdp_max.connect("value-changed", self._on_xrdp_max)
        rdp_tile.pack(_form_row(
            "Max concurrent sessions",
            helper="Hard cap on simultaneous RDP logins. Default 10.",
            control=self._xrdp_max))

        outer.pack_start(rdp_tile, False, False, 0)

        # 4) Gateway
        outer.pack_start(_section_title(
            "Gateway", meta="https://media.mesh/desktop/"),
            False, False, 0)
        gw_tile = Tile()
        gw_head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)

        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        title = Gtk.Label(label="Guacamole (Tomcat-hosted)")
        title.set_xalign(0)
        title.get_style_context().add_class("mackes-section-title")
        body.pack_start(title, False, False, 0)
        sub = Gtk.Label(label=(
            "No-auth picker: any peer on the mesh sees every connection. "
            "Auth is enforced by the firewall + mesh CA, not Guacamole."
        ))
        sub.set_xalign(0); sub.set_line_wrap(True)
        sub.get_style_context().add_class("mackes-page-subtitle")
        body.pack_start(sub, False, False, 0)
        gw_head.pack_start(body, True, True, 0)

        self._tomcat_switch = Gtk.Switch()
        self._tomcat_switch.connect("notify::active", self._on_tomcat_toggle)
        ctrl = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        ctrl.set_valign(Gtk.Align.CENTER)
        ctrl_lbl = Gtk.Label(label="Tomcat")
        ctrl_lbl.set_xalign(1)
        ctrl_lbl.get_style_context().add_class("dim-label")
        ctrl.pack_start(ctrl_lbl, False, False, 0)
        ctrl.pack_start(self._tomcat_switch, False, False, 0)
        gw_head.pack_end(ctrl, False, False, 0)
        gw_tile.pack(gw_head)

        # Code preview of effective gateway route
        gw_code = Gtk.TextView()
        gw_code.set_monospace(True); gw_code.set_editable(False)
        gw_code.get_style_context().add_class("mackes-code")
        gw_code.get_buffer().set_text(
            "https://media.mesh/desktop/   →  http://127.0.0.1:8080/guacamole/\n"
            "  upstream:   Tomcat 10 + guacamole.war\n"
            "  guacd:      127.0.0.1:4822\n"
            "  noauth:     /etc/guacamole/noauth-config.xml"
        )
        gw_tile.pack(gw_code)
        outer.pack_start(gw_tile, False, False, 0)

        # 5) Connections (DataTable + per-row actions)
        self._conn_section = _section_title("Connections", meta="loading…")
        outer.pack_start(self._conn_section, False, False, 0)
        self._table = DataTable(
            columns=[
                Column(name="mark",     title="",            width=32),
                Column(name="name",     title="Connection",  width=240),
                Column(name="protocol", title="Protocol",    width=80,
                       monospace=True),
                Column(name="target",   title="Target",      width=180,
                       monospace=True),
                Column(name="online",   title="Status",      width=80),
                Column(name="actions",  title="",            width=200),
            ],
            searchable=True,
            on_row_activate=self._on_row_activate,
        )
        self._table.set_size_request(-1, 360)
        outer.pack_start(self._table, True, True, 0)

        # Per-connection action toolbar (operates on selected row in the table)
        sel_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        sel_bar.set_margin_top(8)
        sel_bar.pack_start(Button("★ Toggle favorite", kind=ButtonKind.TERTIARY,
                                   on_click=self._on_toggle_fav),
                            False, False, 0)
        sel_bar.pack_start(Button("Hide / unhide", kind=ButtonKind.TERTIARY,
                                   on_click=self._on_toggle_hide),
                            False, False, 0)
        sel_bar.pack_start(Button("Rename…", kind=ButtonKind.TERTIARY,
                                   on_click=self._on_rename),
                            False, False, 0)
        sel_bar.pack_start(Button("Open in browser", kind=ButtonKind.GHOST,
                                   on_click=self._on_open_browser),
                            False, False, 0)
        outer.pack_start(sel_bar, False, False, 0)

        # 6) Auto-discovery
        outer.pack_start(_section_title(
            "Auto-discovery", meta="Headscale ↔ Guacamole sync"),
            False, False, 0)
        sync_tile = Tile()
        self._sync_interval = Gtk.SpinButton.new_with_range(10, 600, 5)
        self._sync_interval.connect("value-changed", self._on_sync_interval)
        sync_tile.pack(_form_row(
            "Sync interval (seconds)",
            helper="How often mackes-remote-sync regenerates the Guacamole "
                   "connection list from the Headscale peer roster.",
            control=self._sync_interval))
        self._last_sync_lbl = Gtk.Label(label="(loading)")
        self._last_sync_lbl.set_xalign(0)
        self._last_sync_lbl.get_style_context().add_class("mackes-section-meta")
        sync_tile.pack(_form_row("Last sync",
                                 helper="Most recent regeneration timestamp.",
                                 control=self._last_sync_lbl))
        outer.pack_start(sync_tile, False, False, 0)

        # 7) Diagnostics
        outer.pack_start(_section_title("Diagnostics"), False, False, 0)
        diag_tile = Tile()
        self._diag_view = Gtk.TextView()
        self._diag_view.set_monospace(True); self._diag_view.set_editable(False)
        self._diag_view.get_style_context().add_class("mackes-code")
        diag_scroll = Gtk.ScrolledWindow()
        diag_scroll.set_min_content_height(200)
        diag_scroll.add(self._diag_view)
        diag_tile.pack(diag_scroll)
        diag_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        diag_bar.set_margin_top(8)
        diag_bar.pack_start(Button("Refresh diagnostics", kind=ButtonKind.GHOST,
                                    icon_name="view-refresh-symbolic",
                                    on_click=self._refresh_diagnostics),
                             False, False, 0)
        diag_tile.pack(diag_bar)
        outer.pack_start(diag_tile, False, False, 0)

        # Scroll wrap
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- refresh ----------------------------------------------------------

    def _refresh(self) -> None:
        self._refresh_status()
        self._refresh_connections()
        self._refresh_settings_widgets()
        self._refresh_diagnostics()

    def _refresh_status(self) -> None:
        for c in list(self._status_notif_box.get_children()):
            self._status_notif_box.remove(c)
        statuses = service_status()
        ok_count = sum(1 for v in statuses.values() if v == "ok")
        if ok_count == len(statuses):
            self._status_notif_box.pack_start(Notification(
                f"Remote desktop live — {ok_count}/{len(statuses)} services ok",
                body="Mesh peers can reach this peer's desktop via "
                     "https://media.mesh/desktop/.",
                kind=NotificationKind.SUCCESS, dismissible=False,
            ), False, False, 0)
        else:
            failed = [u for u, v in statuses.items() if v != "ok"]
            self._status_notif_box.pack_start(Notification(
                f"Remote desktop degraded — {ok_count}/{len(statuses)} services ok",
                body=f"Not active: {', '.join(failed)}",
                kind=NotificationKind.WARNING, dismissible=False,
            ), False, False, 0)
        self._status_notif_box.show_all()

        # Service health tiles
        for c in list(self._svc_grid.get_children()):
            self._svc_grid.remove(c)
        for i, (unit, status) in enumerate(statuses.items()):
            cell = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
            cell.get_style_context().add_class("mackes-stat-tile")
            cell.set_size_request(-1, 56)
            name = Gtk.Label(label=unit.replace(".service", ""))
            name.set_xalign(0)
            cell.pack_start(name, True, True, 0)
            cell.pack_end(_tag(status, _STATUS_TAG_KIND.get(status, "neutral")),
                          False, False, 0)
            self._svc_grid.attach(cell, i % 4, i // 4, 1, 1)
        self._svc_grid.show_all()

    def _refresh_connections(self) -> None:
        conns = active_connections()
        rows = []
        for c in conns:
            mark = "★" if c.is_favorite else ("○" if c.hidden else " ")
            rows.append({
                "id":       c.id,
                "mark":     mark,
                "name":     c.name,
                "protocol": c.protocol.upper(),
                "target":   f"{c.hostname}:{c.port}",
                "online":   "online" if c.online else "—",
                "actions":  "click ★ / hide / rename buttons below",
            })
        self._table.set_rows(rows)
        self._conn_index = {c.id: c for c in conns}
        # Update meta
        visible = sum(1 for c in conns if not c.hidden)
        for child in list(self._conn_section.get_children()):
            if isinstance(child, Gtk.Label) and "mackes-section-meta" in (
                child.get_style_context().list_classes() or []
            ):
                child.set_text(
                    f"{visible} visible · {len(conns) - visible} hidden · "
                    f"{sum(1 for c in conns if c.is_favorite)} favorited"
                )
                break

    def _refresh_settings_widgets(self) -> None:
        self._suppress_writes = True
        try:
            statuses = service_status()
            self._x11vnc_switch.set_active(statuses.get("x11vnc@:0.service") == "ok")
            self._xrdp_switch.set_active(statuses.get("xrdp.service") == "ok")
            self._tomcat_switch.set_active(statuses.get("tomcat.service") == "ok")
            # Display selector
            disp = self._settings.get("x11vnc_display", ":0")
            model = self._x11vnc_display.get_model()
            for i, row in enumerate(model):
                if row[0] == disp:
                    self._x11vnc_display.set_active(i)
                    break
            self._x11vnc_view_only.set_active(bool(self._settings.get("x11vnc_view_only")))
            st = self._settings.get("xrdp_session_type", "Xorg")
            model2 = self._xrdp_session_type.get_model()
            for i, row in enumerate(model2):
                if row[0] == st:
                    self._xrdp_session_type.set_active(i)
                    break
            self._xrdp_max.set_value(int(self._settings.get("xrdp_max_sessions", 10)))
            self._sync_interval.set_value(int(self._settings.get("sync_interval_seconds", 30)))

            # Last sync = noauth-config mtime
            try:
                if NOAUTH_CONFIG_PATH.exists():
                    mtime = NOAUTH_CONFIG_PATH.stat().st_mtime
                    delta = int(time.time() - mtime)
                    if delta < 60:
                        self._last_sync_lbl.set_text(f"{delta}s ago")
                    elif delta < 3600:
                        self._last_sync_lbl.set_text(f"{delta // 60}m ago")
                    else:
                        self._last_sync_lbl.set_text(time.strftime(
                            "%Y-%m-%d %H:%M:%S", time.localtime(mtime)))
                else:
                    self._last_sync_lbl.set_text("(no config yet)")
            except OSError:
                self._last_sync_lbl.set_text("(unknown)")
        finally:
            self._suppress_writes = False

    def _refresh_diagnostics(self) -> None:
        chunks = []
        for unit in ("xrdp.service", "x11vnc@:0.service",
                     "guacd.service", "tomcat.service",
                     "mackes-remote-sync.service"):
            chunks.append(f"==== {unit} ====")
            chunks.append(_service_diag(unit).rstrip())
            chunks.append("")
        self._diag_view.get_buffer().set_text("\n".join(chunks))

    # ---- service toggles --------------------------------------------------

    def _on_x11vnc_toggle(self, sw: Gtk.Switch, _g) -> None:
        if self._suppress_writes:
            return
        action = "enable --now" if sw.get_active() else "disable --now"
        _run_root(["bash", "-c", f"systemctl {action} x11vnc@:0.service"])
        GLib.idle_add(self._refresh)

    def _on_xrdp_toggle(self, sw: Gtk.Switch, _g) -> None:
        if self._suppress_writes:
            return
        action = "enable --now" if sw.get_active() else "disable --now"
        _run_root(["bash", "-c",
                   f"systemctl {action} xrdp.service xrdp-sesman.service"])
        GLib.idle_add(self._refresh)

    def _on_tomcat_toggle(self, sw: Gtk.Switch, _g) -> None:
        if self._suppress_writes:
            return
        action = "enable --now" if sw.get_active() else "disable --now"
        _run_root(["bash", "-c",
                   f"systemctl {action} tomcat.service guacd.service"])
        GLib.idle_add(self._refresh)

    def _on_restart_all(self) -> None:
        _run_root([
            "bash", "-c",
            "systemctl restart xrdp xrdp-sesman x11vnc@:0 guacd tomcat "
            "mackes-remote-sync"
        ])
        GLib.idle_add(self._refresh)

    # ---- settings persistence ---------------------------------------------

    def _on_x11vnc_display(self, combo) -> None:
        if self._suppress_writes:
            return
        self._settings["x11vnc_display"] = combo.get_active_text() or ":0"
        save_settings(self._settings)

    def _on_x11vnc_view_only(self, sw, _g) -> None:
        if self._suppress_writes:
            return
        self._settings["x11vnc_view_only"] = sw.get_active()
        save_settings(self._settings)

    def _on_xrdp_session_type(self, combo) -> None:
        if self._suppress_writes:
            return
        self._settings["xrdp_session_type"] = combo.get_active_text() or "Xorg"
        save_settings(self._settings)

    def _on_xrdp_max(self, spin) -> None:
        if self._suppress_writes:
            return
        self._settings["xrdp_max_sessions"] = int(spin.get_value())
        save_settings(self._settings)

    def _on_sync_interval(self, spin) -> None:
        if self._suppress_writes:
            return
        self._settings["sync_interval_seconds"] = int(spin.get_value())
        save_settings(self._settings)

    # ---- gateway actions --------------------------------------------------

    def _on_open_browser(self) -> None:
        try:
            subprocess.Popen(["xdg-open", "https://media.mesh/desktop/"],
                             stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL,
                             start_new_session=True)
        except OSError:
            pass

    def _on_resync(self) -> None:
        rebuild_connections()
        self._refresh()

    # ---- DataTable handlers ----------------------------------------------

    def _on_row_activate(self, row: dict) -> None:
        self._on_open_browser()

    def _selected_conn(self) -> Optional[ResolvedConnection]:
        row = self._table.selected_row() if hasattr(self._table, "selected_row") else None
        if row is None:
            return None
        return self._conn_index.get(row.get("id"))

    def _on_toggle_fav(self) -> None:
        conn = self._selected_conn()
        if conn is None:
            return
        ov = load_overrides()
        favs = set(ov.favorites)
        favs.symmetric_difference_update({conn.id})
        ov.favorites = sorted(favs)
        save_overrides(ov)
        rebuild_connections()
        self._refresh()

    def _on_toggle_hide(self) -> None:
        conn = self._selected_conn()
        if conn is None:
            return
        ov = load_overrides()
        hidden = set(ov.hidden)
        hidden.symmetric_difference_update({conn.id})
        ov.hidden = sorted(hidden)
        save_overrides(ov)
        rebuild_connections()
        self._refresh()

    def _on_rename(self) -> None:
        conn = self._selected_conn()
        if conn is None:
            return
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        entry = Gtk.Entry()
        entry.set_text(conn.name)
        entry.set_size_request(360, -1)
        body.pack_start(Gtk.Label(label=f"Rename {conn.id}:"), False, False, 0)
        body.pack_start(entry, False, False, 0)
        body.pack_start(Gtk.Label(label="(leave empty to clear the override)"),
                        False, False, 0)
        modal = Modal(self.get_toplevel(), "Rename connection",
                      body, size=ModalSize.SMALL)
        def _save() -> None:
            ov = load_overrides()
            new_name = entry.get_text().strip()
            if new_name and new_name != f"{conn.id.replace('-rdp', '').replace('-vnc', '')} — Session":
                ov.renames[conn.id] = new_name
            else:
                ov.renames.pop(conn.id, None)
            save_overrides(ov)
            rebuild_connections()
            self._refresh()
        modal.add_action("Cancel", kind=ButtonKind.SECONDARY,
                         response_id=Gtk.ResponseType.CANCEL)
        modal.add_action("Save", kind=ButtonKind.PRIMARY,
                         on_click=_save,
                         response_id=Gtk.ResponseType.OK)
        modal.run_then_destroy()
