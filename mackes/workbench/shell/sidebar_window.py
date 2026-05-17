"""Carbon UI Shell — sidebar-grouped workbench window (v1.1.0).

Replaces the v1.0 top-tab Notebook layout with the design's Carbon UI Shell:

    +--------------------------------------------------------------+
    |  brand        | Workbench  Recovery  CLI   ...   user@host  | header (48)
    +---------------+----------------------------------------------+
    | WORKBENCH     |                                              |
    | • Dashboard   |                                              |
    | CONFIGURATION |                                              |
    | • Look & Feel |             content (Gtk.Stack)              |
    | • Devices     |                                              |
    | NETWORK       |                                              |
    | • Mesh VPN    |                                              |
    | • ...         |                                              |
    +---------------+----------------------------------------------+
    | mesh: 5/16   services: 12   sshd ✓  drift: 3   ...           | status (24)
    +--------------------------------------------------------------+

Existing panel widgets are reused as-is — this module owns the chrome and the
content router only.
"""
from __future__ import annotations

from typing import Callable, Dict, List, Optional, Tuple

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

from mackes.state import MackesState
from mackes import __version__


# ---------------------------------------------------------------------------
# Navigation model
# ---------------------------------------------------------------------------


class NavItem:
    __slots__ = ("key", "label", "icon", "badge", "builder")

    def __init__(self, key: str, label: str, icon: str,
                 builder: Callable[[], Gtk.Widget], *, badge: str = "") -> None:
        self.key = key
        self.label = label
        self.icon = icon  # Gtk icon-name (symbolic)
        self.badge = badge
        self.builder = builder  # lazily instantiates the panel widget


class NavGroup:
    __slots__ = ("title", "items")

    def __init__(self, title: str, items: List[NavItem]) -> None:
        self.title = title
        self.items = items


# ---------------------------------------------------------------------------
# Lazy panel builders — each closes over the application state.
# Panels are instantiated on first activation, then cached in the Gtk.Stack.
# ---------------------------------------------------------------------------


def _wrap_in_scroller(widget: Gtk.Widget) -> Gtk.Widget:
    scroller = Gtk.ScrolledWindow()
    scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
    scroller.add(widget)
    return scroller


def _build_subnav_container(panels: List[Tuple[str, str, Gtk.Widget]]) -> Gtk.Widget:
    """Inner sidebar+stack for sections that still have sub-panels (Devices, System, Look&Feel)."""
    stack = Gtk.Stack()
    stack.set_transition_type(Gtk.StackTransitionType.CROSSFADE)
    stack.set_transition_duration(150)
    for pid, label, widget in panels:
        stack.add_titled(_wrap_in_scroller(widget), pid, label)

    sidebar = Gtk.StackSidebar()
    sidebar.set_stack(stack)
    sidebar.set_size_request(200, -1)

    pane = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
    pane.pack_start(sidebar, False, False, 0)
    pane.pack_start(Gtk.Separator(orientation=Gtk.Orientation.VERTICAL), False, False, 0)
    pane.pack_start(stack, True, True, 0)
    return pane


