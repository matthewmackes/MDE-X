"""Apps — unified tabbed panel (Carbon refresh, v1.1.1).

Mirrors docs/design/v1.1.0-carbon-refresh/project/panels-b.jsx::AppsPanel:
  - Carbon tabs (Install / Remove bloat / Installed)
  - Category filter chips
  - Search input
  - Grid of Carbon app cards

Wires to the existing mackes.app_mgmt catalog + install/remove backend.
"""
from __future__ import annotations

from typing import List, Optional

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.app_mgmt import (
    CATALOG, AppDef, install_app, is_dnf_installed, remove_packages,
)
from mackes.presets import default_preset, load_preset
from mackes.state import MackesState


# Category derivation — apps catalog doesn't carry an explicit category
# field, so synthesize one from the backend + a small hand-tuned map.
_CATEGORY_OVERRIDES = {
    "filezilla":      "Internet",
    "terminator":     "System",
    "vlc":            "Multimedia",
    "remmina":        "Internet",
    "mc":             "System",
    "neofetch":       "System",
    "fastfetch":      "System",
    "microsoft-edge-stable": "Internet",
    "code":           "Development",
    "cursor":         "Development",
    "claude-code":    "Development",
}


def _category_for(app: AppDef) -> str:
    if app.name in _CATEGORY_OVERRIDES:
        return _CATEGORY_OVERRIDES[app.name]
    if app.backend in ("dnf-thirdparty", "appimage"):
        return "Third-party"
    return "System"


def _is_installed(app: AppDef) -> bool:
    if app.backend in ("dnf", "dnf-thirdparty"):
        return is_dnf_installed(app.package or app.name)
    if app.backend == "appimage":
        from mackes.state import HOME
        return (HOME / ".local" / "bin" / app.name).exists()
    if app.backend == "npm":
        # rough check: npm root -g + look for the package
        import shutil, subprocess
        if shutil.which("npm") is None:
            return False
        try:
            out = subprocess.check_output(
                ["npm", "ls", "-g", "--depth=0", app.package or app.name],
                text=True, stderr=subprocess.DEVNULL, timeout=10,
            )
            return (app.package or app.name) in out
        except Exception:  # noqa: BLE001
            return False
    return False


def _preset_install_list() -> List[str]:
    state = MackesState.load()
    preset = load_preset(state.active_preset) if state.active_preset else None
    if preset is None:
        preset = default_preset()
    if preset is None:
        return []
    return list(preset.apps.get("install") or [])


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
    for i, p in enumerate(("Mackes Shell", "Apps")):
        lab = Gtk.Label(label=p); lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != 1:
            sep = Gtk.Label(label="/"); sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _tag(text: str, kind: str = "neutral") -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.get_style_context().add_class("mackes-tag")
    lab.get_style_context().add_class(kind)
    return lab


# ---- main panel -----------------------------------------------------------


class AppsPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._active_tab: str = "install"   # install | remove | installed
        self._active_category: str = "all"
        self._search_q: str = ""
        self._build()
        self._refresh_grid()

    def _build(self) -> None:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(32); outer.set_margin_bottom(32)
        outer.set_margin_start(40); outer.set_margin_end(40)

        outer.pack_start(_breadcrumb(), False, False, 0)
        outer.pack_start(_page_title("Apps"), False, False, 0)
        n_installed = sum(1 for app in CATALOG.values() if _is_installed(app))
        outer.pack_start(_page_subtitle(
            f"Curated app catalog per preset. Install, remove, and inspect "
            f"the {n_installed} packages currently on this machine."
        ), False, False, 0)

        # ---- Tabs ----
        tabs_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        tabs_row.set_margin_top(16)
        tabs_row.get_style_context().add_class("mackes-tabs")
        self._tab_buttons = {}
        for key, label in (("install", "Install"),
                           ("remove",  "Remove bloat"),
                           ("installed", f"Installed ({n_installed})")):
            btn = Gtk.ToggleButton(label=label)
            btn.get_style_context().add_class("mackes-tab")
            btn.set_relief(Gtk.ReliefStyle.NONE)
            btn.connect("toggled", self._on_tab_toggled, key)
            self._tab_buttons[key] = btn
            tabs_row.pack_start(btn, False, False, 0)
        self._tab_buttons["install"].set_active(True)
        outer.pack_start(tabs_row, False, False, 0)

        # ---- Category chips + search ----
        controls = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        controls.set_margin_top(16); controls.set_margin_bottom(16)

        self._chips_box = Gtk.FlowBox()
        self._chips_box.set_max_children_per_line(20)
        self._chips_box.set_selection_mode(Gtk.SelectionMode.NONE)
        self._chips_box.set_column_spacing(8); self._chips_box.set_row_spacing(8)
        controls.pack_start(self._chips_box, True, True, 0)

        self._search = Gtk.SearchEntry()
        self._search.set_placeholder_text("Search apps…")
        self._search.set_size_request(280, -1)
        self._search.connect("search-changed", self._on_search_changed)
        controls.pack_end(self._search, False, False, 0)
        outer.pack_start(controls, False, False, 0)

        # ---- App grid ----
        self._grid = Gtk.FlowBox()
        self._grid.set_valign(Gtk.Align.START)
        self._grid.set_max_children_per_line(3)
        self._grid.set_min_children_per_line(1)
        self._grid.set_selection_mode(Gtk.SelectionMode.NONE)
        self._grid.set_homogeneous(True)
        self._grid.set_column_spacing(8); self._grid.set_row_spacing(8)
        outer.pack_start(self._grid, False, False, 0)

        # ---- Log / status ----
        log_head = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        log_head.set_margin_top(24)
        log_title = Gtk.Label(label="Activity log")
        log_title.set_xalign(0)
        log_title.get_style_context().add_class("mackes-section-title")
        log_head.pack_start(log_title, True, True, 0)
        outer.pack_start(log_head, False, False, 0)
        self._log = Gtk.TextView()
        self._log.set_editable(False); self._log.set_monospace(True)
        self._log.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        log_scroll = Gtk.ScrolledWindow()
        log_scroll.set_min_content_height(140)
        log_scroll.add(self._log)
        outer.pack_start(log_scroll, False, False, 0)

        # Whole panel scrolls
        scroller = Gtk.ScrolledWindow()
        scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroller.add(outer)
        self.pack_start(scroller, True, True, 0)

    # ---- handlers ---------------------------------------------------------

    def _on_tab_toggled(self, btn: Gtk.ToggleButton, key: str) -> None:
        if not btn.get_active():
            return
        for k, b in self._tab_buttons.items():
            if k != key and b.get_active():
                b.set_active(False)
        self._active_tab = key
        self._refresh_grid()

    def _on_chip_clicked(self, _btn, category: str) -> None:
        self._active_category = category
        self._refresh_grid()

    def _on_search_changed(self, entry: Gtk.SearchEntry) -> None:
        self._search_q = entry.get_text().strip().lower()
        self._refresh_grid()

    # ---- grid render ------------------------------------------------------

    def _refresh_grid(self) -> None:
        apps = list(CATALOG.values())

        # Tab filter
        if self._active_tab == "install":
            apps = [a for a in apps if not _is_installed(a)]
        elif self._active_tab == "remove":
            # Show items that came from the active preset's remove_bloat
            # list (and are currently installed). This is the Q15 lock list
            # — never a free-form "remove anything" UI.
            state = MackesState.load()
            preset = load_preset(state.active_preset) if state.active_preset else None
            if preset is None:
                preset = default_preset()
            bloat = (preset.apps.get("remove_bloat") if preset else []) or []
            bloat_set = set(bloat)
            apps = [a for a in apps
                    if (a.package or a.name) in bloat_set or a.name in bloat_set]
            if not apps:
                # Show the literal bloat-list items as synthetic AppDefs
                from mackes.app_mgmt import AppDef as _AppDef
                apps = [
                    _AppDef(name=name, display=name, backend="dnf",
                            description="(declared in preset.apps.remove_bloat)")
                    for name in bloat
                ]
            apps = [a for a in apps if _is_installed(a)]
        elif self._active_tab == "installed":
            apps = [a for a in apps if _is_installed(a)]

        # Category filter
        categories = sorted({_category_for(a) for a in CATALOG.values()})
        if self._active_category != "all":
            apps = [a for a in apps if _category_for(a) == self._active_category]

        # Search filter
        if self._search_q:
            apps = [a for a in apps
                    if self._search_q in (a.name + " " + a.display + " "
                                          + a.description).lower()]

        # Rebuild chip row
        for c in list(self._chips_box.get_children()):
            self._chips_box.remove(c)
        self._chips_box.add(self._make_chip("all", "All categories",
                                            active=(self._active_category == "all")))
        for cat in categories:
            self._chips_box.add(self._make_chip(cat, cat,
                                                active=(self._active_category == cat)))
        self._chips_box.show_all()

        # Rebuild app grid
        for c in list(self._grid.get_children()):
            self._grid.remove(c)
        if not apps:
            empty = Gtk.Label(label="No apps match your filters.")
            empty.set_xalign(0); empty.set_margin_top(40)
            empty.get_style_context().add_class("dim-label")
            self._grid.add(empty)
        else:
            preset_install = set(_preset_install_list())
            for app in apps:
                self._grid.add(self._make_app_card(app,
                                                   in_preset=(app.name in preset_install)))
        self._grid.show_all()

    def _make_chip(self, key: str, label: str, *, active: bool) -> Gtk.Widget:
        btn = Gtk.Button(label=label)
        btn.set_relief(Gtk.ReliefStyle.NONE)
        btn.get_style_context().add_class("mackes-tag")
        btn.get_style_context().add_class("accent" if active else "neutral")
        btn.connect("clicked", self._on_chip_clicked, key)
        return btn

    def _make_app_card(self, app: AppDef, *, in_preset: bool) -> Gtk.Widget:
        card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        card.get_style_context().add_class("mackes-app-card")
        card.set_size_request(-1, 160)

        # Top row: icon + pills
        top = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        icon_letter = (app.display or app.name)[0:1].upper()
        icon = Gtk.Label(label=icon_letter)
        icon.set_size_request(40, 40)
        icon.get_style_context().add_class("mackes-app-icon")
        top.pack_start(icon, False, False, 0)
        # Right-side pills
        pills = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        if in_preset:
            pills.pack_end(_tag("Preset", "accent"), False, False, 0)
        if _is_installed(app):
            pills.pack_end(_tag("Installed", "success"), False, False, 0)
        top.pack_end(pills, False, False, 0)
        card.pack_start(top, False, False, 0)

        # Name + description
        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        name = Gtk.Label(label=app.display)
        name.set_xalign(0)
        name.get_style_context().add_class("mackes-app-name")
        body.pack_start(name, False, False, 0)
        desc = Gtk.Label(label=app.description or "")
        desc.set_xalign(0); desc.set_line_wrap(True)
        desc.set_max_width_chars(40)
        desc.get_style_context().add_class("mackes-app-desc")
        body.pack_start(desc, True, True, 0)
        card.pack_start(body, True, True, 0)

        # Footer: meta + action
        foot = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        meta = Gtk.Label(label=f"{_category_for(app)} · {app.backend}")
        meta.set_xalign(0)
        meta.get_style_context().add_class("mackes-app-meta")
        foot.pack_start(meta, True, True, 0)

        if self._active_tab == "install":
            action_btn = Gtk.Button(label="Install")
            action_btn.get_style_context().add_class("cds-button-tertiary")
            action_btn.connect("clicked", self._on_install_clicked, app)
        elif self._active_tab == "remove":
            action_btn = Gtk.Button(label="Remove")
            action_btn.get_style_context().add_class("destructive-action")
            action_btn.connect("clicked", self._on_remove_clicked, app)
        else:  # installed
            action_btn = Gtk.Button(label="Open")
            action_btn.get_style_context().add_class("cds-button-ghost")
            action_btn.connect("clicked", self._on_open_clicked, app)
        foot.pack_end(action_btn, False, False, 0)
        card.pack_start(foot, False, False, 0)
        return card

    # ---- async install / remove / open -----------------------------------

    def _on_install_clicked(self, _btn, app: AppDef) -> None:
        self._append_log(f"→  Installing {app.display}…")
        import threading
        def run() -> None:
            try:
                lines = install_app(app.name)
            except Exception as e:  # noqa: BLE001
                lines = [f"install failed: {e}"]
            GLib.idle_add(self._after_action, app, lines)
        threading.Thread(target=run, daemon=True).start()

    def _on_remove_clicked(self, _btn, app: AppDef) -> None:
        self._append_log(f"→  Removing {app.display}…")
        import threading
        def run() -> None:
            try:
                lines = remove_packages([app.package or app.name], category="bloat")
            except Exception as e:  # noqa: BLE001
                lines = [f"remove failed: {e}"]
            GLib.idle_add(self._after_action, app, lines)
        threading.Thread(target=run, daemon=True).start()

    def _on_open_clicked(self, _btn, app: AppDef) -> None:
        # Best-effort: launch the app's .desktop file via gtk-launch or
        # the binary directly if we know where it is.
        import shutil, subprocess
        target = app.package or app.name
        for cmd_name in (target, app.name, app.display.split()[0].lower()):
            path = shutil.which(cmd_name)
            if path:
                subprocess.Popen([path], stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL,
                                 start_new_session=True)
                self._append_log(f"  launched {path}")
                return
        self._append_log(f"  could not find executable for {app.display}")

    def _after_action(self, app: AppDef, lines: list[str]) -> bool:
        for line in lines:
            self._append_log(f"  {line}")
        self._refresh_grid()
        return False

    def _append_log(self, text: str) -> None:
        buf = self._log.get_buffer()
        end = buf.get_end_iter()
        buf.insert(end, text + "\n")
        # auto-scroll
        end = buf.get_end_iter()
        self._log.scroll_to_iter(end, 0, False, 0, 1)
