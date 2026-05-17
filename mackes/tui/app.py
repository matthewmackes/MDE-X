"""MackesTUI — Textual App.

Mirrors the GUI's structure: 48-line header (brand + preset chip),
26-column sidebar nav, content panel for the active screen, 2-line
status bar. Carbon-Gray-100 palette via CSS.

Nav groups:
  Workbench  — Dashboard
  Network    — Mesh VPN, Mesh SSH, Mesh Services, Mesh Remote
  Fleet      — Inventory, Playbooks, Run history
  Tools      — Snapshots, Debloat levels
  Reference  — Help

Bindings:
  q / ctrl+q       quit
  r                refresh current screen
  ?                help overlay
  digits 1..9      jump to nav item N
"""
from __future__ import annotations

from typing import Optional

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Container, Horizontal, Vertical
from textual.screen import Screen
from textual.widgets import (
    Footer, Header, Label, ListItem, ListView, Static,
)


# Carbon Gray 100 palette — translated into Textual's TCSS.
MACKES_CSS = """
Screen {
    background: #161616;
    color: #f4f4f4;
}

#shell-header {
    height: 3;
    background: #161616;
    border-bottom: solid #393939;
    padding: 0 2;
}

#brand {
    width: 28;
    color: #f1853d;
    text-style: bold;
}

#brand-light {
    color: #c6c6c6;
    text-style: none;
}

#preset-chip {
    color: #c6c6c6;
    background: #262626;
    padding: 0 2;
    height: 1;
}

#shell-body {
    height: 1fr;
}

#sidenav {
    width: 30;
    background: #161616;
    border-right: solid #393939;
}

#sidenav > .group-title {
    color: #8d8d8d;
    text-style: bold;
    padding: 1 2 0 2;
}

#sidenav ListItem {
    padding: 0 2;
    background: transparent;
    color: #c6c6c6;
    height: 1;
}

#sidenav ListItem.--highlight {
    background: #262626;
    color: #f4f4f4;
    border-left: thick #f1853d;
}

#content {
    width: 1fr;
    background: #161616;
    padding: 1 3;
}

#content .page-title {
    text-style: bold;
    color: #f4f4f4;
}

#content .page-sub {
    color: #c6c6c6;
}

#content .section-title {
    color: #f4f4f4;
    text-style: bold;
    padding: 1 0 0 0;
}

#content .stat-label {
    color: #8d8d8d;
    text-style: bold;
}

#content .stat-value {
    color: #f4f4f4;
    text-style: bold;
}

#content .ok {
    color: #42be65;
}

#content .warn {
    color: #f1c21b;
}

#content .fail {
    color: #fa4d56;
}

#content .accent {
    color: #f1853d;
}

#content .code {
    color: #c6c6c6;
    background: #262626;
    padding: 1 2;
}

Footer {
    background: #161616;
}
"""


_NAV = [
    ("Workbench", [
        ("dashboard", "Dashboard"),
    ]),
    ("Network", [
        ("mesh_vpn",      "Mesh VPN"),
        ("mesh_ssh",      "Mesh SSH"),
        ("mesh_services", "Mesh Services"),
        ("mesh_remote",   "Mesh Remote"),
    ]),
    ("Fleet", [
        ("fleet_inventory", "Inventory"),
        ("fleet_playbooks", "Playbooks"),
        ("fleet_runs",      "Run history"),
    ]),
    ("Tools", [
        ("snapshots", "Snapshots"),
        ("debloat",   "Debloat levels"),
    ]),
    ("Reference", [
        ("help", "Help"),
    ]),
]


class MackesTUI(App):
    """The Textual app. Owns the screen registry + nav state."""

    CSS = MACKES_CSS
    BINDINGS = [
        Binding("q", "quit", "Quit", show=True),
        Binding("ctrl+q", "quit", "Quit", show=False),
        Binding("r", "refresh", "Refresh", show=True),
        Binding("question_mark", "toggle_help", "Help", show=True),
    ]
    TITLE = "Mackes Shell — TUI"

    def __init__(self) -> None:
        super().__init__()
        from mackes import __version__
        self.version = __version__
        from mackes.state import MackesState
        try:
            self._state = MackesState.load()
        except Exception:  # noqa: BLE001
            self._state = None
        self._active_key = "dashboard"

    def compose(self) -> ComposeResult:
        yield Header(show_clock=False)
        with Horizontal(id="shell-header"):
            yield Static("[b]Mackes[/b] [#c6c6c6]Shell[/#c6c6c6]", id="brand")
            yield Static("", id="header-pad")
            preset = (self._state.active_preset if self._state else "—") or "—"
            yield Static(f"preset · {preset}", id="preset-chip")
        with Horizontal(id="shell-body"):
            yield self._build_sidenav()
            yield Container(id="content")
        yield Footer()

    def on_mount(self) -> None:
        # Activate the default screen
        self._switch_to(self._active_key)

    # ---- nav -----------------------------------------------------------

    def _build_sidenav(self) -> Container:
        wrapper = Container(id="sidenav")
        # Build a flat list of ListItems and section labels; the active
        # tracking uses ListItem children of a single ListView.
        items = []
        self._nav_keys: list[str] = []
        for group_title, group_items in _NAV:
            items.append(ListItem(Label(f"[b dim]{group_title.upper()}[/]"),
                                  classes="group-title", disabled=True))
            for key, label in group_items:
                items.append(ListItem(Label(label), id=f"nav-{key}"))
                self._nav_keys.append(key)
        self._listview = ListView(*items, id="sidenav-list")
        wrapper.compose_add_child(self._listview)
        return wrapper

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        if event.item is None or event.item.id is None:
            return
        if event.item.id.startswith("nav-"):
            key = event.item.id[4:]
            self._switch_to(key)

    def _switch_to(self, key: str) -> None:
        self._active_key = key
        content = self.query_one("#content", Container)
        # Clear children
        for child in list(content.children):
            child.remove()
        screen = _build_screen(key)
        content.mount(screen)

    # ---- bindings ------------------------------------------------------

    def action_refresh(self) -> None:
        self._switch_to(self._active_key)

    def action_toggle_help(self) -> None:
        self._switch_to("help")


# ---------------------------------------------------------------------------
# Screen builder — returns a Container suitable for mounting into #content
# ---------------------------------------------------------------------------


def _build_screen(key: str) -> Container:
    """Build the active screen widget tree."""
    from mackes.tui.screens import (
        dashboard, mesh_vpn, mesh_ssh, mesh_services, mesh_remote,
        fleet_inventory, fleet_playbooks, fleet_runs,
        snapshots, debloat, help_screen,
    )
    builders = {
        "dashboard":        dashboard.build,
        "mesh_vpn":         mesh_vpn.build,
        "mesh_ssh":         mesh_ssh.build,
        "mesh_services":    mesh_services.build,
        "mesh_remote":      mesh_remote.build,
        "fleet_inventory":  fleet_inventory.build,
        "fleet_playbooks":  fleet_playbooks.build,
        "fleet_runs":       fleet_runs.build,
        "snapshots":        snapshots.build,
        "debloat":          debloat.build,
        "help":             help_screen.build,
    }
    fn = builders.get(key)
    if fn is None:
        return Container(Static(f"(unknown screen: {key})"))
    return fn()