def _build_nav(state: MackesState, navigate: Callable[[str], None]) -> List[NavGroup]:
    """Define the sidebar nav model. Lambdas defer panel imports until activated."""

    def _dashboard():
        from mackes.workbench.dashboard import DashboardView
        return DashboardView(state, navigate=navigate)

    def _look_and_feel():
        from mackes.workbench.look_and_feel.appearance import AppearancePanel
        return _build_subnav_container([
            ("appearance", "Appearance", AppearancePanel()),
        ])

    def _devices():
        from mackes.workbench.devices.display import DisplayPanel
        from mackes.workbench.devices.keyboard import KeyboardPanel
        from mackes.workbench.devices.mouse import MousePanel
        from mackes.workbench.devices.sound import SoundPanel
        from mackes.workbench.devices.power import PowerPanel
        return _build_subnav_container([
            ("display", "Display", DisplayPanel()),
            ("keyboard", "Keyboard", KeyboardPanel()),
            ("mouse", "Mouse & Touchpad", MousePanel()),
            ("sound", "Sound", SoundPanel()),
            ("power", "Power", PowerPanel()),
        ])

    def _system():
        from mackes.workbench.system.window_manager import WindowManagerPanel
        from mackes.workbench.system.workspaces import WorkspacesPanel
        from mackes.workbench.system.session import SessionPanel
        from mackes.workbench.system.notifications import NotificationsPanel
        from mackes.workbench.system.default_apps import DefaultAppsPanel
        from mackes.workbench.system.removable import RemovablePanel
        from mackes.workbench.system.datetime import DateTimePanel
        return _build_subnav_container([
            ("wm", "Window Manager", WindowManagerPanel()),
            ("workspaces", "Workspaces", WorkspacesPanel()),
            ("session", "Session & Startup", SessionPanel()),
            ("notifications", "Notifications", NotificationsPanel()),
            ("default_apps", "Default Apps", DefaultAppsPanel()),
            ("removable", "Removable Media", RemovablePanel()),
            ("datetime", "Date & Time", DateTimePanel()),
        ])

    def _wifi():
        from mackes.workbench.network.wifi import WifiPanel
        return _wrap_in_scroller(WifiPanel())

    def _vpn():
        from mackes.workbench.network.vpn import VpnPanel
        return _wrap_in_scroller(VpnPanel())

    def _qnm():
        from mackes.workbench.network.qnm import QnmPanel
        return _wrap_in_scroller(QnmPanel())

    def _mesh_vpn():
        from mackes.workbench.network.mesh_vpn import MeshVpnPanel
        return _wrap_in_scroller(MeshVpnPanel())

    def _mesh_ssh():
        from mackes.workbench.network.mesh_ssh import MeshSshPanel
        return _wrap_in_scroller(MeshSshPanel())

    def _mesh_services():
        from mackes.workbench.network.mesh_services import MeshServicesPanel
        return _wrap_in_scroller(MeshServicesPanel())

    def _firewall():
        from mackes.workbench.network.firewall import FirewallPanel
        return _wrap_in_scroller(FirewallPanel())

    def _remote_desktop():
        from mackes.workbench.network.remote_desktop import RemoteDesktopPanel
        return _wrap_in_scroller(RemoteDesktopPanel())

    def _apps():
        from mackes.workbench.apps.panel import AppsPanel
        return AppsPanel()

    def _maintain():
        from mackes.workbench.maintain.hub import MaintainHub
        from mackes.workbench.maintain.snapshots import SnapshotsPanel
        from mackes.workbench.maintain.drift import DriftPanel
        from mackes.workbench.maintain.fonts import FontsPanel
        from mackes.workbench.maintain.power import PowerPanel as MaintPower
        from mackes.workbench.maintain.resources import ResourcesPanel
        from mackes.workbench.maintain.health_check import HealthCheckPanel
        from mackes.workbench.maintain.dependencies import DependenciesPanel
        from mackes.workbench.maintain.logs import LogsPanel
        from mackes.workbench.maintain.repair import RepairPanel
        from mackes.workbench.maintain.reset_to_preset import ResetToPresetPanel
        from mackes.workbench.maintain.system_update import SystemUpdatePanel
        from mackes.workbench.maintain.uninstall import UninstallPanel
        from mackes.workbench.maintain.debloat import DebloatPanel

        # Inner Gtk.Stack — hub view + sub-panels. Tile clicks call
        # stack.set_visible_child_name(<key>); a "← Back to Maintain" link
        # at the top of each sub-panel returns to the hub.
        inner_stack = Gtk.Stack()
        inner_stack.set_transition_type(Gtk.StackTransitionType.CROSSFADE)
        inner_stack.set_transition_duration(120)

        def _go(key: str) -> None:
            inner_stack.set_visible_child_name(key)

        # Wrap each sub-panel with a back-link header
        def _wrap_with_back(panel: Gtk.Widget, label: str) -> Gtk.Widget:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
            head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
            head.set_margin_top(16); head.set_margin_start(40)
            head.set_margin_end(40); head.set_margin_bottom(0)
            back = Gtk.Button(label="‹ Back to Maintain")
            back.set_relief(Gtk.ReliefStyle.NONE)
            back.get_style_context().add_class("cds-button-ghost")
            back.connect("clicked", lambda *_: _go("__hub"))
            head.pack_start(back, False, False, 0)
            crumb = Gtk.Label(label=f"Maintain  /  {label}")
            crumb.set_xalign(0)
            crumb.get_style_context().add_class("mackes-breadcrumb")
            head.pack_start(crumb, True, True, 0)
            box.pack_start(head, False, False, 0)
            scroll = Gtk.ScrolledWindow()
            scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
            scroll.add(panel)
            box.pack_start(scroll, True, True, 0)
            return box

        inner_stack.add_named(MaintainHub(on_open=_go, state=state), "__hub")
        inner_stack.add_named(_wrap_with_back(SnapshotsPanel(state), "Snapshots"), "snapshots")
        inner_stack.add_named(_wrap_with_back(DriftPanel(state), "Drift"), "drift")
        inner_stack.add_named(_wrap_with_back(SystemUpdatePanel(), "System update"), "update")
        inner_stack.add_named(_wrap_with_back(FontsPanel(), "Fonts"), "fonts")
        inner_stack.add_named(_wrap_with_back(MaintPower(), "Power"), "power")
        inner_stack.add_named(_wrap_with_back(ResourcesPanel(), "Resources"), "resources")
        inner_stack.add_named(_wrap_with_back(HealthCheckPanel(), "Health"), "health")
        inner_stack.add_named(_wrap_with_back(DependenciesPanel(), "Dependencies"), "deps")
        inner_stack.add_named(_wrap_with_back(LogsPanel(), "Logs"), "logs")
        inner_stack.add_named(_wrap_with_back(RepairPanel(state), "Repair"), "repair")
        inner_stack.add_named(_wrap_with_back(ResetToPresetPanel(state), "Reset to Preset"), "reset")
        inner_stack.add_named(_wrap_with_back(UninstallPanel(), "Uninstall"), "uninstall")
        inner_stack.add_named(_wrap_with_back(DebloatPanel(), "Debloat levels"), "debloat")
        inner_stack.set_visible_child_name("__hub")
        return inner_stack

    def _snapshots():
        from mackes.workbench.maintain.snapshots import SnapshotsPanel
        return _wrap_in_scroller(SnapshotsPanel(state))

    def _fleet_inventory():
        from mackes.workbench.fleet.inventory import FleetInventoryPanel
        return _wrap_in_scroller(FleetInventoryPanel())

    def _fleet_playbooks():
        from mackes.workbench.fleet.playbooks import FleetPlaybooksPanel
        return _wrap_in_scroller(FleetPlaybooksPanel())

    def _fleet_runs():
        from mackes.workbench.fleet.run_history import FleetRunHistoryPanel
        return _wrap_in_scroller(FleetRunHistoryPanel())

    def _help():
        from mackes.workbench.help import HelpPanel
        return HelpPanel()

    return [
        NavGroup("Workbench", [
            NavItem("dashboard", "Dashboard", "view-grid-symbolic", _dashboard),
        ]),
        NavGroup("Configuration", [
            NavItem("look_and_feel", "Look & Feel", "applications-graphics-symbolic", _look_and_feel),
            NavItem("devices", "Devices", "preferences-desktop-peripherals-symbolic", _devices),
            NavItem("system", "System", "computer-symbolic", _system),
        ]),
        NavGroup("Network", [
            NavItem("wifi", "Wi-Fi & Ethernet", "network-wireless-symbolic", _wifi),
            NavItem("vpn", "VPN", "network-vpn-symbolic", _vpn),
            NavItem("qnm", "QNM", "network-workgroup-symbolic", _qnm),
            NavItem("mesh_vpn", "Mesh VPN", "network-server-symbolic", _mesh_vpn, badge="mesh"),
            NavItem("mesh_ssh", "Mesh SSH", "channel-secure-symbolic", _mesh_ssh),
            NavItem("mesh_services", "Mesh Services", "applications-internet-symbolic", _mesh_services),
            NavItem("mesh_remote", "Mesh Remote", "video-display-symbolic", _remote_desktop),
            NavItem("firewall", "Firewall", "network-firewall-symbolic", _firewall),
        ]),
        NavGroup("Fleet", [
            NavItem("fleet_inventory", "Inventory", "view-list-symbolic", _fleet_inventory),
            NavItem("fleet_playbooks", "Playbooks", "text-x-script-symbolic", _fleet_playbooks),
            NavItem("fleet_runs", "Run history", "document-open-recent-symbolic", _fleet_runs),
        ]),
        NavGroup("Apps & Maintenance", [
            NavItem("apps", "Apps", "applications-other-symbolic", _apps),
            NavItem("maintain", "Maintain", "preferences-system-symbolic", _maintain),
            NavItem("snapshots", "Snapshots", "document-revert-symbolic", _snapshots),
        ]),
        NavGroup("Reference", [
            NavItem("help", "Help", "help-browser-symbolic", _help),
        ]),
    ]


