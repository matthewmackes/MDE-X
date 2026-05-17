"""Birthright health check — verify the artifacts apply_* left behind.

After v1.4.0, the wizard ships 11 birthright steps. This module probes
for the on-disk artifacts each step *should* have produced. If items
are missing, the Workbench surfaces a Notification with a 'Re-run Setup
Wizard' CTA so the user can recover from a wizard that was cancelled
mid-flow or installed over an older state.

Public API:

  check_all()                → list[BirthrightItem]  (every probe + status)
  missing()                  → list[BirthrightItem]  (only failing items)
  is_complete()              → bool                  (no missing items)
"""
from __future__ import annotations

import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, List


@dataclass
class BirthrightItem:
    key: str             # short stable id ("themes", "fonts", "sudoers", ...)
    name: str            # display name
    detail: str          # what would fix it
    ok: bool


# ---------------------------------------------------------------------------
# Per-step probes (cheap — file/dir/systemctl checks, no shell-outs > 5s)
# ---------------------------------------------------------------------------


def _check_themes() -> BirthrightItem:
    pad = Path("/usr/share/themes/PadOS")
    carbon = Path("/usr/share/icons/Carbon")
    ok = pad.is_dir() and carbon.is_dir()
    return BirthrightItem(
        key="themes", name="Themes (PadOS GTK + Carbon icons)",
        detail="Re-run Setup Wizard → Themes step",
        ok=ok,
    )


def _check_fonts() -> BirthrightItem:
    if shutil.which("rpm") is None:
        return BirthrightItem("fonts", "IBM Plex fonts",
                              "rpm not available", ok=False)
    try:
        r = subprocess.run(["rpm", "-q", "ibm-plex-sans-fonts",
                             "ibm-plex-mono-fonts"],
                            capture_output=True, timeout=4)
        ok = r.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        ok = False
    return BirthrightItem(
        key="fonts", name="IBM Plex fonts",
        detail="Re-run Setup Wizard → Fonts step",
        ok=ok,
    )


def _check_plymouth() -> BirthrightItem:
    theme_dir = Path("/usr/share/plymouth/themes/mackes")
    active = False
    if shutil.which("plymouth-set-default-theme"):
        try:
            r = subprocess.run(["plymouth-set-default-theme"],
                                capture_output=True, text=True, timeout=4)
            active = "mackes" in (r.stdout or "")
        except (OSError, subprocess.TimeoutExpired):
            pass
    return BirthrightItem(
        key="plymouth", name="Mackes Plymouth boot theme",
        detail="Re-run Setup Wizard → Boot splash step",
        ok=(theme_dir.is_dir() and active),
    )


def _check_sudoers() -> BirthrightItem:
    """v1.4.1 — the NOPASSWD drop-in eliminates the prompt storm."""
    f = Path("/etc/sudoers.d/mackes-shell")
    return BirthrightItem(
        key="sudoers", name="Passwordless admin (sudoers drop-in)",
        detail="Reinstall mackes-shell or copy /usr/share/mackes-shell/"
               "data/sudoers.d/mackes-shell to /etc/sudoers.d/",
        ok=f.is_file(),
    )


def _check_panel_layout() -> BirthrightItem:
    """xfconf panel-0 should exist after Panel layout step."""
    if shutil.which("xfconf-query") is None:
        return BirthrightItem("panel", "Panel layout",
                              "xfconf-query unavailable", ok=False)
    try:
        r = subprocess.run(
            ["xfconf-query", "--channel", "xfce4-panel",
             "--property", "/plugins/plugin-101"],
            capture_output=True, text=True, timeout=4,
        )
        ok = r.returncode == 0 and "whiskermenu" in (r.stdout or "")
    except (OSError, subprocess.TimeoutExpired):
        ok = False
    return BirthrightItem(
        key="panel", name="Mackes panel layout",
        detail="Re-run Setup Wizard → Panel layout step",
        ok=ok,
    )


def _check_apps() -> BirthrightItem:
    """Heuristic: at least one of the preset's apps.install entries is present."""
    try:
        from mackes.presets import default_preset, load_preset
        from mackes.state import MackesState
        from mackes.app_mgmt import is_dnf_installed
        state = MackesState.load()
        preset = load_preset(state.active_preset) if state.active_preset else None
        if preset is None:
            preset = default_preset()
        if preset is None:
            return BirthrightItem("apps", "Curated apps", "no preset", ok=False)
        installable = preset.apps.get("install", []) or []
        if not installable:
            return BirthrightItem("apps", "Curated apps",
                                  "preset has no install list", ok=True)
        # Sample the first 5 to keep this cheap
        ok = any(is_dnf_installed(name) for name in installable[:5])
    except Exception:  # noqa: BLE001
        ok = False
    return BirthrightItem(
        key="apps", name="Curated apps",
        detail="Re-run Setup Wizard → Apps step",
        ok=ok,
    )


