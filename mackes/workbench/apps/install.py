"""Apps → Install — curated app set.

C1, C2, C3 locks. Renders a checkbox row per CATALOG entry that's also
listed in the active preset's `apps.install` block. Each row shows the
backend (dnf / third-party / appimage / npm), the package name, and an
Install button. A bulk 'Install all selected' button sits at the top.

Third-party repo additions happen transparently — the log streams what
got added.
"""
from __future__ import annotations

import threading

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.app_mgmt import CATALOG, install_app, is_dnf_installed
from mackes.presets import default_preset, load_preset
from mackes.state import MackesState
from mackes.workbench._common import (
    info_label, panel_box, section_header, title_label,
)


_BACKEND_LABEL = {
    "dnf": "Fedora repo",
    "dnf-thirdparty": "third-party repo",
    "appimage": "AppImage",
    "npm": "npm global",
}


def _active_preset_install_list() -> list[str]:
    state = MackesState.load()
    preset = None
    if state.active_preset:
        preset = load_preset(state.active_preset)
    if preset is None:
        preset = default_preset()
    if preset is None:
        return []
    return list(preset.apps.get("install") or [])


class AppsInstallPanel(Gtk.Box):
    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._build()

    def _build(self) -> None:
        box = panel_box()
        box.pack_start(title_label("Install curated apps"), False, False, 0)
        box.pack_start(info_label(
            "Mackes ships a curated install list per preset. Third-party "
            "repos (Microsoft, VS Code) and the npm global registry are "
            "enabled transparently as needed."
        ), False, False, 0)

        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        select_all = Gtk.Button(label="Select all not installed")
        select_all.connect("clicked", lambda *_: self._select_unselected())
        bulk = Gtk.Button(label="Install selected")
        bulk.get_style_context().add_class("suggested-action")
        bulk.connect("clicked", lambda *_: self._install_selected())
        refresh = Gtk.Button(label="Refresh")
        refresh.connect("clicked", lambda *_: self._refresh())
        bar.pack_start(select_all, False, False, 0)
        bar.pack_start(bulk, False, False, 0)
        bar.pack_start(refresh, False, False, 0)
        box.pack_start(bar, False, False, 0)

        box.pack_start(section_header("Apps"), False, False, 0)
        self._rows_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        box.pack_start(self._rows_box, False, False, 0)

        self._log = Gtk.TextView(); self._log.set_editable(False)
        self._log.set_monospace(True); self._log.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        log_scroll = Gtk.ScrolledWindow(); log_scroll.set_min_content_height(160)
        log_scroll.add(self._log)
        box.pack_start(section_header("Log"), False, False, 0)
        box.pack_start(log_scroll, True, True, 0)

        self.add(box)
        self._rows: list[tuple[Gtk.CheckButton, str]] = []
        self._refresh()

    def _refresh(self) -> None:
        for child in list(self._rows_box.get_children()):
            self._rows_box.remove(child)
        self._rows = []
        for name in _active_preset_install_list():
            app = CATALOG.get(name)
            if app is None:
                continue
            installed = is_dnf_installed(app.package or app.name)
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
            check = Gtk.CheckButton(); check.set_sensitive(not installed)
            row.pack_start(check, False, False, 0)
            self._rows.append((check, name))

            label = Gtk.Label(label=app.display); label.set_xalign(0)
            label.set_size_request(180, -1)
            row.pack_start(label, False, False, 0)

            badge = Gtk.Label(label=_BACKEND_LABEL.get(app.backend, app.backend))
            badge.get_style_context().add_class("dim-label")
            badge.set_size_request(150, -1); badge.set_xalign(0)
            row.pack_start(badge, False, False, 0)

            state_lbl = Gtk.Label(label="installed" if installed else app.description)
            state_lbl.set_xalign(0); state_lbl.set_line_wrap(True)
            state_lbl.get_style_context().add_class("dim-label" if not installed else "success")
            row.pack_start(state_lbl, True, True, 0)

            inst_btn = Gtk.Button(label="Install"); inst_btn.set_sensitive(not installed)
            inst_btn.connect("clicked", lambda *_a, n=name: self._install_one(n))
            row.pack_end(inst_btn, False, False, 0)
            self._rows_box.pack_start(row, False, False, 0)
        if not self._rows:
            self._rows_box.pack_start(info_label(
                "No curated apps declared in the active preset."
            ), False, False, 0)
        self._rows_box.show_all()

    def _select_unselected(self) -> None:
        for check, _ in self._rows:
            if check.get_sensitive():
                check.set_active(True)

    def _install_one(self, name: str) -> None:
        self._append_log(f"--- installing {name} ---")
        self._run_async(lambda: install_app(name))

    def _install_selected(self) -> None:
        names = [n for c, n in self._rows if c.get_active() and c.get_sensitive()]
        if not names:
            self._append_log("(no apps selected)")
            return
        self._append_log(f"--- installing {len(names)} apps: {', '.join(names)} ---")
        def runner() -> list[str]:
            actions: list[str] = []
            for n in names:
                actions.extend(install_app(n))
            return actions
        self._run_async(runner)

    def _run_async(self, fn) -> None:
        def worker() -> None:
            try:
                actions = fn()
            except Exception as e:  # noqa: BLE001
                actions = [f"error: {e}"]
            GLib.idle_add(self._finish, actions)

        threading.Thread(target=worker, daemon=True).start()

    def _finish(self, actions: list[str]) -> bool:
        for line in actions:
            self._append_log(line)
        self._refresh()
        return False

    def _append_log(self, line: str) -> None:
        buf = self._log.get_buffer()
        end = buf.get_end_iter()
        buf.insert(end, line + "\n")