# ---------------------------------------------------------------------------
# Main window
# ---------------------------------------------------------------------------


class WorkbenchWindow(Gtk.ApplicationWindow):
    """Carbon UI Shell window. Single visual root for the whole app."""

    def __init__(self, application: Gtk.Application, state: MackesState) -> None:
        super().__init__(application=application)
        # v1.4.2 — Fit the workstation resolution perfectly. Open at the
        # primary monitor's exact size and immediately maximize so the
        # WM finishes the job (xfwm4 maximizes to the workarea — full
        # screen minus the xfce4-panel strip).
        mon_w, mon_h = _primary_monitor_size()
        self.set_default_size(mon_w, mon_h)
        self.set_title("Mackes Shell")
        self.state = state
        # Maximize on first show. We connect to `realize` so the
        # WM has a window manager state to manipulate.
        self.connect("realize", lambda *_: self.maximize())

        # CSS-class root marker so accent files can scope rules to the app
        # window when needed.
        self.get_style_context().add_class("mackes-app-window")
        if state.active_preset:
            self.get_style_context().add_class(f"preset-{state.active_preset}")

        self._nav: List[NavGroup] = _build_nav(state, navigate=self.go_to)
        self._nav_buttons: Dict[str, Gtk.Button] = {}
        self._panel_widgets: Dict[str, Gtk.Widget] = {}
        self._tweaks = _load_tweaks()

        # ---- 3-zone layout (header / body / status) -----------------------
        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        root.pack_start(self._build_header(), False, False, 0)
        body = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        body.pack_start(self._build_sidenav(), False, False, 0)
        self._content = Gtk.Stack()
        self._content.set_transition_type(Gtk.StackTransitionType.CROSSFADE)
        self._content.set_transition_duration(120)
        body.pack_start(self._content, True, True, 0)
        root.pack_start(body, True, True, 0)
        self._status_bar = self._build_status_bar()
        root.pack_start(self._status_bar, False, False, 0)
        self.add(root)

        # ---- Floating overlays (Tweaks + toast host, anchored b-r) ---------
        self.remove(root)
        self._overlay = Gtk.Overlay()
        self._overlay.add(root)
        try:
            from mackes.workbench.shell.tweaks_panel import TweaksOverlay
            self._tweaks_overlay = TweaksOverlay(self, self._tweaks, on_change=self._on_tweaks_change)
            self._overlay.add_overlay(self._tweaks_overlay)
        except Exception:  # noqa: BLE001
            self._tweaks_overlay = None
        try:
            from mackes.workbench.shell.toasts import install_host
            install_host(self._overlay)
        except Exception:  # noqa: BLE001
            pass
        self.add(self._overlay)

        # Apply initial tweaks (chrome / status bar visibility / density).
        self._apply_tweaks(self._tweaks)

        # ---- Activate initial panel ---------------------------------------
        initial = self._tweaks.get("active_panel") or "dashboard"
        self.go_to(initial)

        # ---- Live nav-badge refresh (every 30s) ---------------------------
        self._refresh_nav_badges()
        GLib.timeout_add_seconds(30, self._refresh_nav_badges_tick)

        # ---- Auto-lock the admin session on window close ------------------
        self.connect("destroy", self._on_destroy_lock)

    # ----- Build helpers ---------------------------------------------------

    def _build_header(self) -> Gtk.Widget:
        header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        header.get_style_context().add_class("mackes-shell-header")
        header.set_size_request(-1, 48)

        # Brand block — same width as sidenav (256), right-bordered
        brand = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        brand.set_size_request(256, -1)
        brand.set_margin_start(16); brand.set_margin_end(16)
        # logo dot
        logo = Gtk.Image.new_from_icon_name("preferences-desktop-symbolic", Gtk.IconSize.LARGE_TOOLBAR)
        logo.get_style_context().add_class("mackes-dot")
        logo.get_style_context().add_class("accent")
        brand.pack_start(logo, False, False, 0)
        # text label "Mackes Shell" with two-tone weight
        text = Gtk.Label()
        text.set_markup(
            '<span weight="600">Mackes</span><span weight="400" alpha="80%"> Shell</span>'
        )
        text.get_style_context().add_class("mackes-brand-text")
        brand.pack_start(text, False, False, 0)
        # Right border via separator
        header.pack_start(brand, False, False, 0)
        header.pack_start(_vsep(), False, False, 0)

        # Mode buttons (Workbench / Recovery / CLI) — only Workbench is
        # active today; the others are visual stubs that surface their
        # planned function on hover via tooltip.
        for label, tip, active in (
            ("Workbench", "The main control plane (active)", True),
            ("Recovery", "Snapshots & restore — see Snapshots", False),
            ("CLI", "Drop to a shell with mesh env preloaded", False),
        ):
            b = Gtk.Button(label=label)
            b.get_style_context().add_class("mackes-header-action")
            b.get_style_context().add_class("mackes-header-action-flush")
            if active:
                b.get_style_context().add_class("checked")
            b.set_tooltip_text(tip)
            b.set_relief(Gtk.ReliefStyle.NONE)
            header.pack_start(b, False, False, 0)

        # Spacer
        spacer = Gtk.Box()
        header.pack_start(spacer, True, True, 0)

        # Right-aligned actions: preset chip, user
        preset_label = (self.state.active_preset or "mackes").title()
        chip = Gtk.Button(label=preset_label)
        chip.get_style_context().add_class("mackes-header-action")
        chip.set_tooltip_text("Active preset — switch via Tweaks → Preset")
        chip.set_relief(Gtk.ReliefStyle.NONE)
        chip.connect("clicked", self._on_preset_chip)
        header.pack_end(chip, False, False, 0)

        import getpass, socket
        try:
            user = getpass.getuser()
        except Exception:  # noqa: BLE001
            user = "user"
        try:
            host = socket.gethostname()
        except Exception:  # noqa: BLE001
            host = "host"
        ident = Gtk.Button(label=f"{user}@{host}")
        ident.get_style_context().add_class("mackes-header-action")
        ident.set_relief(Gtk.ReliefStyle.NONE)
        ident.set_tooltip_text("Active user @ hostname")
        header.pack_end(ident, False, False, 0)

        # Help button (one-click access)
        help_btn = Gtk.Button()
        help_btn.set_image(Gtk.Image.new_from_icon_name("help-browser-symbolic", Gtk.IconSize.BUTTON))
        help_btn.get_style_context().add_class("mackes-header-action")
        help_btn.set_relief(Gtk.ReliefStyle.NONE)
        help_btn.set_tooltip_text("Help")
        help_btn.connect("clicked", lambda *_: self.go_to("help"))
        header.pack_end(help_btn, False, False, 0)

        # v1.4.1 — always-visible Setup Wizard button (next to Help).
        # Users were missing the wizard entry point; the Tweaks drawer's
        # "Re-open Wizard" button is too hidden.
        wiz_btn = Gtk.Button(label="Setup")
        wiz_btn.set_image(
            Gtk.Image.new_from_icon_name("system-run-symbolic", Gtk.IconSize.BUTTON))
        wiz_btn.set_always_show_image(True)
        wiz_btn.get_style_context().add_class("mackes-header-action")
        wiz_btn.set_relief(Gtk.ReliefStyle.NONE)
        wiz_btn.set_tooltip_text("Open the Setup Wizard — re-run birthright")
        wiz_btn.connect("clicked", self._on_open_wizard)
        header.pack_end(wiz_btn, False, False, 0)

        # ---- Admin session lock/unlock button (v1.4.0) -------------------
        from mackes.admin_session import AdminSession
        self._admin = AdminSession.instance()
        self._unlock_btn = Gtk.Button()
        self._unlock_btn.get_style_context().add_class("mackes-header-action")
        self._unlock_btn.set_relief(Gtk.ReliefStyle.NONE)
        self._unlock_btn.connect("clicked", self._on_unlock_clicked)
        self._update_unlock_button()
        self._admin.add_listener(lambda _ok: self._update_unlock_button())
        header.pack_end(self._unlock_btn, False, False, 0)

        return header

    def _update_unlock_button(self) -> None:
        if self._admin.is_unlocked():
            icon = "changes-allow-symbolic"
            tip = ("Admin session unlocked — privileged actions won't "
                   "prompt for a password until you lock or close Mackes")
            label = "Locked ▾"
            label = "Unlocked"
        else:
            icon = "changes-prevent-symbolic"
            tip = ("Click to unlock the admin session — authorize once, "
                   "then everything runs without further prompts")
            label = "Unlock"
        img = Gtk.Image.new_from_icon_name(icon, Gtk.IconSize.BUTTON)
        # Replace the existing child
        for c in list(self._unlock_btn.get_children()):
            self._unlock_btn.remove(c)
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        row.set_margin_start(8); row.set_margin_end(8)
        row.pack_start(img, False, False, 0)
        row.pack_start(Gtk.Label(label=label), False, False, 0)
        self._unlock_btn.add(row)
        self._unlock_btn.show_all()
        self._unlock_btn.set_tooltip_text(tip)

    def _on_unlock_clicked(self, *_) -> None:
        if self._admin.is_unlocked():
            # Confirm-less lock — instant.
            self._admin.lock()
            return
        # Async unlock to avoid blocking the GTK main loop on the password
        # prompt.
        import threading
        def runner() -> None:
            self._admin.unlock()  # listener will refresh the button
        threading.Thread(target=runner, daemon=True).start()

    def _build_sidenav(self) -> Gtk.Widget:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_size_request(256, -1)
        outer.get_style_context().add_class("mackes-side-nav")

        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        scroller.add(inner)
        outer.pack_start(scroller, True, True, 0)

        for group in self._nav:
            # group title row
            title = Gtk.Label(label=group.title)
            title.set_xalign(0)
            title.get_style_context().add_class("mackes-side-nav-group-title")
            title.set_margin_top(8)
            inner.pack_start(title, False, False, 0)
            for item in group.items:
                btn = self._make_nav_button(item)
                self._nav_buttons[item.key] = btn
                inner.pack_start(btn, False, False, 0)
            # spacer between groups
            spacer = Gtk.Box()
            spacer.set_size_request(-1, 4)
            inner.pack_start(spacer, False, False, 0)

        return outer

    def _make_nav_button(self, item: NavItem) -> Gtk.Button:
        btn = Gtk.Button()
        btn.set_relief(Gtk.ReliefStyle.NONE)
        btn.get_style_context().add_class("mackes-side-nav-item")
        btn.set_size_request(-1, 40)

        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        row.set_margin_start(16); row.set_margin_end(16)

        icon = Gtk.Image.new_from_icon_name(item.icon, Gtk.IconSize.MENU)
        row.pack_start(icon, False, False, 0)

        label = Gtk.Label(label=item.label)
        label.set_xalign(0)
        row.pack_start(label, True, True, 0)

        if item.badge:
            badge = Gtk.Label(label=item.badge)
            badge.get_style_context().add_class("mackes-sn-badge")
            row.pack_end(badge, False, False, 0)

        btn.add(row)
        btn.connect("clicked", lambda *_: self.go_to(item.key))
        return btn

    def _build_status_bar(self) -> Gtk.Widget:
        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=24)
        bar.get_style_context().add_class("mackes-status-bar")
        bar.set_size_request(-1, 24)
        bar.set_margin_start(16); bar.set_margin_end(16)

        # 4 live items + 2 fixed right-aligned items. Built once, refreshed
        # in place via _refresh_status_bar() every 30s.
        self._sb_mesh = _status_item("● mesh: …")
        self._sb_services = _status_item("● services: …")
        self._sb_sshd = _status_item("● sshd: …")
        self._sb_drift = _status_item("● drift: …")
        bar.pack_start(self._sb_mesh, False, False, 0)
        bar.pack_start(self._sb_services, False, False, 0)
        bar.pack_start(self._sb_sshd, False, False, 0)
        bar.pack_start(self._sb_drift, False, False, 0)

        right = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=24)
        right.pack_end(_status_item(f"v{__version__}"), False, False, 0)
        right.pack_end(_status_item(
            f"preset: {self.state.active_preset or '—'}"), False, False, 0)
        bar.pack_end(right, True, True, 0)

        # Kick off the 30s refresh loop
        self._refresh_status_bar()
        GLib.timeout_add_seconds(30, self._refresh_status_bar_tick)
        return bar

    def _refresh_status_bar_tick(self) -> bool:
        self._refresh_status_bar()
        return True   # keep firing

    def _refresh_status_bar(self) -> None:
        """Pull live values from service_health / Headscale / drift detector."""
        try:
            from mackes.state import service_health
            sh = service_health()
        except Exception:  # noqa: BLE001
            sh = {}
        try:
            from mackes.mesh_vpn import headscale_list_peers, MESH_CAP
            peers = headscale_list_peers()
            mesh_total = len(peers)
            mesh_online = sum(1 for p in peers if p.online)
            mesh_cap = MESH_CAP
        except Exception:  # noqa: BLE001
            mesh_online = mesh_total = mesh_cap = 0
        try:
            from mackes.mesh_services import load_registry
            services_n = len(load_registry())
        except Exception:  # noqa: BLE001
            services_n = 0
        try:
            from mackes.presets import active_preset_drift
            _preset, items = active_preset_drift()
            drift_n = len(items or [])
        except Exception:  # noqa: BLE001
            drift_n = 0

        # Mesh
        _set_status_item(
            self._sb_mesh,
            f"● mesh: {mesh_online}/{mesh_cap or 16}",
            "ok" if mesh_online > 0 else "warn",
        )
        # Services
        _set_status_item(
            self._sb_services, f"● services: {services_n}",
            "ok" if services_n else "warn",
        )
        # sshd
        sshd_state = sh.get("sshd", "missing")
        _set_status_item(
            self._sb_sshd, f"● sshd",
            {"ok": "ok", "warn": "warn", "fail": "fail",
             "missing": "warn"}.get(sshd_state, "warn"),
        )
        # Drift
        _set_status_item(
            self._sb_drift, f"● drift: {drift_n}",
            "warn" if drift_n else "ok",
        )

    # ----- Navigation ------------------------------------------------------

    def go_to(self, key: str) -> None:
        """Activate the panel with the given nav key, lazily building it."""
        # Find the nav item
        item = self._find_item(key)
        if item is None:
            # Backward-compat: some callers pass legacy panel keys like
            # "appearance" (which now lives under look_and_feel inner stack).
            # Map them to the parent group.
            legacy = _LEGACY_KEY_MAP.get(key)
            if legacy:
                self.go_to(legacy)
            return

        # Lazy-build the panel
        if item.key not in self._panel_widgets:
            try:
                widget = item.builder()
            except Exception as e:  # noqa: BLE001
                widget = _make_error_placeholder(item.label, str(e))
            self._panel_widgets[item.key] = widget
            self._content.add_named(widget, item.key)
            widget.show_all()
        self._content.set_visible_child_name(item.key)

        # Update nav button visual state
        for k, btn in self._nav_buttons.items():
            ctx = btn.get_style_context()
            if k == item.key:
                ctx.add_class("active")
            else:
                ctx.remove_class("active")

        # Persist active panel
        self._tweaks["active_panel"] = item.key
        _save_tweaks(self._tweaks)

    def _find_item(self, key: str) -> Optional[NavItem]:
        for g in self._nav:
            for it in g.items:
                if it.key == key:
                    return it
        return None

    # ----- Tweaks integration ---------------------------------------------

    def _on_tweaks_change(self, tweaks: dict) -> None:
        self._tweaks = tweaks
        _save_tweaks(tweaks)
        self._apply_tweaks(tweaks)

    def _apply_tweaks(self, tweaks: dict) -> None:
        # status bar visibility
        if tweaks.get("show_status_bar", True):
            self._status_bar.show_all()
        else:
            self._status_bar.hide()
        # density — propagates to carbon-layout.css via root style class
        ctx = self.get_style_context()
        for d in ("compact", "cozy", "comfortable"):
            ctx.remove_class(f"mackes-density-{d}")
        density = tweaks.get("density") or "cozy"
        ctx.add_class(f"mackes-density-{density}")
        # preset swap — reload preset CSS, restyle root class, bounce conky
        new_preset = tweaks.get("preset")
        if new_preset and new_preset != self.state.active_preset:
            from mackes.app import _install_css
            if self.state.active_preset:
                ctx.remove_class(f"preset-{self.state.active_preset}")
            ctx.add_class(f"preset-{new_preset}")
            self.state.active_preset = new_preset
            self.state.save()
            _install_css(new_preset)
            # Repaint the Conky HUD with the new accent (if it's running)
            try:
                from mackes.conky_hud import is_running, restart_with
                if is_running():
                    restart_with()
            except Exception:  # noqa: BLE001
                pass

        # Conky HUD toggle (Q2 v1.4.0 — birthright + Tweaks toggle)
        try:
            from mackes.conky_hud import apply_tweak
            apply_tweak(bool(tweaks.get("show_conky", True)))
        except Exception:  # noqa: BLE001
            pass

    # ---- live nav badges --------------------------------------------------

    def _refresh_nav_badges_tick(self) -> bool:
        self._refresh_nav_badges()
        return True

    def _refresh_nav_badges(self) -> None:
        """Update peer/service/fleet/drift counts on the side-nav badges."""
        badges: dict[str, str] = {}
        try:
            from mackes.mesh_vpn import headscale_list_peers
            peers = headscale_list_peers()
            online = sum(1 for p in peers if p.online)
            if online:
                badges["mesh_vpn"] = str(online)
        except Exception:  # noqa: BLE001
            pass
        try:
            from mackes.mesh_services import load_registry
            n = len(load_registry())
            if n:
                badges["mesh_services"] = str(n)
        except Exception:  # noqa: BLE001
            pass
        try:
            from mackes.fleet import list_runs
            recent = list_runs(limit=200, since=__import__("time").time() - 86400)
            failures = sum(1 for r in recent if r.exit_code != 0)
            if failures:
                badges["fleet_runs"] = str(failures)
        except Exception:  # noqa: BLE001
            pass
        try:
            from mackes.presets import active_preset_drift
            _preset, items = active_preset_drift()
            n = len(items or [])
            if n:
                badges["maintain"] = str(n)
        except Exception:  # noqa: BLE001
            pass

        # Apply to live buttons
        for key, badge_text in badges.items():
            btn = self._nav_buttons.get(key)
            if btn is None:
                continue
            self._set_nav_button_badge(btn, badge_text)
        # Clear badges that are no longer present
        for key, btn in self._nav_buttons.items():
            if key not in badges:
                self._set_nav_button_badge(btn, "")

    @staticmethod
    def _set_nav_button_badge(btn: Gtk.Button, text: str) -> None:
        """Replace any existing trailing badge label on a side-nav button."""
        row = btn.get_child()
        if not isinstance(row, Gtk.Box):
            return
        # Remove existing badge (last child with .mackes-sn-badge class)
        for child in row.get_children():
            classes = child.get_style_context().list_classes() if hasattr(
                child.get_style_context(), "list_classes") else []
            if "mackes-sn-badge" in classes:
                row.remove(child)
        if text:
            badge = Gtk.Label(label=text)
            badge.get_style_context().add_class("mackes-sn-badge")
            row.pack_end(badge, False, False, 0)
        row.show_all()

    # ---- admin session auto-lock on close ---------------------------------

    def _on_destroy_lock(self, *_) -> None:
        try:
            from mackes.admin_session import AdminSession
            sess = AdminSession.instance()
            if sess.is_unlocked():
                sess.lock()
        except Exception:  # noqa: BLE001
            pass

    def _on_preset_chip(self, *_) -> None:
        if self._tweaks_overlay is not None:
            self._tweaks_overlay.open()

    def _on_open_wizard(self, *_) -> None:
        """Header → Setup button. Force the wizard regardless of provisioned state."""
        try:
            from mackes.wizard.window import WizardWindow
            from mackes.state import MackesState
            state = MackesState.load()
            w = WizardWindow(application=self.get_application(), state=state)
            w.show_all()
        except Exception as e:  # noqa: BLE001
            try:
                from mackes.workbench.shell.toasts import toast
                toast(f"Could not open wizard: {e}", kind="error")
            except Exception:  # noqa: BLE001
                pass


