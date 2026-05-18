"""System → Boot & Login — combined Plymouth + LightDM greeter panel.

What it covers (matches the `apply_plymouth` + `apply_lightdm` birthright
steps that previously had no dedicated GUI surface beyond multi-monitor
greeter routing in System → Screens):

* Plymouth theme selector (lists every theme under
  /usr/share/plymouth/themes/; current default read from
  `plymouth-set-default-theme`)
* LightDM greeter background image
* Auto-login toggle (`/etc/lightdm/lightdm.conf:autologin-user`)
* Greeter clock format + indicators (read-only display)

All writes route through AdminSession (sudoers NOPASSWD already covers
plymouth-set-default-theme + the lightdm.conf paths via the Mackes
drop-in).

Plymouth theme changes require an initrd rebuild — that's heavy
(~30 s) so it runs on a daemon thread with status updates posted via
GLib.idle_add.
"""
from __future__ import annotations

import shutil
import subprocess
import threading
from pathlib import Path

import gi
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

from mackes.admin_session import AdminSession
from mackes.probe_cache import cached, invalidate
from mackes.workbench._common import (
    info_label,
    panel_box,
    section_description,
    section_header,
)


PLYMOUTH_THEMES_DIR = Path("/usr/share/plymouth/themes")
LIGHTDM_CONF = Path("/etc/lightdm/lightdm.conf")
LIGHTDM_GREETER_CONF = Path("/etc/lightdm/lightdm-gtk-greeter.conf")


# ---- shared layout helpers ------------------------------------------------


def _breadcrumb(parts: list[str]) -> Gtk.Widget:
    bc = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
    bc.get_style_context().add_class("mackes-breadcrumb")
    for i, p in enumerate(parts):
        lab = Gtk.Label(label=p); lab.set_xalign(0)
        bc.pack_start(lab, False, False, 0)
        if i != len(parts) - 1:
            sep = Gtk.Label(label="/"); sep.set_xalign(0)
            sep.get_style_context().add_class("mackes-dot")
            bc.pack_start(sep, False, False, 0)
    return bc


def _page_subtitle(text: str) -> Gtk.Widget:
    lab = Gtk.Label(label=text)
    lab.set_xalign(0); lab.set_line_wrap(True)
    lab.get_style_context().add_class("mackes-page-subtitle")
    return lab


# ---- probes ---------------------------------------------------------------


def _list_plymouth_themes() -> list[str]:
    if not PLYMOUTH_THEMES_DIR.is_dir():
        return []
    return sorted(p.name for p in PLYMOUTH_THEMES_DIR.iterdir()
                  if p.is_dir() and not p.name.startswith("."))


def _current_plymouth_theme() -> str:
    if shutil.which("plymouth-set-default-theme") is None:
        return "(plymouth not installed)"
    try:
        r = subprocess.run(["plymouth-set-default-theme"],
                           capture_output=True, text=True, timeout=5)
        return (r.stdout or "").strip() or "(unknown)"
    except (OSError, subprocess.TimeoutExpired):
        return "(unknown)"


def _read_ini_value(path: Path, section: str, key: str) -> str:
    """Best-effort INI section/key read. Returns '' if not present."""
    if not path.is_file():
        return ""
    try:
        in_section = False
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("[") and line.endswith("]"):
                in_section = (line[1:-1] == section)
                continue
            if in_section and "=" in line:
                k, _, v = line.partition("=")
                if k.strip() == key:
                    return v.strip()
        return ""
    except OSError:
        return ""


def _lightdm_status() -> dict:
    """Return {autologin_user, greeter_bg, greeter_indicators, greeter_clock}."""
    return {
        "autologin_user": _read_ini_value(LIGHTDM_CONF, "Seat:*", "autologin-user"),
        "greeter_bg":     _read_ini_value(LIGHTDM_GREETER_CONF, "greeter", "background"),
        "greeter_clock":  _read_ini_value(LIGHTDM_GREETER_CONF, "greeter", "clock-format"),
        "greeter_indicators": _read_ini_value(LIGHTDM_GREETER_CONF, "greeter", "indicators"),
    }


# ---- The panel ------------------------------------------------------------