def _check_remote_desktop() -> BirthrightItem:
    return BirthrightItem(
        key="remote_desktop", name="Mesh Remote (xrdp + Guacamole)",
        detail="Re-run Setup Wizard → Remote desktop step",
        ok=(shutil.which("xrdp") is not None
            and Path("/etc/guacamole/noauth-config.xml").is_file()),
    )


def _check_fleet() -> BirthrightItem:
    has_ansible = shutil.which("ansible-pull") is not None
    timer_active = False
    if shutil.which("systemctl"):
        try:
            r = subprocess.run(
                ["systemctl", "is-enabled", "mackes-ansible-pull.timer"],
                capture_output=True, text=True, timeout=4,
            )
            timer_active = (r.stdout or "").strip() == "enabled"
        except (OSError, subprocess.TimeoutExpired):
            pass
    return BirthrightItem(
        key="fleet", name="Mesh Fleet (ansible-pull on 30-min timer)",
        detail="Re-run Setup Wizard → Fleet management step",
        ok=(has_ansible and timer_active),
    )


def _check_conky() -> BirthrightItem:
    cfg = Path(os.path.expanduser("~/.config/mackes-conky/mackes.conf"))
    autostart = Path(os.path.expanduser("~/.config/autostart/mackes-conky.desktop"))
    return BirthrightItem(
        key="conky", name="Conky HUD",
        detail="Re-run Setup Wizard → Conky HUD step",
        ok=(shutil.which("conky") is not None
            and cfg.is_file() and autostart.is_file()),
    )


def _check_flathub() -> BirthrightItem:
    if shutil.which("flatpak") is None:
        return BirthrightItem("flathub", "Flathub remote",
                              "flatpak not installed", ok=False)
    try:
        r = subprocess.run(["flatpak", "remotes", "--user",
                             "--columns=name"],
                            capture_output=True, text=True, timeout=4)
        ok = "flathub" in (r.stdout or "")
    except (OSError, subprocess.TimeoutExpired):
        ok = False
    return BirthrightItem(
        key="flathub", name="Flathub remote",
        detail="Re-run Setup Wizard → Flathub step",
        ok=ok,
    )


def _check_third_party_repos() -> BirthrightItem:
    pkg = Path("/etc/yum.repos.d/google-chrome.repo")  # ships with fws-repos
    rpmfusion = Path("/etc/yum.repos.d/rpmfusion-free.repo")
    return BirthrightItem(
        key="third_party_repos", name="Third-party repos (Chrome, RPM Fusion)",
        detail="Re-run Setup Wizard → Third-party repos step",
        ok=(pkg.exists() or rpmfusion.exists()),
    )


# ---------------------------------------------------------------------------
# Aggregator
# ---------------------------------------------------------------------------


def _check_maximizer() -> BirthrightItem:
    """v1.4.1 — always-maximize service shipped + wmctrl present."""
    has_wmctrl = shutil.which("wmctrl") is not None
    bin_present = Path("/usr/bin/mackes-maximizer").is_file()
    unit = Path("/usr/lib/systemd/user/mackes-maximizer.service")
    disabled = Path(os.path.expanduser(
        "~/.config/mackes-shell/maximizer.disabled"))
    ok = (has_wmctrl and bin_present and unit.is_file()
          and not disabled.exists())
    return BirthrightItem(
        key="maximizer", name="Always-maximize windows service",
        detail="Re-run Setup Wizard → Maximize windows step",
        ok=ok,
    )


_PROBES: List[Callable[[], BirthrightItem]] = [
    _check_themes,
    _check_fonts,
    _check_plymouth,
    _check_sudoers,
    _check_panel_layout,
    _check_apps,
    _check_remote_desktop,
    _check_fleet,
    _check_conky,
    _check_maximizer,
    _check_flathub,
    _check_third_party_repos,
]


def check_all() -> List[BirthrightItem]:
    items: List[BirthrightItem] = []
    for probe in _PROBES:
        try:
            items.append(probe())
        except Exception as e:  # noqa: BLE001
            items.append(BirthrightItem(
                key=probe.__name__, name=probe.__name__,
                detail=f"probe error: {e}", ok=False,
            ))
    return items


def missing() -> List[BirthrightItem]:
    return [i for i in check_all() if not i.ok]


def is_complete() -> bool:
    return not missing()