# ---------------------------------------------------------------------------
# Tweaks persistence — light JSON in ~/.config/mackes-shell/tweaks.json
# ---------------------------------------------------------------------------


def _tweaks_path():
    from mackes.state import CONFIG_DIR
    return CONFIG_DIR / "tweaks.json"


def _load_tweaks() -> dict:
    import json
    p = _tweaks_path()
    defaults = {
        "preset": None,             # picked up from MackesState
        "density": "cozy",          # compact / cozy / comfortable
        "show_status_bar": True,
        "show_xfce_frame": True,    # client-side; XFCE frame is OS-managed but we keep the flag
        "show_conky": True,         # v1.4.0: birthright Conky HUD on by default
        "active_panel": "dashboard",
    }
    if not p.exists():
        return defaults
    try:
        loaded = json.loads(p.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return defaults
    defaults.update({k: v for k, v in loaded.items() if k in defaults})
    return defaults


def _save_tweaks(tweaks: dict) -> None:
    import json
    p = _tweaks_path()
    try:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(tweaks, indent=2, sort_keys=True), encoding="utf-8")
    except OSError:
        pass


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _vsep() -> Gtk.Widget:
    s = Gtk.Separator(orientation=Gtk.Orientation.VERTICAL)
    return s


def _primary_monitor_size() -> tuple[int, int]:
    """Detect the primary monitor's pixel size via GdkDisplay.

    Falls back to 1280×800 if GdkDisplay isn't available yet (very
    rare; typically only during early test imports).
    """
    try:
        display = Gdk.Display.get_default()
        if display is None:
            return (1280, 800)
        mon = display.get_primary_monitor() or display.get_monitor(0)
        geom = mon.get_geometry()
        return (max(1024, geom.width), max(700, geom.height))
    except Exception:  # noqa: BLE001
        return (1280, 800)