class BootLoginPanel(Gtk.Box):
    """System → Boot & Login full-page panel."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(["Mackes Shell", "System", "Boot & Login"]),
                         False, False, 0)
        title = Gtk.Label(label="Boot & Login")
        title.set_xalign(0); title.get_style_context().add_class("mackes-page-title")
        outer.pack_start(title, False, False, 0)
        outer.pack_start(_page_subtitle(
            "Change what your computer looks like as it starts up — "
            "from the boot animation to the login screen."
        ), False, False, 0)

        outer.pack_start(self._build_plymouth_section(), False, False, 0)
        outer.pack_start(self._build_autologin_section(), False, False, 0)
        outer.pack_start(self._build_greeter_section(), False, False, 0)

        self.pack_start(outer, True, True, 0)

    # ---- Sections --------------------------------------------------------

    def _build_plymouth_section(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(section_header("Boot animation (Plymouth)"), False, False, 0)
        box.pack_start(section_description(
            "Pick the animation that plays while your computer is "
            "starting up. Changing this rebuilds part of the system "
            "and can take a minute."
        ), False, False, 0)

        self._plymouth_current = Gtk.Label(label="Current: (checking…)")
        self._plymouth_current.set_xalign(0)
        box.pack_start(self._plymouth_current, False, False, 0)

        self._plymouth_combo = Gtk.ComboBoxText()
        box.pack_start(self._plymouth_combo, False, False, 0)

        self._plymouth_apply = Gtk.Button(label="Apply boot theme")
        self._plymouth_apply.get_style_context().add_class("suggested-action")
        self._plymouth_apply.connect("clicked", lambda *_: self._apply_plymouth())
        box.pack_start(self._plymouth_apply, False, False, 0)

        self._plymouth_status = Gtk.Label(label="")
        self._plymouth_status.set_xalign(0); self._plymouth_status.set_line_wrap(True)
        box.pack_start(self._plymouth_status, False, False, 0)

        threading.Thread(target=self._refresh_plymouth, daemon=True).start()
        return box

    def _build_autologin_section(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(section_header("Auto-login"), False, False, 0)
        box.pack_start(section_description(
            "Skip the login screen and start your session right after "
            "boot. Convenient on a personal machine; not recommended "
            "for shared computers."
        ), False, False, 0)

        self._autologin_status = Gtk.Label(label="(checking…)")
        self._autologin_status.set_xalign(0)
        box.pack_start(self._autologin_status, False, False, 0)

        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        row.set_margin_top(8)
        self._autologin_entry = Gtk.Entry()
        self._autologin_entry.set_placeholder_text("username to auto-log-in")
        row.pack_start(self._autologin_entry, True, True, 0)
        self._autologin_apply = Gtk.Button(label="Set auto-login")
        self._autologin_apply.connect("clicked", lambda *_: self._apply_autologin(True))
        row.pack_start(self._autologin_apply, False, False, 0)
        self._autologin_disable = Gtk.Button(label="Disable auto-login")
        self._autologin_disable.connect("clicked", lambda *_: self._apply_autologin(False))
        row.pack_start(self._autologin_disable, False, False, 0)
        box.pack_start(row, False, False, 0)

        threading.Thread(target=self._refresh_autologin, daemon=True).start()
        return box

    def _build_greeter_section(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(section_header("Login screen (greeter)"), False, False, 0)
        box.pack_start(section_description(
            "Mackes already styles the LightDM login screen with the "
            "Carbon design system. The details below are read-only — "
            "the multi-monitor 'where to show the login' setting is "
            "on the Screens panel."
        ), False, False, 0)

        self._greeter_view = Gtk.TextView()
        self._greeter_view.set_editable(False)
        self._greeter_view.set_monospace(True)
        self._greeter_view.set_cursor_visible(False)
        self._greeter_view.get_buffer().set_text("(loading…)")
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroll.set_size_request(-1, 140)
        scroll.add(self._greeter_view)
        box.pack_start(scroll, False, False, 0)

        info = info_label(
            "To change the wallpaper, open Screens or set a new image at "
            "/usr/share/mackes-shell/branding/standard-wallpaper.png and "
            "re-apply your preset."
        )
        box.pack_start(info, False, False, 0)

        threading.Thread(target=self._refresh_greeter, daemon=True).start()
        return box

    # ---- Threaded refreshers --------------------------------------------

    def _refresh_plymouth(self) -> None:
        themes = cached("boot.plymouth_themes",
                        factory=_list_plymouth_themes, ttl_s=300)
        current = cached("boot.plymouth_current",
                         factory=_current_plymouth_theme, ttl_s=10)

        def apply():
            self._plymouth_combo.remove_all()
            for t in themes:
                self._plymouth_combo.append(t, t)
            if current in themes:
                self._plymouth_combo.set_active_id(current)
            elif themes:
                self._plymouth_combo.set_active(0)
            self._plymouth_current.set_text(f"Current: {current}")
        GLib.idle_add(apply)

    def _refresh_autologin(self) -> None:
        status = cached("boot.lightdm_status",
                        factory=_lightdm_status, ttl_s=30)
        user = status.get("autologin_user", "")
        if user:
            text = f"✓ Auto-login is on for user: {user}"
        else:
            text = "Auto-login is off — the login screen appears at boot."

        def apply():
            self._autologin_status.set_text(text)
            self._autologin_entry.set_text(user)
        GLib.idle_add(apply)

    def _refresh_greeter(self) -> None:
        status = cached("boot.lightdm_status",
                        factory=_lightdm_status, ttl_s=30)
        lines = [
            f"background    = {status.get('greeter_bg','(default)')}",
            f"clock-format  = {status.get('greeter_clock','(default)')}",
            f"indicators    = {status.get('greeter_indicators','(default)')}",
        ]
        GLib.idle_add(self._greeter_view.get_buffer().set_text,
                      "\n".join(lines))

    # ---- Apply handlers --------------------------------------------------

    def _apply_plymouth(self) -> None:
        theme = self._plymouth_combo.get_active_id()
        if not theme:
            return
        self._plymouth_apply.set_sensitive(False)
        self._plymouth_status.set_text(
            f"Setting {theme} and rebuilding initrd — this can take a minute…")

        def worker():
            if shutil.which("plymouth-set-default-theme") is None:
                rc, out = 1, "plymouth-set-default-theme is not installed"
            else:
                rc, out = AdminSession.instance().run(
                    ["plymouth-set-default-theme", "-R", theme], timeout=300)
            invalidate("boot.plymouth_current")
            if rc == 0:
                msg = f"✓ {theme} applied. Reboot to see it."
            else:
                last = out.strip().splitlines()[-1] if out.strip() else f"rc={rc}"
                msg = f"Apply failed: {last}"
            GLib.idle_add(self._plymouth_status.set_text, msg)
            GLib.idle_add(self._plymouth_apply.set_sensitive, True)
            GLib.idle_add(self._refresh_plymouth)
        threading.Thread(target=worker, daemon=True).start()

    def _apply_autologin(self, enabled: bool) -> None:
        if enabled:
            user = self._autologin_entry.get_text().strip()
            if not user:
                self._autologin_status.set_text(
                    "Type a username first.")
                return
        else:
            user = ""

        def worker():
            # Rewrite lightdm.conf via a small sed-style INI patch
            ok = self._write_autologin(user)
            invalidate("boot.lightdm_status")
            GLib.idle_add(self._refresh_autologin)
            GLib.idle_add(
                self._autologin_status.set_text,
                "✓ Saved. Takes effect at next login." if ok
                else "Could not save — see Logs panel for details.",
            )
        threading.Thread(target=worker, daemon=True).start()

    def _write_autologin(self, user: str) -> bool:
        """Set or unset autologin-user in /etc/lightdm/lightdm.conf.

        Reads the existing file, patches the [Seat:*] section in memory,
        writes to /tmp, then installs via AdminSession.
        """
        try:
            current = (LIGHTDM_CONF.read_text(encoding="utf-8")
                       if LIGHTDM_CONF.is_file() else "")
        except OSError:
            current = ""
        lines = current.splitlines() if current else ["[Seat:*]"]
        in_seat, found_key = False, False
        out_lines: list[str] = []
        for line in lines:
            s = line.strip()
            if s.startswith("[") and s.endswith("]"):
                in_seat = (s == "[Seat:*]")
                out_lines.append(line)
                continue
            if in_seat and s.startswith(("autologin-user=", "autologin-user ")):
                found_key = True
                if user:
                    out_lines.append(f"autologin-user={user}")
                # else: drop the line (disable autologin)
                continue
            out_lines.append(line)
        if "[Seat:*]" not in current:
            out_lines.insert(0, "[Seat:*]")
        if user and not found_key:
            # Inject just after [Seat:*] header
            for i, ln in enumerate(out_lines):
                if ln.strip() == "[Seat:*]":
                    out_lines.insert(i + 1, f"autologin-user={user}")
                    break
        new_text = "\n".join(out_lines).rstrip() + "\n"

        import tempfile
        with tempfile.NamedTemporaryFile(mode="w", delete=False,
                                          suffix=".conf",
                                          encoding="utf-8") as tmp:
            tmp.write(new_text)
            tmp_path = tmp.name
        rc, _ = AdminSession.instance().run(
            ["install", "-D", "-m", "0644", tmp_path, str(LIGHTDM_CONF)],
            timeout=10,
        )
        try:
            Path(tmp_path).unlink()
        except OSError:
            pass
        return rc == 0


__all__ = ["BootLoginPanel"]
