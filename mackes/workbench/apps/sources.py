"""Apps → Sources & Repos — surfaces every app source on the machine and
lets the user enable/disable the optional ones.

What it covers (matches the `apply_third_party_repos` + `apply_flathub`
birthright steps that previously had no GUI surface):

* Flathub flatpak remote (per-user)
* fedora-workstation-repositories package (Chrome, Steam, NVIDIA repos
  shipped disabled)
* RPM Fusion free + nonfree
* Currently enabled dnf repos (read-only listing — copy `repo-id` to
  toggle in CLI; toggling these via GUI is a follow-up)

All reads run on a daemon thread so first-paint is instant.
"""
from __future__ import annotations

import shutil
import subprocess
import threading

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
    title_label,
)


# ---- Helpers --------------------------------------------------------------


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


def _rpm_installed(pkg: str) -> bool:
    try:
        r = subprocess.run(["rpm", "-q", pkg],
                           capture_output=True, timeout=5)
        return r.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def _flathub_present() -> bool:
    """True iff the Flathub remote is configured (user or system)."""
    if shutil.which("flatpak") is None:
        return False
    try:
        r = subprocess.run(["flatpak", "remotes", "--columns=name"],
                           capture_output=True, text=True, timeout=5)
        return "flathub" in (r.stdout or "")
    except (OSError, subprocess.TimeoutExpired):
        return False


def _list_enabled_repos() -> list[str]:
    """Return a sorted list of currently-enabled dnf repo IDs."""
    try:
        out = subprocess.check_output(
            ["dnf", "repolist", "--enabled", "--quiet"],
            text=True, timeout=10,
        )
    except (OSError, subprocess.CalledProcessError,
            subprocess.TimeoutExpired):
        return []
    repos = []
    for line in out.splitlines()[1:]:  # skip header
        parts = line.split()
        if parts and not parts[0].startswith("repo"):
            repos.append(parts[0])
    return sorted(repos)


# ---- The panel ------------------------------------------------------------