def _status_item(text: str, kind: Optional[str] = None) -> Gtk.Widget:
    """Build a single .mackes-status-bar-item Label."""
    lab = Gtk.Label(label=text)
    lab.set_xalign(0)
    lab.get_style_context().add_class("mackes-status-bar-item")
    if kind:
        lab.get_style_context().add_class(kind)
    return lab


def _set_status_item(widget: Gtk.Label, text: str, kind: Optional[str]) -> None:
    """Update an existing status item's text + class (called every 30s)."""
    widget.set_text(text)
    ctx = widget.get_style_context()
    for k in ("ok", "warn", "fail"):
        ctx.remove_class(k)
    if kind:
        ctx.add_class(kind)


def _make_error_placeholder(name: str, err: str) -> Gtk.Widget:
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
    box.set_margin_top(40); box.set_margin_start(40)
    box.set_margin_end(40); box.set_margin_bottom(40)
    title = Gtk.Label(label=f"{name} failed to load")
    title.set_xalign(0)
    title.get_style_context().add_class("mackes-page-title")
    body = Gtk.Label(label=err)
    body.set_xalign(0)
    body.set_line_wrap(True)
    body.get_style_context().add_class("mackes-page-subtitle")
    box.pack_start(title, False, False, 0)
    box.pack_start(body, False, False, 0)
    return box


# Legacy nav keys → new nav keys (so Dashboard quick-action links keep working).
_LEGACY_KEY_MAP = {
    "appearance": "look_and_feel",
    "display": "devices", "keyboard": "devices", "mouse": "devices",
    "sound": "devices", "power": "devices",
    "wm": "system", "workspaces": "system", "session": "system",
    "notifications": "system", "default_apps": "system", "removable": "system",
    "datetime": "system",
    "apps_install": "apps", "apps_remove": "apps", "apps_installed": "apps",
    "drift": "maintain", "update": "maintain", "fonts": "maintain",
    "resources": "maintain", "health": "maintain", "deps": "maintain",
    "logs": "maintain", "repair": "maintain", "reset": "maintain",
    "uninstall": "maintain",
}