class SourcesPanel(Gtk.Box):
    """Apps → Sources & Repos full-page panel."""

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.set_margin_top(12); outer.set_margin_bottom(12)
        outer.set_margin_start(16); outer.set_margin_end(16)

        outer.pack_start(_breadcrumb(["Mackes Shell", "Apps", "Sources & Repos"]),
                         False, False, 0)
        title = Gtk.Label(label="Sources & Repos")
        title.set_xalign(0); title.get_style_context().add_class("mackes-page-title")
        outer.pack_start(title, False, False, 0)
        outer.pack_start(_page_subtitle(
            "Manage where your apps and updates come from. Enable "
            "Flathub for the largest Linux app store, or turn on RPM "
            "Fusion to get media codecs and proprietary drivers."
        ), False, False, 0)

        outer.pack_start(self._build_flathub_section(), False, False, 0)
        outer.pack_start(self._build_rpmfusion_section(), False, False, 0)
        outer.pack_start(self._build_workstation_repos_section(), False, False, 0)
        outer.pack_start(self._build_enabled_repos_section(), False, False, 0)

        self.pack_start(outer, True, True, 0)

    # ---- Sections --------------------------------------------------------

    def _build_flathub_section(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(section_header("Flathub"), False, False, 0)
        box.pack_start(section_description(
            "The largest Linux app store. Adds thousands of apps that "
            "aren't in Fedora's main repos, including newer versions "
            "of Firefox, OBS Studio, Discord, and many more."
        ), False, False, 0)

        self._flathub_status = Gtk.Label(label="(checking…)")
        self._flathub_status.set_xalign(0)
        box.pack_start(self._flathub_status, False, False, 0)

        self._flathub_btn = Gtk.Button(label="Add Flathub")
        self._flathub_btn.get_style_context().add_class("suggested-action")
        self._flathub_btn.connect("clicked", lambda *_: self._add_flathub())
        box.pack_start(self._flathub_btn, False, False, 0)

        threading.Thread(target=self._refresh_flathub, daemon=True).start()
        return box

    def _build_rpmfusion_section(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(section_header("RPM Fusion"), False, False, 0)
        box.pack_start(section_description(
            "Community-maintained repos with software Fedora can't ship "
            "for legal reasons — H.264/HEVC playback, restricted "
            "drivers, and more."
        ), False, False, 0)

        self._rpmfusion_status = Gtk.Label(label="(checking…)")
        self._rpmfusion_status.set_xalign(0)
        box.pack_start(self._rpmfusion_status, False, False, 0)

        self._rpmfusion_btn = Gtk.Button(label="Enable RPM Fusion (free + nonfree)")
        self._rpmfusion_btn.get_style_context().add_class("suggested-action")
        self._rpmfusion_btn.connect("clicked", lambda *_: self._add_rpmfusion())
        box.pack_start(self._rpmfusion_btn, False, False, 0)

        threading.Thread(target=self._refresh_rpmfusion, daemon=True).start()
        return box

    def _build_workstation_repos_section(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(section_header("Fedora workstation repos"), False, False, 0)
        box.pack_start(section_description(
            "Adds the off-by-default repo files for Google Chrome, "
            "Steam, NVIDIA drivers, and a few others. Each individual "
            "repo stays disabled until you turn it on; this just "
            "makes them available."
        ), False, False, 0)

        self._workstation_status = Gtk.Label(label="(checking…)")
        self._workstation_status.set_xalign(0)
        box.pack_start(self._workstation_status, False, False, 0)

        self._workstation_btn = Gtk.Button(
            label="Install fedora-workstation-repositories")
        self._workstation_btn.connect(
            "clicked", lambda *_: self._add_workstation_repos())
        box.pack_start(self._workstation_btn, False, False, 0)

        threading.Thread(target=self._refresh_workstation, daemon=True).start()
        return box

    def _build_enabled_repos_section(self) -> Gtk.Widget:
        box = panel_box()
        box.pack_start(section_header("Currently enabled repos"), False, False, 0)
        box.pack_start(section_description(
            "Every dnf repo your machine pulls updates from. Read-only "
            "for now — manage individual repos via `dnf config-manager` "
            "in a terminal."
        ), False, False, 0)

        self._repos_view = Gtk.TextView()
        self._repos_view.set_editable(False)
        self._repos_view.set_monospace(True)
        self._repos_view.set_cursor_visible(False)
        self._repos_view.get_buffer().set_text("(loading…)")
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroll.set_size_request(-1, 180)
        scroll.add(self._repos_view)
        box.pack_start(scroll, False, False, 0)

        threading.Thread(target=self._refresh_repos, daemon=True).start()
        return box

    # ---- Refreshers (run in worker threads) ------------------------------

    def _refresh_flathub(self) -> None:
        present = cached("sources.flathub_present",
                         factory=_flathub_present, ttl_s=30)
        text = ("✓ Flathub remote is configured."
                if present else "Flathub is not configured.")
        GLib.idle_add(self._flathub_status.set_text, text)
        GLib.idle_add(self._flathub_btn.set_sensitive, not present)
        GLib.idle_add(self._flathub_btn.set_label,
                      "Flathub already added" if present else "Add Flathub")

    def _refresh_rpmfusion(self) -> None:
        free = cached("sources.rpmfusion_free",
                      factory=lambda: _rpm_installed("rpmfusion-free-release"),
                      ttl_s=60)
        nonfree = cached("sources.rpmfusion_nonfree",
                         factory=lambda: _rpm_installed("rpmfusion-nonfree-release"),
                         ttl_s=60)
        if free and nonfree:
            text = "✓ RPM Fusion free + nonfree enabled."
            label = "Already enabled"
        elif free:
            text = "RPM Fusion free is enabled. Nonfree is missing."
            label = "Add RPM Fusion nonfree"
        else:
            text = "RPM Fusion is not enabled."
            label = "Enable RPM Fusion (free + nonfree)"
        GLib.idle_add(self._rpmfusion_status.set_text, text)
        GLib.idle_add(self._rpmfusion_btn.set_sensitive, not (free and nonfree))
        GLib.idle_add(self._rpmfusion_btn.set_label, label)

    def _refresh_workstation(self) -> None:
        present = cached("sources.workstation_repos",
                         factory=lambda: _rpm_installed("fedora-workstation-repositories"),
                         ttl_s=60)
        text = ("✓ fedora-workstation-repositories is installed."
                if present else "Off-by-default vendor repos not yet available.")
        GLib.idle_add(self._workstation_status.set_text, text)
        GLib.idle_add(self._workstation_btn.set_sensitive, not present)
        GLib.idle_add(self._workstation_btn.set_label,
                      "Already installed" if present
                      else "Install fedora-workstation-repositories")

    def _refresh_repos(self) -> None:
        repos = cached("sources.enabled_repos",
                       factory=_list_enabled_repos, ttl_s=30)
        text = "\n".join(repos) if repos else "(no repos found or dnf unreachable)"
        GLib.idle_add(self._repos_view.get_buffer().set_text, text)

    # ---- Apply handlers (use AdminSession) -------------------------------

    def _add_flathub(self) -> None:
        if shutil.which("flatpak") is None:
            self._flathub_status.set_text("flatpak is not installed.")
            return
        self._flathub_btn.set_sensitive(False)
        self._flathub_status.set_text("Adding Flathub…")

        def worker():
            try:
                r = subprocess.run(
                    ["flatpak", "remote-add", "--if-not-exists", "--user",
                     "flathub",
                     "https://dl.flathub.org/repo/flathub.flatpakrepo"],
                    capture_output=True, text=True, timeout=60,
                )
                rc, msg = r.returncode, (r.stdout or "") + (r.stderr or "")
            except (OSError, subprocess.TimeoutExpired) as e:
                rc, msg = 1, str(e)
            invalidate("sources.flathub_present")
            GLib.idle_add(self._refresh_flathub)
            if rc != 0:
                GLib.idle_add(self._flathub_status.set_text,
                              f"Add failed: {msg.strip().splitlines()[-1] if msg else rc}")
        threading.Thread(target=worker, daemon=True).start()

    def _add_rpmfusion(self) -> None:
        from mackes.birthright import _detect_fedora_version
        ver = _detect_fedora_version()
        if not ver:
            self._rpmfusion_status.set_text(
                "Could not detect Fedora version — install manually.")
            return
        urls = [
            f"https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-{ver}.noarch.rpm",
            f"https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-{ver}.noarch.rpm",
        ]
        self._rpmfusion_btn.set_sensitive(False)
        self._rpmfusion_status.set_text("Enabling RPM Fusion (asking for your password)…")

        def worker():
            rc, out = AdminSession.instance().run(
                ["dnf", "install", "-y", *urls], timeout=300)
            invalidate("sources.rpmfusion_free")
            invalidate("sources.rpmfusion_nonfree")
            GLib.idle_add(self._refresh_rpmfusion)
            if rc != 0:
                last = (out.strip().splitlines()[-1] if out.strip() else f"rc={rc}")
                GLib.idle_add(self._rpmfusion_status.set_text,
                              f"Enable failed: {last}")
        threading.Thread(target=worker, daemon=True).start()

    def _add_workstation_repos(self) -> None:
        self._workstation_btn.set_sensitive(False)
        self._workstation_status.set_text(
            "Installing fedora-workstation-repositories…")

        def worker():
            rc, out = AdminSession.instance().run(
                ["dnf", "install", "-y", "fedora-workstation-repositories"],
                timeout=300,
            )
            invalidate("sources.workstation_repos")
            GLib.idle_add(self._refresh_workstation)
            if rc != 0:
                last = (out.strip().splitlines()[-1] if out.strip() else f"rc={rc}")
                GLib.idle_add(self._workstation_status.set_text,
                              f"Install failed: {last}")
        threading.Thread(target=worker, daemon=True).start()


__all__ = ["SourcesPanel"]
